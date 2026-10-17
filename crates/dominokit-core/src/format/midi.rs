//! Standard MIDI File ↔ `Clip`, following what Domino's own import and
//! export do (`probe/FINDINGS.md` §1–5).
//!
//! CONTROL events are Control Change Macros whose meaning comes from the
//! sound-module definition. Domino does not store the module in the file, so
//! the converter uses Domino's built-in `GMLevel1` table.
//!
//! Both directions stream: events are read straight out of the file bytes
//! and written straight into the output, one track per thread.

use std::collections::VecDeque;

use rayon::prelude::*;

use crate::format::chunk::{self, item};
use crate::format::clip::{decode_clip, Clip, NoteItem, TopNode, Track, TrackChild};
use crate::format::events::{self, EventValue, TextKind};
use crate::format::smf::{self, write_varlen, Ev, Smf, SmfError, TrackIter};
use crate::format::template::{SONG_REST, SONG_TAIL, TRACK_HEAD, TRACK_TAIL};

/// Domino caps note gates to this on MIDI import.
pub const MAX_IMPORT_GATE: u32 = 65_535;

/// GMLevel1 `Value/@Offset` for plain CC macros (stored = MIDI − offset).
fn cc_offset(cc: u8) -> i32 {
    if cc == 10 { 64 } else { 0 }
}

#[derive(Default)]
struct Warnings {
    counts: Vec<(&'static str, usize)>,
}

impl Warnings {
    fn add(&mut self, what: &'static str) {
        match self.counts.iter_mut().find(|(w, _)| *w == what) {
            Some((_, n)) => *n += 1,
            None => self.counts.push((what, 1)),
        }
    }

    fn merge(mut self, other: Warnings) -> Warnings {
        for (w, n) in other.counts {
            match self.counts.iter_mut().find(|(x, _)| *x == w) {
                Some((_, m)) => *m += n,
                None => self.counts.push((w, n)),
            }
        }
        self
    }

