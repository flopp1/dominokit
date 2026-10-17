//! Minimal Standard MIDI File codec: just enough structure for the Domino
//! converter (`format::midi`). Ticks are absolute; running status is read but
//! never written (Domino's own exports don't use it either).

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmfEvent {
    /// Channel voice message: status (0x80..=0xEF) and its 1 or 2 data bytes.
    Channel { status: u8, data: [u8; 2] },
    /// Meta event `FF type len data`.
    Meta { kind: u8, data: Vec<u8> },
    /// `F0`/`F7` sysex; `data` excludes the length but includes everything after it.
    Sysex { status: u8, data: Vec<u8> },
}

impl SmfEvent {
    pub fn channel(status: u8, a: u8, b: u8) -> SmfEvent {
        SmfEvent::Channel { status, data: [a, b] }
    }

    pub fn meta(kind: u8, data: &[u8]) -> SmfEvent {
        SmfEvent::Meta { kind, data: data.to_vec() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmfTrack {
    /// (absolute tick, event), in file order.
    pub events: Vec<(u32, SmfEvent)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Smf {
    pub format: u16,
    /// Ticks per quarter note.
    pub ppq: u16,
    pub tracks: Vec<SmfTrack>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SmfError {
    #[error("not a MIDI file (no MThd header)")]
    NotMidi,
    #[error("SMPTE time division is not supported")]
    Smpte,
    #[error("the MIDI file is truncated or damaged ({0})")]
    Damaged(&'static str),
}

const fn data_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1,
        _ => 2,
    }
}

/// A borrowed event, as the track iterator yields it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev<'a> {
    Channel { status: u8, a: u8, b: u8 },
    Meta { kind: u8, data: &'a [u8] },
    Sysex { status: u8, data: &'a [u8] },
}

impl Ev<'_> {
    pub fn to_owned(self) -> SmfEvent {
        match self {
            Ev::Channel { status, a, b } => SmfEvent::Channel { status, data: [a, b] },
            Ev::Meta { kind, data } => SmfEvent::Meta { kind, data: data.to_vec() },
            Ev::Sysex { status, data } => SmfEvent::Sysex { status, data: data.to_vec() },
        }
    }
}

/// Zero-copy iterator over one `MTrk` body: (absolute tick, event). Stops
/// after End of Track.
pub struct TrackIter<'a> {
    buf: &'a [u8],
    i: usize,
    tick: u32,
    running: Option<u8>,
    done: bool,
}

impl<'a> TrackIter<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        TrackIter { buf, i: 0, tick: 0, running: None, done: false }
    }

    #[inline]
    fn u8(&mut self) -> Result<u8, SmfError> {
        let b = *self.buf.get(self.i).ok_or(SmfError::Damaged("unexpected end"))?;
        self.i += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], SmfError> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or(SmfError::Damaged("unexpected end"))?;
        let s = &self.buf[self.i..end];
        self.i = end;
        Ok(s)
    }

    #[inline]
    fn varlen(&mut self) -> Result<u32, SmfError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(SmfError::Damaged("bad variable-length number"))
    }

    fn read(&mut self) -> Result<Ev<'a>, SmfError> {
        self.tick = self.tick.saturating_add(self.varlen()?);
        let first = self.u8()?;
        Ok(match first {
            0xFF => {
                let kind = self.u8()?;
                let len = self.varlen()? as usize;
                let data = self.bytes(len)?;
                self.done |= kind == 0x2F;
                Ev::Meta { kind, data }
            }
            0xF0 | 0xF7 => {
                self.running = None;
                let len = self.varlen()? as usize;
                Ev::Sysex { status: first, data: self.bytes(len)? }
            }
            0x80..=0xEF => {
                self.running = Some(first);
                let a = self.u8()?;
                let b = if data_len(first) == 2 { self.u8()? } else { 0 };
                Ev::Channel { status: first, a, b }
            }
            0x00..=0x7F => {
                let status = self.running.ok_or(SmfError::Damaged("data byte without status"))?;
                let b = if data_len(status) == 2 { self.u8()? } else { 0 };
                Ev::Channel { status, a: first, b }
            }
            _ => return Err(SmfError::Damaged("unexpected status byte")),
        })
    }
}

