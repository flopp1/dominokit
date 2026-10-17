//! Typed views of Domino event containers, and builders for the ones the
//! MIDI converter creates. Layouts are the ones Domino itself writes
//! (`probe/FINDINGS.md` §3); everything is decoded by actual chunk size.

use crate::format::chunk::{item, Item};
use crate::format::clip::EventItem;

/// Text-like events: one `2001` raw-bytes field (no transcoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// 2005 comment (SMF FF 01 text).
    Text,
    /// 2011 (FF 05).
    Lyric,
    /// 2017 (FF 06).
    Marker,
    /// 2012 (FF 07).
    Cue,
}

impl TextKind {
    pub const fn event_id(self) -> u16 {
        match self {
            TextKind::Text => 2005,
            TextKind::Lyric => 2011,
            TextKind::Marker => 2017,
            TextKind::Cue => 2012,
        }
    }

    pub const fn meta_type(self) -> u8 {
        match self {
            TextKind::Text => 0x01,
            TextKind::Lyric => 0x05,
            TextKind::Marker => 0x06,
            TextKind::Cue => 0x07,
        }
    }

    pub fn from_event_id(id: u16) -> Option<TextKind> {
        Some(match id {
            2005 => TextKind::Text,
            2011 => TextKind::Lyric,
            2017 => TextKind::Marker,
            2012 => TextKind::Cue,
            _ => return None,
        })
    }

    pub fn from_meta_type(t: u8) -> Option<TextKind> {
        Some(match t {
            0x01 => TextKind::Text,
            0x05 => TextKind::Lyric,
            0x06 => TextKind::Marker,
            0x07 => TextKind::Cue,
            _ => return None,
        })
    }
}

/// Decoded meaning of a non-note event.
#[derive(Debug, Clone, PartialEq)]
pub enum EventValue {
    /// 2002: `patch` is the MIDI program + 1 (1..=128).
    Program { patch: u8 },
    /// 2003: an instance of a Control Change Macro. `value` is stored minus
    /// the module's `Value/@Offset` (e.g. pan 64 → 0, pitch bend 8192 → 0).
    Control { ccm: u16, gate: f32, value: f32 },
    /// 2008.
    Tempo { bpm: f32 },
    /// 2015: denominator stored literally (4, 8, 16, …).
    TimeSig { num: u8, den: u8 },
    /// 2016: sharps `n` → n, flats `n` → 8 + n, minor + 16.
    KeySig { key: u8 },
    Text { kind: TextKind, bytes: Vec<u8> },
    /// Anything else (sysex, measure link, chord, …): kept verbatim.
    Other,
}

/// The body of the first field `id` in a chunk sequence (no allocation).
pub fn field(body: &[u8], id: u16) -> Option<&[u8]> {
    let mut i = 0usize;
    while i + 6 <= body.len() {
        let tag = u16::from_le_bytes([body[i], body[i + 1]]);
        let size = u32::from_le_bytes(body[i + 2..i + 6].try_into().unwrap()) as usize;
        let end = (i + 6).checked_add(size).filter(|&e| e <= body.len())?;
        if tag == id {
            return Some(&body[i + 6..end]);
        }
        i = end;
    }
    None
}

fn u8_of(b: &[u8]) -> Option<u8> {
    (b.len() == 1).then(|| b[0])
}

/// First value of an f32 list (`id 0, size 4, f32` chunks).
fn f32_first(list: &[u8]) -> Option<f32> {
    field(list, 0).filter(|v| v.len() == 4).map(|v| f32::from_le_bytes(v.try_into().unwrap()))
}

/// Decode an event container's body.
pub fn decode(event: &EventItem) -> EventValue {
    let body = &event.item.body;
    let get = |id: u16| field(body, id);
    match event.id {
        2002 => match get(2003).and_then(u8_of) {
            Some(patch) => EventValue::Program { patch },
            None => EventValue::Other,
        },
        2003 => match (get(2001).filter(|b| b.len() == 2), get(2003)) {
            (Some(ccm), Some(v)) => EventValue::Control {
                ccm: u16::from_le_bytes([ccm[0], ccm[1]]),
                gate: get(2002).and_then(f32_first).unwrap_or(0.0),
                value: f32_first(v).unwrap_or(0.0),
            },
            _ => EventValue::Other,
        },
        2008 => match get(2001).and_then(f32_first) {
            Some(bpm) => EventValue::Tempo { bpm },
            None => EventValue::Other,
        },
        2015 => match (get(2001).and_then(u8_of), get(2002).and_then(u8_of)) {
            (Some(num), Some(den)) => EventValue::TimeSig { num, den },
            _ => EventValue::Other,
        },
        2016 => match get(2001).and_then(u8_of) {
            Some(key) => EventValue::KeySig { key },
            None => EventValue::Other,
        },
        id => match (TextKind::from_event_id(id), get(2001)) {
            (Some(kind), Some(t)) => EventValue::Text { kind, bytes: t.to_vec() },
            _ => EventValue::Other,
        },
    }
}

