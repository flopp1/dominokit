//! The `Clip` model and its lossless codec.
//!
//! Goal: `encode_clip(decode_clip(x)) == x` for any well-formed unpacked
//! chunk stream, even one full of undocumented chunks.
//!
//! Performance:
//! - A note in Domino's canonical 40-byte layout (every note Domino writes)
//!   is a plain 12-byte `Copy` value, and a track child is 16 bytes; only
//!   notes with an unusual layout keep their original inner chunks, boxed.
//! - Decoding pattern-matches the canonical layout straight out of the byte
//!   slice. Big tracks are split at note boundaries and decoded on all cores
//!   (the split points are verified: each piece must end exactly where the
//!   next begins, otherwise the track is decoded in one go).
//! - Encoding sizes everything first, then fills disjoint regions of one
//!   buffer in parallel — per track, and per block of children within a
//!   big track.

use rayon::prelude::*;

use crate::format::chunk::{self, Item, ParseResult};

/// Bytes of a canonical note's body (34) and with its header (40).
pub const NOTE_BODY_LEN: usize = 34;
pub const NOTE_LEN: usize = 40;

/// Tracks bigger than this are decoded / encoded in parallel pieces.
const PAR_BYTES: usize = 1 << 20;
const PAR_CHILDREN: usize = 1 << 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopNode {
    /// Every song-level item, kept in original position.
    Raw(Item),
    /// Tag 1003.
    Track(Track),
}