    fn finish(self) -> Vec<String> {
        self.counts.into_iter().map(|(w, n)| format!("{n} × {w}")).collect()
    }
}

// ---------------------------------------------------------------------------
// import
// ---------------------------------------------------------------------------

/// Convert an SMF into a Domino clip (a whole project: the first track
/// becomes the conductor track). Problems are reported in `clip.warnings`.
pub fn clip_from_smf_bytes(bytes: &[u8]) -> Result<Clip, SmfError> {
    let c = smf::chunks(bytes)?;
    let imported: Vec<Imported> = if c.format == 0 && c.tracks.len() == 1 {
        split_format0(c.tracks[0])?
            .into_par_iter()
            .enumerate()
            .map(|(i, events)| import_track(events.into_iter().map(Ok), i == 0))
            .collect::<Result<_, _>>()?
    } else {
        c.tracks
            .par_iter()
            .enumerate()
            .map(|(i, t)| import_track(TrackIter::new(t), i == 0))
            .collect::<Result<_, _>>()?
    };

    let song_name = imported.first().map(|t| t.name.clone()).unwrap_or_default();
    let copyright = imported.iter().find(|t| !t.copyright.is_empty()).map(|t| t.copyright.clone()).unwrap_or_default();
    let mut root = item(1000, &song_name);
    root.extend_from_slice(&item(1001, &copyright));
    root.extend_from_slice(&item(1002, &c.ppq.to_le_bytes()));
    root.extend_from_slice(SONG_REST);
    let mut clip = decode_clip(&root);
    let mut warnings = Warnings::default();
    for t in imported {
        warnings = warnings.merge(t.warnings);
        clip.nodes.push(TopNode::Track(t.track));
    }
    clip.nodes.extend(decode_clip(SONG_TAIL).nodes);
    clip.warnings = warnings.finish();
    Ok(clip)
}

/// Convert a parsed SMF (tests and small tools).
pub fn clip_from_smf(smf: &Smf) -> Clip {
    clip_from_smf_bytes(&smf::write(smf)).expect("a written SMF parses")
}

/// Format 0: meta events stay on the conductor track, channel events are
/// split into one track per channel (in channel order).
fn split_format0(track: &[u8]) -> Result<Vec<Vec<(u32, Ev<'_>)>>, SmfError> {
    let mut conductor = Vec::new();
    let mut per_channel: Vec<Vec<(u32, Ev)>> = vec![Vec::new(); 16];
    let mut end = 0u32;
    for ev in TrackIter::new(track) {
        let (tick, e) = ev?;
        end = end.max(tick);
        match e {
            Ev::Channel { status, .. } => per_channel[(status & 0x0F) as usize].push((tick, e)),
            Ev::Meta { kind: 0x2F, .. } => {}
            _ => conductor.push((tick, e)),
        }
    }
    let eot = (end, Ev::Meta { kind: 0x2F, data: &[] });
    let mut out = vec![conductor];
    out.extend(per_channel.into_iter().filter(|t| !t.is_empty()));
    for t in &mut out {
        t.push(eot);
    }
    Ok(out)
}

struct Imported {
    track: Track,
    name: Vec<u8>,
    copyright: Vec<u8>,
    warnings: Warnings,
}

fn import_track<'a>(
    events: impl Iterator<Item = Result<(u32, Ev<'a>), SmfError>>,
    conductor: bool,
) -> Result<Imported, SmfError> {
    let mut w = Warnings::default();
    let head = chunk::read_all(TRACK_HEAD).items;
    let mut children: Vec<TrackChild> = head.into_iter().map(TrackChild::raw).collect();
    // open notes per (channel, key), oldest first: indices into `children`
    let mut open: Vec<VecDeque<usize>> = vec![VecDeque::new(); 16 * 128];
    let mut name: Vec<u8> = Vec::new();
    let mut copyright: Vec<u8> = Vec::new();
    let mut port: Option<u8> = None;
    let mut channel: Option<u8> = None;
    let mut end_of_track = 0u32;

    fn close(children: &mut [TrackChild], idx: usize, off: u32, w: &mut Warnings) {
        if let TrackChild::Note(n) = &mut children[idx] {
            let mut gate = off.saturating_sub(n.tick).max(1);
            if gate > MAX_IMPORT_GATE {
                gate = MAX_IMPORT_GATE;
                w.add("note longer than 65535 ticks (shortened, like Domino does)");
            }
            n.gate = gate;
        }
    }

    for ev in events {
        let (tick, ev) = ev?;
        end_of_track = end_of_track.max(tick);
        match ev {
            Ev::Channel { status, a, b } => {
                let ch = status & 0x0F;
                match channel {
                    None => channel = Some(ch),
                    Some(c) if c != ch => w.add("track with events on several channels"),
                    _ => {}
                }
                let slot = (ch as usize) << 7 | (a & 0x7F) as usize;
                let event = match status & 0xF0 {
                    0x90 if b > 0 => {
                        open[slot].push_back(children.len());
                        children.push(TrackChild::Note(NoteItem::canonical(tick, 1, a, b)));
                        continue;
                    }
                    0x80 | 0x90 => {
                        if let Some(idx) = open[slot].pop_front() {
                            close(&mut children, idx, tick, &mut w);
                        }
                        continue;
                    }
                    0xB0 => events::control(tick, a as u16, (b as i32 - cc_offset(a)) as f32),
                    0xE0 => events::control(tick, 130, (((b as i32) << 7 | a as i32) - 8192) as f32),
                    0xD0 => events::control(tick, 132, a as f32),
                    0xC0 => events::program(tick, a.wrapping_add(1)),
                    _ => {
                        w.add("polyphonic key pressure event (GMLevel1 has no macro for it; dropped)");
                        continue;
                    }
                };
                children.push(TrackChild::event(event));
            }
            Ev::Meta { kind, data } => {
                let event = match kind {
                    0x02 => {
                        if copyright.is_empty() {
                            copyright = data.to_vec();
                        }
                        continue;
                    }
                    0x03 => {
                        if name.is_empty() {
                            name = data.to_vec();
                        }
                        continue;
                    }
                    0x21 => {
                        port = data.first().copied();
                        continue;
                    }
                    0x2F => break,
                    0x51 if data.len() == 3 => {
                        let us = u32::from_be_bytes([0, data[0], data[1], data[2]]).max(1);
                        events::tempo(tick, (60_000_000f64 / us as f64) as f32)
                    }
                    0x58 if data.len() >= 2 => events::time_sig(tick, data[0], 1u8.checked_shl(data[1] as u32).unwrap_or(0)),
                    0x59 if data.len() >= 2 => events::key_sig(tick, events::key_sig_from_smf(data[0] as i8, data[1] != 0)),
                    k => match TextKind::from_meta_type(k) {
                        Some(kind) => events::text(tick, kind, data),
                        None => {
                            w.add("meta event with no Domino equivalent (dropped)");
                            continue;
                        }
                    },
                };
                children.push(TrackChild::event(event));
            }
            Ev::Sysex { .. } => w.add("sysex event (not supported; dropped)"),
        }
    }
    // notes never switched off end at the end of the track
    for q in &open {
        for &idx in q {
            close(&mut children, idx, end_of_track, &mut w);
        }
    }

    let mut track = Track { children, trailing: Vec::new() };
    let ch = channel.unwrap_or(0);
    track.set_setting(1000, (port.unwrap_or(0) as u16).to_le_bytes().to_vec());
    track.set_setting(1001, vec![ch]);
    track.set_setting(1002, name.clone());
    track.set_setting(1003, vec![conductor as u8]);
    track.set_setting(1004, vec![(ch == 9 && !conductor) as u8]);
    track.children.push(TrackChild::length(end_of_track));
    track.children.extend(chunk::read_all(TRACK_TAIL).items.into_iter().map(TrackChild::raw));
    // events arrive in tick order, so the track already is
    Ok(Imported { track, name, copyright, warnings: w })
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

/// Delta-time event writer for one `MTrk` body.
struct TrackWriter {
    out: Vec<u8>,
    last: u32,
}

impl TrackWriter {
    fn new(capacity: usize) -> Self {
        TrackWriter { out: Vec::with_capacity(capacity), last: 0 }
    }

    #[inline]
    fn at(&mut self, tick: u32) {
        let tick = tick.max(self.last);
        write_varlen(&mut self.out, tick - self.last);
        self.last = tick;
    }

    #[inline]
    fn channel(&mut self, tick: u32, bytes: &[u8]) {
        self.at(tick);
        self.out.extend_from_slice(bytes);
    }

    fn meta(&mut self, tick: u32, kind: u8, data: &[u8]) {
        self.at(tick);
        self.out.extend_from_slice(&[0xFF, kind]);
        write_varlen(&mut self.out, data.len() as u32);
        self.out.extend_from_slice(data);
    }
}

/// Convert a clip to format-1 SMF bytes. A clip without a conductor track
/// (e.g. a clipboard copy) gets an empty one in front, so every clip track
/// keeps its own SMF track when the file is opened in Domino again.
pub fn smf_bytes_from_clip(clip: &Clip) -> Result<(Vec<u8>, Vec<String>), String> {
    let root = |tag: u16| -> Vec<u8> {
        clip.nodes
            .iter()
            .find_map(|n| match n {
                TopNode::Raw(i) if i.tag == tag => Some(i.body.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let (song_name, copyright) = (root(1000), root(1001));

    let tracks: Vec<&Track> = clip.tracks().collect();
    let mut bodies: Vec<(Vec<u8>, Warnings)> =
        tracks.par_iter().map(|t| export_track(t, &song_name, &copyright)).collect();
    if !tracks.iter().any(|t| t.is_conductor()) {
        let mut w = TrackWriter::new(64);
        conductor_head(&mut w, &song_name, &copyright);
        w.meta(0, 0x2F, &[]);
        bodies.insert(0, (w.out, Warnings::default()));
    }

    // the SMF header counts tracks in 16 bits and each track's size in 32
    if bodies.len() > u16::MAX as usize {
        return Err(format!("{} tracks, but a MIDI file holds at most 65535", bodies.len()));
    }
    if let Some(k) = bodies.iter().position(|(b, _)| b.len() > u32::MAX as usize) {
        return Err(format!("MIDI track {} would be over 4096 MB, the most a MIDI file can store", k + 1));
    }
    let total: usize = 14 + bodies.iter().map(|(b, _)| 8 + b.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(bodies.len() as u16).to_be_bytes());
    out.extend_from_slice(&clip.ppq().unwrap_or(480).to_be_bytes());
    let mut warnings = Warnings::default();
    for (body, w) in bodies {
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        warnings = warnings.merge(w);
    }
    Ok((out, warnings.finish()))
}

/// Convert a clip to a parsed SMF (tests and small tools).
pub fn smf_from_clip(clip: &Clip) -> (Smf, Vec<String>) {
    let (bytes, warnings) = smf_bytes_from_clip(clip).expect("within MIDI limits");
    (smf::parse(&bytes).expect("own output parses"), warnings)
}

fn conductor_head(w: &mut TrackWriter, song_name: &[u8], copyright: &[u8]) {
    w.meta(0, 0x03, song_name);
    if !copyright.is_empty() {
        w.meta(0, 0x02, copyright);
    }
}

fn clamp7(v: f32) -> u8 {
    (v as i32).clamp(0, 127) as u8
}

/// Something that sounds at a tick: a note-on or an event. `class` orders
/// same-tick items the way Domino does: everything in its original order,
/// then time signatures, then key signatures.
#[derive(Clone, Copy)]
struct On {
    tick: u32,
    class: u8,
    idx: usize,
}

fn export_track(track: &Track, song_name: &[u8], copyright: &[u8]) -> (Vec<u8>, Warnings) {
    let ch = track.channel().unwrap_or(0) & 0x0F;
    let mut warn = Warnings::default();
    let mut ons: Vec<On> = Vec::with_capacity(track.children.len());
    let mut offs: Vec<(u32, u8)> = Vec::with_capacity(track.children.len());
    for (idx, child) in track.children.iter().enumerate() {
        match child {
            TrackChild::Note(_) | TrackChild::OddNote(_) => {
                let n = child.as_note().unwrap();
                ons.push(On { tick: n.tick, class: 1, idx });
                offs.push(((n.tick as u64 + n.gate as u64).min(u32::MAX as u64) as u32, n.key.min(127)));
            }
            TrackChild::Event(e) => {
                if let Some(pos) = e.pos {
                    let class = match e.id {
                        2015 => 2,
                        2016 => 3,
                        _ => 1,
                    };
                    ons.push(On { tick: pos, class, idx });
                }
            }
            _ => {}
        }
    }
    // already in order for anything Domino wrote; sort (stably) otherwise
    if !ons.windows(2).all(|p| (p[0].tick, p[0].class) <= (p[1].tick, p[1].class)) {
        ons.par_sort_by_key(|o| (o.tick, o.class));
    }
    if !offs.windows(2).all(|p| p[0].0 <= p[1].0) {
        offs.par_sort_by_key(|o| o.0);
    }

    let mut w = TrackWriter::new(offs.len() * 8 + ons.len() * 8 + 64);
    if track.is_conductor() {
        conductor_head(&mut w, song_name, copyright);
    } else {
        let name = track.setting(1002).map(|i| i.body.as_slice()).unwrap_or(&[]);
        w.meta(0, 0x03, name);
        w.meta(0, 0x21, &[track.port().unwrap_or(0) as u8]);
    }

    // merge: at the same tick note-offs go first (a re-struck key isn't cut short)
    let (mut i, mut j) = (0usize, 0usize);
    while i < ons.len() || j < offs.len() {
        if j < offs.len() && (i == ons.len() || offs[j].0 <= ons[i].tick) {
            let (tick, key) = offs[j];
            w.channel(tick, &[0x80 | ch, key, 0]);
            j += 1;
            continue;
        }
        let on = ons[i];
        i += 1;
        let child = &track.children[on.idx];
        match (child.as_note(), child.as_event()) {
            (Some(n), _) => w.channel(on.tick, &[0x90 | ch, n.key.min(127), n.vel.clamp(1, 127)]),
            (_, Some(e)) => write_event(&mut w, on.tick, ch, &events::decode(e), &mut warn),
            _ => unreachable!(),
        }
    }
    let eot = track.length().unwrap_or(0).max(w.last);
    w.meta(eot, 0x2F, &[]);
    (w.out, warn)
}

fn write_event(w: &mut TrackWriter, t: u32, ch: u8, value: &EventValue, warn: &mut Warnings) {
    let cc = |w: &mut TrackWriter, n: u8, v: u8| w.channel(t, &[0xB0 | ch, n, v]);
    match *value {
        EventValue::Program { patch } => w.channel(t, &[0xC0 | ch, patch.saturating_sub(1) & 0x7F]),
        EventValue::Control { ccm, value, .. } => match ccm {
            121 | 123 => cc(w, ccm as u8, 0),
            0..=127 => cc(w, ccm as u8, clamp7(value + cc_offset(ccm as u8) as f32)),
            130 => {
                let v = ((value as i32) + 8192).clamp(0, 16383);
                w.channel(t, &[0xE0 | ch, (v & 0x7F) as u8, (v >> 7) as u8]);
            }
            132 => w.channel(t, &[0xD0 | ch, clamp7(value)]),
            140 => {
                cc(w, 101, 0);
                cc(w, 100, 0);
                cc(w, 6, clamp7(value));
            }
            141 => {
                let v = ((value as i32) + 8192).clamp(0, 16383);
                cc(w, 101, 0);
                cc(w, 100, 1);
                cc(w, 6, (v >> 7) as u8);
                cc(w, 38, (v & 0x7F) as u8);
            }
            142 => {
                cc(w, 101, 0);
                cc(w, 100, 2);
                cc(w, 6, clamp7(value + 64.0));
            }
            200 => {
                w.at(t);
                w.out.extend_from_slice(&[0xF0, 5, 0x7E, 0x7F, 0x09, 0x01, 0xF7]);
            }
            128 => {}
            _ => warn.add("control macro unknown to GMLevel1 (not exported)"),
        },
        EventValue::Tempo { bpm } => {
            let us = ((60_000_000f32 / bpm.max(0.001)) as u32).clamp(1, 0xFF_FFFF);
            w.meta(t, 0x51, &us.to_be_bytes()[1..]);
        }
        EventValue::TimeSig { num, den } => {
            let dd = (den.max(1) as u32).trailing_zeros() as u8;
            w.meta(t, 0x58, &[num, dd, 24, 8]);
        }
        EventValue::KeySig { key } => {
            let (sf, minor) = events::key_sig_to_smf(key);
            w.meta(t, 0x59, &[sf as u8, minor as u8]);
        }
        EventValue::Text { kind, ref bytes } => w.meta(t, kind.meta_type(), bytes),
        EventValue::Other => warn.add("Domino-only event (measure link, chord, scale, variables, sysex…) not exported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::smf::{SmfEvent, SmfTrack};

    #[test]
    fn note_round_trip_through_smf() {
        let smf = Smf {
            format: 1,
            ppq: 480,
            tracks: vec![
                SmfTrack { events: vec![(0, SmfEvent::meta(0x03, b"Song")), (0, SmfEvent::meta(0x2F, &[]))] },
                SmfTrack {
                    events: vec![
                        (0, SmfEvent::meta(0x03, b"Lead")),
                        (0, SmfEvent::channel(0x92, 60, 100)),
                        (240, SmfEvent::channel(0x92, 60, 0)),
                        (240, SmfEvent::channel(0xB2, 10, 64)),
                        (960, SmfEvent::meta(0x2F, &[])),
                    ],
                },
            ],
        };
        let clip = clip_from_smf(&smf);
        assert_eq!(clip.ppq(), Some(480));
        assert_eq!(clip.track_count(), 2);
        let lead = clip.tracks().nth(1).unwrap();
        assert!(clip.tracks().next().unwrap().is_conductor());
        assert_eq!(lead.channel(), Some(2));
        let n = lead.notes().next().unwrap();
        assert_eq!((n.tick, n.gate, n.key, n.vel), (0, 240, 60, 100));
        assert_eq!(lead.length(), Some(960));
        // pan 64 is stored as 0
        let e = lead.events().next().unwrap();
        assert_eq!(events::decode(e), EventValue::Control { ccm: 10, gate: 0.0, value: 0.0 });

        let (back, warnings) = smf_bytes_from_clip(&clip).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let again = clip_from_smf_bytes(&back).unwrap();
        assert_eq!(crate::format::clip::encode_clip(&again), crate::format::clip::encode_clip(&clip));
    }

    #[test]
    fn overlapping_same_key_notes_pair_first_in_first_out() {
        let smf = Smf {
            format: 1,
            ppq: 480,
            tracks: vec![SmfTrack {
                events: vec![
                    (0, SmfEvent::channel(0x90, 60, 100)),
                    (100, SmfEvent::channel(0x90, 60, 90)),
                    (200, SmfEvent::channel(0x80, 60, 0)),
                    (300, SmfEvent::channel(0x80, 60, 0)),
                    (400, SmfEvent::meta(0x2F, &[])),
                ],
            }],
        };
        let clip = clip_from_smf(&smf);
        let g: Vec<_> = clip.note_views().iter().map(|n| (n.tick, n.gate)).collect();
        assert_eq!(g, vec![(0, 200), (100, 200)]);
    }

    #[test]
    fn format0_is_split_by_channel() {
        let smf = Smf {
            format: 0,
            ppq: 96,
            tracks: vec![SmfTrack {
                events: vec![
                    (0, SmfEvent::meta(0x51, &[0x07, 0xA1, 0x20])),
                    (0, SmfEvent::channel(0x91, 60, 100)),
                    (0, SmfEvent::channel(0x90, 50, 100)),
                    (96, SmfEvent::channel(0x81, 60, 0)),
                    (96, SmfEvent::channel(0x80, 50, 0)),
                    (96, SmfEvent::meta(0x2F, &[])),
                ],
            }],
        };
        let clip = clip_from_smf(&smf);
        assert_eq!(clip.track_count(), 3);
        let chans: Vec<u8> = clip.tracks().skip(1).map(|t| t.channel().unwrap()).collect();
        assert_eq!(chans, vec![0, 1]);
        assert_eq!(clip.tracks().next().unwrap().events().count(), 1);
    }
}