/// First value of an f32 list item.
pub fn f32_list_first(item: &Item) -> Option<f32> {
    f32_first(&item.body)
}

fn f32_list(v: f32) -> Vec<u8> {
    item(0, &v.to_le_bytes())
}

fn event(id: u16, pos: u32, fields: &[Vec<u8>]) -> EventItem {
    let mut body = item(1001, &pos.to_le_bytes());
    for field in fields {
        body.extend_from_slice(field);
    }
    EventItem { id, pos: Some(pos), item: Item { tag: id, body } }
}

/// 2002 exactly as Domino writes it: patch, empty 2004, 2005 = 0, then the
/// (bank?) fields at −1.
pub fn program(pos: u32, patch: u8) -> EventItem {
    event(
        2002,
        pos,
        &[
            item(2003, &[patch]),
            item(2004, &[]),
            item(2005, &[0]),
            item(2001, &[0xFF]),
            item(2002, &[0xFF]),
            item(2006, &[0xFF, 0xFF]),
        ],
    )
}

pub fn control(pos: u32, ccm: u16, value: f32) -> EventItem {
    event(
        2003,
        pos,
        &[
            item(2001, &ccm.to_le_bytes()),
            item(2002, &f32_list(0.0)),
            item(2003, &f32_list(value)),
        ],
    )
}

pub fn tempo(pos: u32, bpm: f32) -> EventItem {
    event(2008, pos, &[item(2001, &f32_list(bpm))])
}

pub fn time_sig(pos: u32, num: u8, den: u8) -> EventItem {
    event(2015, pos, &[item(2001, &[num]), item(2002, &[den])])
}

pub fn key_sig(pos: u32, key: u8) -> EventItem {
    event(2016, pos, &[item(2001, &[key])])
}

pub fn text(pos: u32, kind: TextKind, bytes: &[u8]) -> EventItem {
    event(kind.event_id(), pos, &[item(2001, bytes)])
}

/// SMF key signature (sharps/flats, minor) → Domino's 2016 key byte.
pub fn key_sig_from_smf(sf: i8, minor: bool) -> u8 {
    let base = if sf >= 0 { sf as u8 } else { 8 + sf.unsigned_abs() };
    base + if minor { 16 } else { 0 }
}

/// Domino's 2016 key byte → SMF (sharps/flats, minor).
pub fn key_sig_to_smf(key: u8) -> (i8, bool) {
    let minor = key >= 16;
    let k = key % 16;
    let sf = if k <= 7 { k as i8 } else { -((k - 8) as i8) };
    (sf, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_match_domino_sizes() {
        // container body sizes seen in probe.dms
        assert_eq!(program(0, 6).item.body.len(), 52);
        assert_eq!(control(0, 1, 48.0).item.body.len(), 50);
        assert_eq!(tempo(0, 120.0).item.body.len(), 26);
        assert_eq!(time_sig(0, 4, 4).item.body.len(), 24);
        assert_eq!(key_sig(0, 0).item.body.len(), 17);
    }

    #[test]
    fn decodes_what_it_builds() {
        assert_eq!(decode(&program(0, 87)), EventValue::Program { patch: 87 });
        assert_eq!(
            decode(&control(5, 130, -8192.0)),
            EventValue::Control { ccm: 130, gate: 0.0, value: -8192.0 }
        );
        assert_eq!(decode(&tempo(0, 123.456)), EventValue::Tempo { bpm: 123.456 });
        assert_eq!(decode(&time_sig(0, 7, 16)), EventValue::TimeSig { num: 7, den: 16 });
        assert_eq!(
            decode(&text(0, TextKind::Marker, b"m")),
            EventValue::Text { kind: TextKind::Marker, bytes: b"m".to_vec() }
        );
    }

    #[test]
    fn key_signature_table() {
        // FINDINGS §3 table
        for (sf, minor, stored) in [
            (0, false, 0),
            (1, false, 1),
            (6, false, 6),
            (-1, false, 9),
            (-7, false, 15),
            (0, true, 16),
            (7, true, 23),
            (-6, true, 30),
        ] {
            assert_eq!(key_sig_from_smf(sf, minor), stored);
            assert_eq!(key_sig_to_smf(stored), (sf, minor));
        }
    }
}