impl TopNode {
    pub fn as_track(&self) -> Option<&Track> {
        match self {
            TopNode::Track(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_track_mut(&mut self) -> Option<&mut Track> {
        match self {
            TopNode::Track(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Track {
    pub children: Vec<TrackChild>,
    /// Unparsable tail of the track body (preserved verbatim).
    pub trailing: Vec<u8>,
}

impl Track {
    pub fn notes(&self) -> impl Iterator<Item = &NoteItem> {
        self.children.iter().filter_map(|c| c.as_note())
    }

    pub fn notes_mut(&mut self) -> impl Iterator<Item = &mut NoteItem> {
        self.children.iter_mut().filter_map(|c| c.as_note_mut())
    }

    pub fn note_count(&self) -> usize {
        if self.children.len() >= PAR_CHILDREN {
            self.children.par_iter().filter(|c| c.is_note()).count()
        } else {
            self.children.iter().filter(|c| c.is_note()).count()
        }
    }

    pub fn events(&self) -> impl Iterator<Item = &EventItem> {
        self.children.iter().filter_map(|c| c.as_event())
    }

    pub fn length(&self) -> Option<u32> {
        self.children.iter().rev().find_map(|c| match c {
            TrackChild::Length(l) => Some(l.length),
            _ => None,
        })
    }

    /// Replace (or append) the end-of-track / copied-range length (2009).
    pub fn set_length(&mut self, length: u32) {
        for child in self.children.iter_mut().rev() {
            if let TrackChild::Length(l) = child {
                l.length = length;
                return;
            }
        }
        self.children.push(TrackChild::length(length));
    }

    /// Raw settings chunk with this tag, if present. Settings sit at the
    /// start (and the tail chunks at the end), so the search stops at the
    /// first note instead of scanning the whole track.
    pub fn setting(&self, tag: u16) -> Option<&Item> {
        fn found(c: &TrackChild, tag: u16) -> Option<&Item> {
            match c {
                TrackChild::Raw(item) if item.tag == tag => Some(item),
                _ => None,
            }
        }
        let found = |c| found(c, tag);
        self.children
            .iter()
            .take_while(|c| !c.is_note())
            .find_map(found)
            .or_else(|| self.children.iter().rev().take_while(|c| !c.is_note()).find_map(found))
            .or_else(|| self.children.iter().find_map(found))
    }

    /// Replace the body of a settings chunk (appended if missing).
    pub fn set_setting(&mut self, tag: u16, body: Vec<u8>) {
        for child in &mut self.children {
            if let TrackChild::Raw(item) = child {
                if item.tag == tag {
                    item.body = body;
                    return;
                }
            }
        }
        self.children.push(TrackChild::raw(Item { tag, body }));
    }

    /// Track `1003 = 1` marks Domino's conductor track (tempo, time and key
    /// signatures). Only a project's first track has it.
    pub fn is_conductor(&self) -> bool {
        self.setting(1003).and_then(|i| i.u8_value()) == Some(1)
    }

    /// Output port (track 1000, u16; 0 = port A).
    pub fn port(&self) -> Option<u16> {
        self.setting(1000).and_then(|i| i.u16_value())
    }

    /// Channel (track 1001 u8 chunk), if present with the documented size.
    pub fn channel(&self) -> Option<u8> {
        self.setting(1001).and_then(|i| i.u8_value())
    }

    /// Display name (track 1002 string chunk, lossy UTF-8).
    pub fn display_name(&self) -> Option<String> {
        self.setting(1002).map(|i| String::from_utf8_lossy(&i.body).into_owned())
    }

    /// The track's time span: its 2009 length, extended to cover every note
    /// end and event position.
    pub fn span(&self) -> u32 {
        let end = |c: &TrackChild| match c {
            TrackChild::Event(e) => e.pos.map_or(0, |p| p.saturating_add(1)),
            c => c.as_note().map_or(0, NoteItem::end_u32),
        };
        let content = if self.children.len() >= PAR_CHILDREN {
            self.children.par_iter().map(end).max()
        } else {
            self.children.iter().map(end).max()
        };
        self.length().unwrap_or(0).max(content.unwrap_or(0)).max(1)
    }

    /// Size of the encoded track body (without the 1003 header).
    fn body_len(&self) -> usize {
        let children: usize = if self.children.len() >= PAR_CHILDREN {
            self.children.par_iter().map(TrackChild::encoded_len).sum()
        } else {
            self.children.iter().map(TrackChild::encoded_len).sum()
        };
        children + self.trailing.len()
    }

    fn write_body(&self, region: &mut [u8], length_override: Option<u32>) {
        let write = |children: &[TrackChild], out: &mut Out<'_>| {
            for child in children {
                match (child, length_override) {
                    (TrackChild::Length(l), Some(len)) => LengthItem { length: len, ..(**l).clone() }.write(out),
                    _ => child.write(out),
                }
            }
        };
        let (body, trailing) = region.split_at_mut(region.len() - self.trailing.len());
        trailing.copy_from_slice(&self.trailing);
        if self.children.len() < PAR_CHILDREN {
            write(&self.children, &mut Out::new(body));
            return;
        }
        // blocks of children into disjoint parts of the region
        let blocks: Vec<&[TrackChild]> = self.children.chunks(PAR_CHILDREN).collect();
        let lens: Vec<usize> = blocks.par_iter().map(|b| b.iter().map(TrackChild::encoded_len).sum()).collect();
        let mut parts = Vec::with_capacity(blocks.len());
        let mut rest = body;
        for len in lens {
            let (head, tail) = rest.split_at_mut(len);
            parts.push(head);
            rest = tail;
        }
        parts.into_par_iter().zip(blocks).for_each(|(part, block)| write(block, &mut Out::new(part)));
    }
}

/// One chunk inside a track. Canonical notes are stored inline; everything
/// rarer is boxed, so the enum is 16 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackChild {
    /// Settings (1000-1022), anything undocumented.
    Raw(Box<Item>),
    /// 2001 in the canonical layout.
    Note(NoteItem),
    /// 2001 in any other layout (extra or reordered fields, no velocity).
    OddNote(Box<OddNote>),
    /// Any other event container (2002 and up).
    Event(Box<EventItem>),
    /// 2009 end of track (the copied range's length in a clipboard copy).
    Length(Box<LengthItem>),
}

impl TrackChild {
    pub fn raw(item: Item) -> TrackChild {
        TrackChild::Raw(Box::new(item))
    }

    pub fn event(event: EventItem) -> TrackChild {
        TrackChild::Event(Box::new(event))
    }

    pub fn length(length: u32) -> TrackChild {
        TrackChild::Length(Box::new(LengthItem::canonical(length)))
    }

    #[inline]
    pub fn is_note(&self) -> bool {
        matches!(self, TrackChild::Note(_) | TrackChild::OddNote(_))
    }

    #[inline]
    pub fn as_note(&self) -> Option<&NoteItem> {
        match self {
            TrackChild::Note(n) => Some(n),
            TrackChild::OddNote(o) => Some(&o.note),
            _ => None,
        }
    }

    #[inline]
    pub fn as_note_mut(&mut self) -> Option<&mut NoteItem> {
        match self {
            TrackChild::Note(n) => Some(n),
            TrackChild::OddNote(o) => Some(&mut o.note),
            _ => None,
        }
    }

    pub fn as_event(&self) -> Option<&EventItem> {
        match self {
            TrackChild::Event(e) => Some(e),
            _ => None,
        }
    }

    /// The same child with its note replaced (keeps an odd note's layout).
    pub fn with_note(&self, note: NoteItem) -> TrackChild {
        match self {
            TrackChild::OddNote(o) => TrackChild::OddNote(Box::new(OddNote { note, ..(**o).clone() })),
            _ => TrackChild::Note(note),
        }
    }

    #[inline]
    pub fn encoded_len(&self) -> usize {
        match self {
            TrackChild::Note(_) => NOTE_LEN,
            TrackChild::Raw(i) => 6 + i.body.len(),
            TrackChild::OddNote(o) => o.encoded_len(),
            TrackChild::Event(e) => 6 + e.item.body.len(),
            TrackChild::Length(l) => l.encoded_len(),
        }
    }

    #[inline]
    fn write(&self, out: &mut Out<'_>) {
        match self {
            TrackChild::Note(n) => n.write(out),
            TrackChild::Raw(i) => out.item(i.tag, &i.body),
            TrackChild::OddNote(o) => o.write(out),
            TrackChild::Event(e) => e.write(out),
            TrackChild::Length(l) => l.write(out),
        }
    }

    /// Standalone encoding of this child (chunk header included).
    pub fn encode(&self) -> Vec<u8> {
        let mut v = vec![0u8; self.encoded_len()];
        self.write(&mut Out::new(&mut v));
        v
    }

    /// The inner chunks of a note as they will be written (Inspector).
    pub fn note_fields(&self) -> Vec<Item> {
        chunk::read_all(&self.encode()[6..]).items
    }
}

/// Any non-note event. `pos` is the value of its inner `1001` (u32) when
/// present; everything else in the body is kept verbatim, only `pos` is
/// ever patched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventItem {
    pub id: u16,
    pub pos: Option<u32>,
    pub item: Item,
}

impl EventItem {
    fn write(&self, out: &mut Out<'_>) {
        let start = out.pos;
        out.item(self.id, &self.item.body);
        if let Some(p) = self.pos {
            patch_pos_in_body(&mut out.buf[start + 6..out.pos], p);
        }
    }

    /// Re-encode the container with `pos` patched into its inner `1001`.
    pub fn encode(&self) -> Vec<u8> {
        TrackChild::Event(Box::new(self.clone())).encode()
    }
}

#[inline]
fn read_header(body: &[u8], i: usize) -> Option<(u16, usize, usize)> {
    let h = body.get(i..i + 6)?;
    let tag = u16::from_le_bytes([h[0], h[1]]);
    let size = u32::from_le_bytes([h[2], h[3], h[4], h[5]]) as usize;
    let end = (i + 6).checked_add(size).filter(|&e| e <= body.len())?;
    Some((tag, i + 6, end))
}

/// Patch the first `1001` chunk with a 4-byte body in `body` to `pos`.
fn patch_pos_in_body(body: &mut [u8], pos: u32) {
    let mut i = 0usize;
    while let Some((tag, start, end)) = read_header(body, i) {
        if tag == 1001 && end - start == 4 {
            body[start..end].copy_from_slice(&pos.to_le_bytes());
            return;
        }
        i = end;
    }
}

/// Find the first inner `1001` chunk with a 4-byte body.
fn find_pos(body: &[u8]) -> Option<u32> {
    let mut i = 0usize;
    while let Some((tag, start, end)) = read_header(body, i) {
        if tag == 1001 && end - start == 4 {
            return Some(u32::from_le_bytes(body[start..end].try_into().unwrap()));
        }
        i = end;
    }
    None
}

/// A note: position, length, key, velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteItem {
    pub tick: u32,
    pub gate: u32,
    pub key: u8,
    pub vel: u8,
}

/// The canonical note: 2001 { 1001 tick, 2001 key, 2002 vel, 2003 gate }.
const NOTE_TEMPLATE: [u8; NOTE_LEN] = [
    0xD1, 0x07, 0x22, 0, 0, 0, // 2001, 34 bytes
    0xE9, 0x03, 4, 0, 0, 0, 0, 0, 0, 0, // 1001 tick
    0xD1, 0x07, 1, 0, 0, 0, 0, // 2001 key
    0xD2, 0x07, 1, 0, 0, 0, 0, // 2002 velocity
    0xD3, 0x07, 4, 0, 0, 0, 0, 0, 0, 0, // 2003 gate
];

/// A canonical note body (34 bytes), or `None`.
#[inline]
fn canonical_note(b: &[u8]) -> Option<NoteItem> {
    let b: &[u8; NOTE_BODY_LEN] = b.try_into().ok()?;
    if b[0..6] != NOTE_TEMPLATE[6..12]
        || b[10..16] != NOTE_TEMPLATE[16..22]
        || b[17..23] != NOTE_TEMPLATE[23..29]
        || b[24..30] != NOTE_TEMPLATE[30..36]
    {
        return None;
    }
    Some(NoteItem {
        tick: u32::from_le_bytes([b[6], b[7], b[8], b[9]]),
        key: b[16],
        vel: b[23],
        gate: u32::from_le_bytes([b[30], b[31], b[32], b[33]]),
    })
}

impl NoteItem {
    /// Canonical 40-byte note, the layout Domino writes.
    pub const fn canonical(tick: u32, gate: u32, key: u8, vel: u8) -> Self {
        NoteItem { tick, gate, key, vel }
    }

    pub fn set_tick(&mut self, tick: u32) {
        self.tick = tick;
    }

    pub fn set_gate(&mut self, gate: u32) {
        self.gate = gate;
    }

    pub fn set_key(&mut self, key: u8) {
        self.key = key;
    }

    #[inline]
    fn write(&self, out: &mut Out<'_>) {
        let mut b = NOTE_TEMPLATE;
        b[12..16].copy_from_slice(&self.tick.to_le_bytes());
        b[22] = self.key;
        b[29] = self.vel;
        b[36..40].copy_from_slice(&self.gate.to_le_bytes());
        out.put(&b);
    }

    /// Canonical encoding (40 bytes).
    pub fn encode(&self) -> Vec<u8> {
        TrackChild::Note(*self).encode()
    }

    /// End tick (tick + gate), in i64 to avoid wrap.
    pub fn end(&self) -> i64 {
        self.tick as i64 + self.gate as i64
    }

    #[inline]
    pub fn end_u32(&self) -> u32 {
        self.tick.saturating_add(self.gate)
    }
}

/// A note read in a non-canonical layout: its inner chunks are kept so it
/// round-trips byte for byte; the four known fields are patched on write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OddNote {
    pub note: NoteItem,
    pub children: Vec<Item>,
    idx_tick: usize,
    idx_key: usize,
    /// `None`: the note has no velocity chunk (`note.vel` shows 100).
    idx_vel: Option<usize>,
    idx_gate: usize,
}

impl OddNote {
    fn encoded_len(&self) -> usize {
        6 + self.children.iter().map(|c| 6 + c.body.len()).sum::<usize>()
    }

    fn write(&self, out: &mut Out<'_>) {
        out.put(&2001u16.to_le_bytes());
        out.put(&((self.encoded_len() - 6) as u32).to_le_bytes());
        let n = &self.note;
        for (i, c) in self.children.iter().enumerate() {
            if i == self.idx_tick {
                out.item(c.tag, &n.tick.to_le_bytes());
            } else if i == self.idx_key {
                out.item(c.tag, &[n.key]);
            } else if Some(i) == self.idx_vel {
                out.item(c.tag, &[n.vel]);
            } else if i == self.idx_gate {
                out.item(c.tag, &n.gate.to_le_bytes());
            } else {
                out.item(c.tag, &c.body);
            }
        }
    }
}

/// 2009 end of track, lossless: keeps any extra inner chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LengthItem {
    pub length: u32,
    pub children: Vec<Item>,
    pub idx_length: usize,
}

impl LengthItem {
    pub fn canonical(length: u32) -> Self {
        LengthItem {
            length,
            children: vec![Item { tag: 1001, body: length.to_le_bytes().to_vec() }],
            idx_length: 0,
        }
    }