impl<'a> Iterator for TrackIter<'a> {
    type Item = Result<(u32, Ev<'a>), SmfError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.i >= self.buf.len() {
            return None;
        }
        match self.read() {
            Ok(ev) => Some(Ok((self.tick, ev))),
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

/// The header fields and the `MTrk` bodies of a file, without parsing events.
pub struct Chunks<'a> {
    pub format: u16,
    pub ppq: u16,
    pub tracks: Vec<&'a [u8]>,
}

pub fn chunks(bytes: &[u8]) -> Result<Chunks<'_>, SmfError> {
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        return Err(SmfError::NotMidi);
    }
    let hlen = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if hlen < 6 || bytes.len() < 8 + hlen {
        return Err(SmfError::Damaged("header"));
    }
    let format = u16::from_be_bytes([bytes[8], bytes[9]]);
    let ntrks = u16::from_be_bytes([bytes[10], bytes[11]]);
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    if division & 0x8000 != 0 {
        return Err(SmfError::Smpte);
    }
    let mut tracks = Vec::with_capacity(ntrks as usize);
    let mut i = 8 + hlen;
    while tracks.len() < ntrks as usize && i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_be_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let start = i + 8;
        let end = start.saturating_add(len).min(bytes.len());
        i = start.saturating_add(len);
        if id == b"MTrk" {
            tracks.push(&bytes[start..end]); // unknown chunk types are skipped, per the SMF spec
        }
    }
    Ok(Chunks { format, ppq: division.max(1), tracks })
}

/// Parse into owned events (convenient for tests and small files).
pub fn parse(bytes: &[u8]) -> Result<Smf, SmfError> {
    let c = chunks(bytes)?;
    let tracks = c
        .tracks
        .iter()
        .map(|t| {
            let events = TrackIter::new(t).map(|r| r.map(|(tick, e)| (tick, e.to_owned()))).collect::<Result<_, _>>()?;
            Ok(SmfTrack { events })
        })
        .collect::<Result<_, SmfError>>()?;
    Ok(Smf { format: c.format, ppq: c.ppq, tracks })
}

pub fn write_varlen(out: &mut Vec<u8>, mut v: u32) {
    let mut stack = [0u8; 5];
    let mut n = 0;
    loop {
        stack[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for k in (0..n).rev() {
        out.push(stack[k] | if k > 0 { 0x80 } else { 0 });
    }
}

/// Serialise as a format-1 (or the given format) SMF. Each track's events
/// must already be sorted by tick; an End of Track is appended if missing.
pub fn write(smf: &Smf) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&smf.format.to_be_bytes());
    out.extend_from_slice(&(smf.tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&smf.ppq.to_be_bytes());
    for track in &smf.tracks {
        let mut body = Vec::new();
        let mut last = 0u32;
        let mut ended = false;
        for (tick, ev) in &track.events {
            if ended {
                break;
            }
            let tick = (*tick).max(last);
            write_varlen(&mut body, tick - last);
            last = tick;
            match ev {
                SmfEvent::Channel { status, data } => {
                    body.push(*status);
                    body.extend_from_slice(&data[..data_len(*status)]);
                }
                SmfEvent::Meta { kind, data } => {
                    body.push(0xFF);
                    body.push(*kind);
                    write_varlen(&mut body, data.len() as u32);
                    body.extend_from_slice(data);
                    ended = *kind == 0x2F;
                }
                SmfEvent::Sysex { status, data } => {
                    body.push(*status);
                    write_varlen(&mut body, data.len() as u32);
                    body.extend_from_slice(data);
                }
            }
        }
        if !ended {
            body.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        }
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varlen_round_trip() {
        for v in [0u32, 0x7F, 0x80, 0x3FFF, 0x4000, 0x0FFF_FFFF] {
            let mut b = Vec::new();
            write_varlen(&mut b, v);
            assert_eq!(TrackIter::new(&b).varlen().unwrap(), v);
        }
    }

    #[test]
    fn write_then_parse() {
        let smf = Smf {
            format: 1,
            ppq: 480,
            tracks: vec![SmfTrack {
                events: vec![
                    (0, SmfEvent::meta(0x03, b"name")),
                    (0, SmfEvent::channel(0x90, 60, 100)),
                    (480, SmfEvent::channel(0x80, 60, 0)),
                    (480, SmfEvent::channel(0xC0, 5, 0)),
                    (500, SmfEvent::meta(0x2F, &[])),
                ],
            }],
        };
        let parsed = parse(&write(&smf)).unwrap();
        assert_eq!(parsed, smf);
    }

    #[test]
    fn running_status_is_read() {
        // MThd, 1 track: 90 3C 64, (running) 3E 64, then EOT
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xE0MTrk".to_vec();
        let body = [0x00, 0x90, 0x3C, 0x64, 0x10, 0x3E, 0x64, 0x00, 0xFF, 0x2F, 0x00];
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&body);
        let smf = parse(&bytes).unwrap();
        assert_eq!(smf.tracks[0].events[1], (16, SmfEvent::channel(0x90, 0x3E, 0x64)));
    }

    #[test]
    fn rejects_non_midi_and_smpte() {
        assert_eq!(parse(b"PortalSequenceData...."), Err(SmfError::NotMidi));
        let bytes = b"MThd\0\0\0\x06\0\x01\0\0\xE7\x28".to_vec();
        assert_eq!(parse(&bytes), Err(SmfError::Smpte));
    }
}