    pub fn set_length(&mut self, length: u32) {
        self.length = length;
    }

    fn encoded_len(&self) -> usize {
        6 + self.children.iter().map(|c| 6 + c.body.len()).sum::<usize>()
    }

    fn write(&self, out: &mut Out<'_>) {
        out.put(&2009u16.to_le_bytes());
        out.put(&((self.encoded_len() - 6) as u32).to_le_bytes());
        for (i, c) in self.children.iter().enumerate() {
            if i == self.idx_length {
                out.item(c.tag, &self.length.to_le_bytes());
            } else {
                out.item(c.tag, &c.body);
            }
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        TrackChild::Length(Box::new(self.clone())).encode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Clip {
    pub nodes: Vec<TopNode>,
    /// Unparsable tail of the unpacked stream (preserved verbatim).
    pub trailing: Vec<u8>,
    /// Non-fatal problems found while decoding.
    pub warnings: Vec<String>,
}

impl Clip {
    pub fn ppq(&self) -> Option<u16> {
        self.nodes.iter().find_map(|n| match n {
            TopNode::Raw(item) if item.tag == 1002 => item.u16_value(),
            _ => None,
        })
    }

    pub fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.nodes.iter().filter_map(|n| n.as_track())
    }

    pub fn tracks_mut(&mut self) -> impl Iterator<Item = &mut Track> {
        self.nodes.iter_mut().filter_map(|n| n.as_track_mut())
    }

    pub fn track_count(&self) -> usize {
        self.tracks().count()
    }

    /// The clip's length from the tracks' `2009` chunks. In a clipboard copy
    /// every track holds the copied range's length; in a `.dms` project each
    /// track holds its own end-of-track tick, so the longest one is used.
    pub fn stretch_len(&self) -> Option<u32> {
        self.tracks().filter_map(|t| t.length()).max()
    }

    /// The clip's time span: the 2009 length, extended to cover every note
    /// end and event position.
    pub fn span(&self) -> u32 {
        self.nodes.par_iter().filter_map(|n| n.as_track()).map(Track::span).max().unwrap_or(1)
    }

    pub fn note_count(&self) -> usize {
        self.nodes.par_iter().filter_map(|n| n.as_track()).map(Track::note_count).sum()
    }

    /// Flat read-only note view.
    pub fn note_views(&self) -> Vec<NoteView> {
        let mut out = Vec::with_capacity(self.note_count());
        for (track, t) in self.tracks().enumerate() {
            out.extend(t.notes().map(|n| NoteView { track, tick: n.tick, gate: n.gate, key: n.key, vel: n.vel }));
        }
        out
    }

    pub fn push(&mut self, node: TopNode) {
        self.nodes.push(node);
    }

    /// Rough heap footprint, for undo budgeting.
    pub fn approx_bytes(&self) -> usize {
        self.tracks().map(|t| t.children.capacity() * std::mem::size_of::<TrackChild>()).sum::<usize>()
            + self.nodes.len() * 64
    }
}

/// Flat read-only view of a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteView {
    pub track: usize,
    pub tick: u32,
    pub gate: u32,
    pub key: u8,
    pub vel: u8,
}

// ---- decoding -----------------------------------------------------------------

/// Decode the unpacked chunk stream into a `Clip` (tolerant and lossless).
pub fn decode_clip(unpacked: &[u8]) -> Clip {
    enum Pending<'a> {
        Raw(Item),
        Track(&'a [u8]),
    }
    let mut pending = Vec::new();
    let mut i = 0usize;
    while let Some((tag, start, end)) = read_header(unpacked, i) {
        let body = &unpacked[start..end];
        pending.push(if tag == 1003 { Pending::Track(body) } else { Pending::Raw(Item { tag, body: body.to_vec() }) });
        i = end;
    }
    let trailing = unpacked[i..].to_vec();
    let nodes: Vec<TopNode> = pending
        .into_par_iter()
        .map(|p| match p {
            Pending::Raw(item) => TopNode::Raw(item),
            Pending::Track(body) => TopNode::Track(decode_track(body)),
        })
        .collect();

    let mut clip = Clip { nodes, trailing, warnings: Vec::new() };
    if !clip.trailing.is_empty() {
        clip.warnings.push(format!(
            "{} trailing byte(s) at the end of the chunk stream were preserved",
            clip.trailing.len()
        ));
    }
    let bad: Vec<String> = clip
        .tracks()
        .enumerate()
        .filter(|(_, t)| !t.trailing.is_empty())
        .map(|(k, t)| format!("track {}: {} trailing byte(s) inside the track body were preserved", k + 1, t.trailing.len()))
        .collect();
    clip.warnings.extend(bad);
    clip
}

/// Header of a canonical note chunk plus its first field header — a cheap
/// way to find chunk boundaries in the middle of a track.
const SYNC: [u8; 12] = [0xD1, 0x07, 0x22, 0, 0, 0, 0xE9, 0x03, 4, 0, 0, 0];

/// Split a track body into pieces that start at canonical notes, so they
/// can be walked independently. Callers must check that each piece's walk
/// ends exactly at the piece's end.
fn split_points(body: &[u8]) -> Vec<usize> {
    let pieces = (body.len() / PAR_BYTES).clamp(1, rayon::current_num_threads() * 4);
    let mut points = vec![0];
    for k in 1..pieces {
        let from = (body.len() * k / pieces).max(*points.last().unwrap() + 1);
        if let Some(off) = body[from..].windows(SYNC.len()).position(|w| w == SYNC) {
            let at = from + off;
            if at < body.len() {
                points.push(at);
            }
        }
    }
    points.dedup();
    points.push(body.len());
    points
}

/// Walk a piece of a track body with `f`. Returns the result and how many
/// bytes were consumed as whole chunks.
fn walk<T>(piece: &[u8], mut init: T, mut f: impl FnMut(&mut T, u16, &[u8])) -> (T, usize) {
    let mut i = 0usize;
    while let Some((tag, start, end)) = read_header(piece, i) {
        f(&mut init, tag, &piece[start..end]);
        i = end;
    }
    (init, i)
}

/// Apply `f` to the chunks of a track body, in parallel pieces when the body
/// is big. Returns per-piece results and the offset where whole chunks end.
fn walk_track<T: Send>(body: &[u8], init: impl Fn(usize) -> T + Sync, f: impl Fn(&mut T, u16, &[u8]) + Sync) -> (Vec<T>, usize) {
    if body.len() >= 2 * PAR_BYTES {
        let points = split_points(body);
        let last = points.len() - 2;
        let pieces: Vec<(T, usize)> = (0..=last)
            .into_par_iter()
            .map(|k| {
                let piece = &body[points[k]..points[k + 1]];
                walk(piece, init(piece.len()), &f)
            })
            .collect();
        let exact = pieces.iter().enumerate().all(|(k, (_, used))| k == last || points[k] + used == points[k + 1]);
        if exact {
            let end = points[last] + pieces[last].1;
            return (pieces.into_iter().map(|(t, _)| t).collect(), end);
        }
    }
    let (t, end) = walk(body, init(body.len()), &f);
    (vec![t], end)
}

fn decode_child(children: &mut Vec<TrackChild>, tag: u16, inner: &[u8]) {
    children.push(match tag {
        2001 => match canonical_note(inner) {
            Some(n) => TrackChild::Note(n),
            None => parse_note(inner),
        },
        2009 => parse_length(inner),
        t if t >= 2002 => TrackChild::event(EventItem { id: t, pos: find_pos(inner), item: Item { tag: t, body: inner.to_vec() } }),
        _ => TrackChild::raw(Item { tag, body: inner.to_vec() }),
    });
}

fn decode_track(body: &[u8]) -> Track {
    let (pieces, end) = walk_track(body, |len| Vec::with_capacity(len / NOTE_LEN + 32), decode_child);
    let children = if pieces.len() == 1 {
        let mut v = pieces.into_iter().next().unwrap();
        v.shrink_to_fit();
        v
    } else {
        let mut v = Vec::with_capacity(pieces.iter().map(Vec::len).sum());
        for p in pieces {
            v.extend(p);
        }
        v
    };
    Track { children, trailing: body[end..].to_vec() }
}

/// A tag-2001 item is a note only if it has `1001` (4 bytes), `2001` (1 byte)
/// and `2003` (4 bytes) at their documented sizes; otherwise it is kept as a
/// verbatim event.
fn parse_note(body: &[u8]) -> TrackChild {
    let parts = chunk::read_all(body);
    let mut idx = [None::<usize>; 4];
    for (i, c) in parts.items.iter().enumerate() {
        let slot = match (c.tag, c.body.len()) {
            (1001, 4) => 0,
            (2001, 1) => 1,
            (2002, 1) => 2,
            (2003, 4) => 3,
            _ => continue,
        };
        idx[slot].get_or_insert(i);
    }
    match (idx[0], idx[1], idx[3]) {
        (Some(idx_tick), Some(idx_key), Some(idx_gate)) if parts.is_clean() => {
            let items = &parts.items;
            let note = NoteItem {
                tick: items[idx_tick].u32_value().unwrap(),
                key: items[idx_key].u8_value().unwrap(),
                vel: idx[2].map_or(100, |i| items[i].u8_value().unwrap()),
                gate: items[idx_gate].u32_value().unwrap(),
            };
            TrackChild::OddNote(Box::new(OddNote { note, children: parts.items, idx_tick, idx_key, idx_vel: idx[2], idx_gate }))
        }
        _ => TrackChild::event(EventItem { id: 2001, pos: find_pos(body), item: Item { tag: 2001, body: body.to_vec() } }),
    }
}

fn parse_length(body: &[u8]) -> TrackChild {
    let ParseResult { items, trailing } = chunk::read_all(body);
    match items.iter().position(|c| c.tag == 1001 && c.body.len() == 4) {
        Some(idx) if trailing.is_empty() => TrackChild::Length(Box::new(LengthItem {
            length: items[idx].u32_value().unwrap(),
            children: items,
            idx_length: idx,
        })),
        _ => TrackChild::raw(Item { tag: 2009, body: body.to_vec() }),
    }
}

// ---- encoding -----------------------------------------------------------------

/// Cursor over a pre-sized output region.
struct Out<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Out<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Out { buf, pos: 0 }
    }

    #[inline]
    fn put(&mut self, b: &[u8]) {
        self.buf[self.pos..self.pos + b.len()].copy_from_slice(b);
        self.pos += b.len();
    }

    #[inline]
    fn item(&mut self, tag: u16, body: &[u8]) {
        self.put(&tag.to_le_bytes());
        self.put(&(body.len() as u32).to_le_bytes());
        self.put(body);
    }
}

/// Adjustments applied while encoding, so callers never clone a clip just
/// to change how it is written.
#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    /// Leave out conductor tracks (`1003 = 1`).
    pub skip_conductor: bool,
    /// Write this value into every track's `2009`.
    pub uniform_length: Option<u32>,
    /// Insert this track before the first track.
    pub insert_before_first_track: Option<Track>,
}

/// The file format stores every chunk size, and the whole unpacked size, in
/// 32 bits: the only real limit on how much a clip can hold (about 107
/// million notes per track, 4 GiB in all). Checks that an encoding fits.
pub fn check_format_limits(clip: &Clip, opts: &EncodeOptions) -> Result<(), String> {
    let limit = u32::MAX as usize;
    let mut total = clip.trailing.len();
    let tracks = clip.tracks().filter(|t| !(opts.skip_conductor && t.is_conductor())).chain(opts.insert_before_first_track.iter());
    for (k, t) in tracks.enumerate() {
        let body = t.body_len();
        if body > limit {
            return Err(format!(
                "track {} is {} MB, but the Domino format allows at most 4096 MB (about 107 million notes) per track",
                k + 1,
                body >> 20
            ));
        }
        total += 6 + body;
    }
    total += clip.nodes.iter().map(|n| match n {
        TopNode::Raw(i) => 6 + i.body.len(),
        _ => 0,
    }).sum::<usize>();
    if total > limit {
        return Err(format!("the clip is {} MB, but the Domino format allows at most 4096 MB in all", total >> 20));
    }
    Ok(())
}

/// Encode a `Clip` back into the unpacked chunk stream.
pub fn encode_clip(clip: &Clip) -> Vec<u8> {
    encode_clip_with(clip, &EncodeOptions::default())
}

pub fn encode_clip_with(clip: &Clip, opts: &EncodeOptions) -> Vec<u8> {
    // what to write, in order
    let mut parts: Vec<&TopNode> = Vec::with_capacity(clip.nodes.len() + 1);
    let inserted = opts.insert_before_first_track.clone().map(TopNode::Track);
    let mut pending_insert = inserted.as_ref();
    for node in &clip.nodes {
        if let TopNode::Track(t) = node {
            if let Some(ins) = pending_insert.take() {
                parts.push(ins);
            }
            if opts.skip_conductor && t.is_conductor() {
                continue;
            }
        }
        parts.push(node);
    }
    if let Some(ins) = pending_insert {
        parts.push(ins);
    }

    let lens: Vec<usize> = parts
        .par_iter()
        .map(|n| match n {
            TopNode::Raw(i) => 6 + i.body.len(),
            TopNode::Track(t) => 6 + t.body_len(),
        })
        .collect();
    let total = lens.iter().sum::<usize>() + clip.trailing.len();
    let mut out = vec![0u8; total];

    // carve the buffer into one region per node and fill them in parallel
    let mut regions = Vec::with_capacity(parts.len());
    let mut rest: &mut [u8] = &mut out;
    for &len in &lens {
        let (head, tail) = rest.split_at_mut(len);
        regions.push(head);
        rest = tail;
    }
    rest.copy_from_slice(&clip.trailing);
    regions.into_par_iter().zip(parts.par_iter()).for_each(|(region, node)| match node {
        TopNode::Raw(i) => Out::new(region).item(i.tag, &i.body),
        TopNode::Track(t) => {
            let (head, body) = region.split_at_mut(6);
            head[..2].copy_from_slice(&1003u16.to_le_bytes());
            head[2..].copy_from_slice(&(body.len() as u32).to_le_bytes());
            t.write_body(body, opts.uniform_length);
        }
    });
    out
}

/// Count the notes in an unpacked chunk stream without building a clip (used
/// to verify an encoding cheaply).
pub fn count_notes(unpacked: &[u8]) -> usize {
    let mut tracks = Vec::new();
    let mut i = 0usize;
    while let Some((tag, start, end)) = read_header(unpacked, i) {
        if tag == 1003 {
            tracks.push(&unpacked[start..end]);
        }
        i = end;
    }
    tracks
        .par_iter()
        .map(|body| {
            let count = |n: &mut usize, tag: u16, inner: &[u8]| {
                if tag == 2001 && (canonical_note(inner).is_some() || matches!(parse_note(inner), TrackChild::OddNote(_))) {
                    *n += 1;
                }
            };
            walk_track(body, |_| 0usize, count).0.into_iter().sum::<usize>()
        })
        .sum()
}

/// Pre-write self-check: the encoded bytes must hold exactly the model's notes.
pub fn verify_encoding(clip_notes: usize, unpacked: &[u8]) -> Result<(), String> {
    let actual = count_notes(unpacked);
    if actual != clip_notes {
        return Err(format!(
            "output self-check failed: the model has {clip_notes} notes, the encoded bytes {actual}"
        ));
    }
    Ok(())
}

/// Encode → count notes → compare.
pub fn self_check(clip: &Clip) -> Result<(), String> {
    verify_encoding(clip.note_count(), &encode_clip(clip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::chunk::item;

    #[test]
    fn values_stay_small() {
        assert_eq!(std::mem::size_of::<NoteItem>(), 12);
        assert_eq!(std::mem::size_of::<TrackChild>(), 16);
    }

    #[test]
    fn ppq_parses_from_root_chunk() {
        let clip = decode_clip(&item(1002, &[0xE0, 0x01]));
        assert_eq!(clip.ppq(), Some(480));
    }

    #[test]
    fn canonical_note_encodes_like_the_template() {
        let n = NoteItem::canonical(0, 240, 36, 100);
        assert_eq!(n.encode(), crate::format::template::note_bytes(0, 36, 100, 240));
        assert!(canonical_note(&n.encode()[6..]).is_some());
    }

    #[test]
    fn note_round_trip_is_byte_exact() {
        // a note with an unknown extra inner chunk in a weird place
        let mut body = item(1001, &7u32.to_le_bytes());
        body.extend_from_slice(&item(999, &[1, 2, 3]));
        body.extend_from_slice(&item(2001, &[64]));
        body.extend_from_slice(&item(2002, &[100]));
        body.extend_from_slice(&item(2003, &240u32.to_le_bytes()));
        let note = chunk::Item { tag: 2001, body }.encode();
        let track = chunk::Item { tag: 1003, body: note }.encode();
        let mut clip = decode_clip(&track);
        let t = clip.tracks().next().unwrap();
        assert!(matches!(t.children[0], TrackChild::OddNote(_)));
        let n = t.notes().next().unwrap();
        assert_eq!((n.tick, n.key, n.vel, n.gate), (7, 64, 100, 240));
        assert_eq!(encode_clip(&clip), track);
        // patched fields land in the right inner chunks
        clip.tracks_mut().next().unwrap().notes_mut().next().unwrap().set_key(65);
        let again = decode_clip(&encode_clip(&clip));
        assert_eq!(again.tracks().next().unwrap().notes().next().unwrap().key, 65);
        assert_eq!(count_notes(&encode_clip(&clip)), 1);
    }

    #[test]
    fn note_without_velocity_stays_without() {
        let mut body = item(1001, &7u32.to_le_bytes());
        body.extend_from_slice(&item(2001, &[64]));
        body.extend_from_slice(&item(2003, &240u32.to_le_bytes()));
        let track = chunk::Item { tag: 1003, body: chunk::Item { tag: 2001, body }.encode() }.encode();
        let clip = decode_clip(&track);
        assert_eq!(clip.note_count(), 1);
        assert_eq!(encode_clip(&clip), track);
    }

    #[test]
    fn noncanonical_note_is_kept_as_event() {
        let body = item(1001, &7u32.to_le_bytes());
        let noteish = chunk::Item { tag: 2001, body }.encode();
        let track = chunk::Item { tag: 1003, body: noteish }.encode();
        let clip = decode_clip(&track);
        assert_eq!(clip.note_count(), 0);
        assert_eq!(clip.tracks().next().unwrap().children.len(), 1);
        assert_eq!(encode_clip(&clip), track);
    }

    #[test]
    fn length_item_extra_chunks_survive() {
        let mut body = item(1001, &960u32.to_le_bytes());
        body.extend_from_slice(&item(1002, &[7]));
        let length = chunk::Item { tag: 2009, body }.encode();
        let track = chunk::Item { tag: 1003, body: length }.encode();
        let clip = decode_clip(&track);
        assert_eq!(clip.stretch_len(), Some(960));
        assert_eq!(encode_clip(&clip), track);
    }

    #[test]
    fn trailing_bytes_survive_with_warning() {
        let mut unpacked = item(1002, &[0xE0, 0x01]);
        unpacked.extend_from_slice(&[0xAA, 0xBB]);
        let clip = decode_clip(&unpacked);
        assert_eq!(clip.trailing, vec![0xAA, 0xBB]);
        assert_eq!(clip.warnings.len(), 1);
        assert_eq!(encode_clip(&clip), unpacked);
    }

    #[test]
    fn event_pos_is_patched_without_touching_other_bytes() {
        let mut ev_body = item(1001, &300u32.to_le_bytes());
        ev_body.extend_from_slice(&item(2001, &[10u8]));
        let unpacked = chunk::Item { tag: 1003, body: item(2003, &ev_body) }.encode();
        let mut clip = decode_clip(&unpacked);
        if let TrackChild::Event(e) = &mut clip.tracks_mut().next().unwrap().children[0] {
            e.pos = Some(60);
        }
        let mut patched = item(1001, &60u32.to_le_bytes());
        patched.extend_from_slice(&item(2001, &[10u8]));
        let expected = chunk::Item { tag: 1003, body: item(2003, &patched) }.encode();
        assert_eq!(encode_clip(&clip), expected);
    }

    /// A big single track (decoded / encoded in parallel pieces) with odd
    /// chunks and fake sync patterns scattered through it.
    #[test]
    fn big_tracks_round_trip_in_parallel_pieces() {
        let mut body = Vec::new();
        for i in 0..400_000u32 {
            body.extend_from_slice(&crate::format::template::note_bytes(i, (i % 128) as u8, 100, 10));
            if i % 50_000 == 7 {
                // a raw chunk whose body contains the sync pattern
                let mut fake = SYNC.to_vec();
                fake.extend_from_slice(&[0u8; 30]);
                body.extend_from_slice(&item(1500, &fake));
                body.extend_from_slice(&item(2003, &item(1001, &i.to_le_bytes())));
            }
        }
        let unpacked = chunk::Item { tag: 1003, body }.encode();
        let clip = decode_clip(&unpacked);
        assert_eq!(clip.note_count(), 400_000);
        assert_eq!(encode_clip(&clip), unpacked);
        assert_eq!(count_notes(&unpacked), 400_000);
    }

    #[test]
    fn encode_options() {
        let golden = crate::format::template::tests::golden_unpacked();
        let clip = decode_clip(&golden);
        let mut conductor = clip.tracks().next().unwrap().clone();
        conductor.children.retain(|c| !c.is_note());
        conductor.set_setting(1003, vec![1]);
        let with = encode_clip_with(&clip, &EncodeOptions { insert_before_first_track: Some(conductor), ..Default::default() });
        let decoded = decode_clip(&with);
        assert_eq!(decoded.track_count(), 2);
        assert!(decoded.tracks().next().unwrap().is_conductor());
        let back = encode_clip_with(&decoded, &EncodeOptions { skip_conductor: true, ..Default::default() });
        assert_eq!(back, golden);
        let longer = encode_clip_with(&clip, &EncodeOptions { uniform_length: Some(4000), ..Default::default() });
        assert_eq!(decode_clip(&longer).stretch_len(), Some(4000));
    }
}
