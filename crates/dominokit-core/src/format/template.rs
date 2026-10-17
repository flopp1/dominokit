//! Fallback templates from the reference implementation (Appendix A).
//!
//! These are exactly the chunks Domino itself writes for a track's settings
//! and the song's root (compare `probe/probe.dms`), so the MIDI importer and
//! the conductor track added for `.dms` export build on them.

use crate::format::chunk::item;
use crate::format::clip::{Clip, NoteItem, TopNode, Track, TrackChild};
use hex_literal::hex;

pub const MAGIC: &[u8; 18] = b"PortalSequenceData";

/// 1000 (empty name), 1001 (empty copyright).
pub const SONG_START: &[u8] = &hex!("e80300000000e90300000000");

/// Follows item 1002 (PPQ).
pub const SONG_REST: &[u8] = &hex!(
    "ef030400000030000000f1030400000000000000f40308000000000000000000"
    "0000f5030400000000000000f6030400000000000000fb0300000000fc030400"
    "000010000000fd030100000001fe030100000001ff0304000000110000000004"
    "0100000001"
);

pub const TRACK_HEAD: &[u8] = &hex!(
    "e803020000000000e9030100000000ea0300000000eb030100000000ec030100"
    "000000f003010000003cf1031100000047656e6572616c204d49444920447275"
    "6df303020000000000f4030400000000000000f8030400000064000000f90304"
    "00000000000000f5030100000001f603020000000100f7030100000001fa0301"
    "000000fffb030400000001000000fc0310000000000000000000000000000000"
    "00000000fd030100000000fe03010000007f"
);

pub const TRACK_TAIL: &[u8] = &hex!(
    "ed030400000032000000ee030100000064ef0304000000e0010000f2030e0000"
    "00e8030100000000e9030100000000"
);

pub const SONG_TAIL: &[u8] = &hex!(
    "ee0300000000f0031a000000e80300000000e9030400000001000000ea030400"
    "000001000000f903420000006400010000000065000100000000660001000000"
    "006700040000006400000068000100000000690004000000640000006a000c00"
    "0000050505050505050505050505"
);

/// Build a note item's 40 canonical bytes (§2.5 / Appendix A).
pub fn note_bytes(tick: u32, key: u8, vel: u8, gate: u32) -> Vec<u8> {
    item(2001, &{
        let mut b = item(1001, &tick.to_le_bytes());
        b.extend_from_slice(&item(2001, &[key]));
        b.extend_from_slice(&item(2002, &[vel]));
        b.extend_from_slice(&item(2003, &gate.to_le_bytes()));
        b
    })
}

/// Synthesise a fallback track (Appendix A TRACK_HEAD/TAIL) with the given
/// notes and stretch length.
pub fn fallback_track(notes: &[NoteItem], length: u32) -> Track {
    let head = crate::format::chunk::read_all(TRACK_HEAD).items;
    let tail = crate::format::chunk::read_all(TRACK_TAIL).items;
    let mut track = Track::default();
    track.children.reserve_exact(head.len() + notes.len() + 1 + tail.len());
    track.children.extend(head.into_iter().map(TrackChild::raw));
    track.children.extend(notes.iter().cloned().map(TrackChild::Note));
    track.children.push(TrackChild::length(length));
    track.children.extend(tail.into_iter().map(TrackChild::raw));
    track
}

/// Assemble a complete fallback clip (Appendix A) from tracks.
///
/// `tracks` are (notes, stretch length) pairs. Tracks with no notes are not
/// written by the reference; callers decide.
pub fn fallback_clip(ppq: u16, tracks: &[(Vec<NoteItem>, u32)]) -> Clip {
    let mut root: Vec<u8> = SONG_START.to_vec();
    root.extend_from_slice(&item(1002, &ppq.to_le_bytes()));
    root.extend_from_slice(SONG_REST);
    let mut clip = crate::format::clip::decode_clip(&root);
    for (notes, length) in tracks {
        clip.push(TopNode::Track(fallback_track(notes, *length)));
    }
    clip.nodes.extend(crate::format::clip::decode_clip(SONG_TAIL).nodes);
    clip
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn template_track_parses_with_note_and_length() {
        let notes = vec![NoteItem::canonical(0, 240, 36, 100)];
        let track = fallback_track(&notes, 960);
        assert_eq!(track.notes().count(), 1);
        assert_eq!(track.length(), Some(960));
    }

    #[test]
    fn fallback_clip_has_ppq_and_round_trips() {
        let notes = vec![
            NoteItem::canonical(0, 240, 36, 100),
            NoteItem::canonical(240, 240, 38, 90),
        ];
        let clip = fallback_clip(480, &[(notes, 480)]);
        assert_eq!(clip.ppq(), Some(480));
        assert_eq!(clip.note_count(), 2);
        assert_eq!(clip.stretch_len(), Some(480));
        // encode(decode(x)) == x on the synthesised clip
        let re = crate::format::clip::encode_clip(&clip);
        let clip2 = crate::format::clip::decode_clip(&re);
        assert_eq!(clip, clip2);
    }

    #[test]
    fn golden_payload_matches_appendix_a_assembly() {
        // The golden payload of §5.1 was produced by this assembly with
        // note-start stretch 960 and three notes.
        let notes = vec![
            NoteItem::canonical(0, 240, 36, 100),
            NoteItem::canonical(240, 240, 38, 90),
            NoteItem::canonical(480, 480, 127, 64),
        ];
        let clip = fallback_clip(480, &[(notes, 960)]);
        let unpacked = crate::format::clip::encode_clip(&clip);
        assert_eq!(unpacked, golden_unpacked());
    }

    /// §5.1 golden unpacked chunk data (598 bytes).
    pub fn golden_unpacked() -> Vec<u8> {
        hex_to_bytes(
            "e80300000000e90300000000ea0302000000e001ef030400000030000000f1030400000000000000f403080000000000
000000000000f5030400000000000000f6030400000000000000fb0300000000fc030400000010000000fd0301000000
01fe030100000001ff03040000001100000000040100000001eb0369010000e803020000000000e9030100000000ea03
00000000eb030100000000ec030100000000f003010000003cf1031100000047656e6572616c204d494449204472756d
f303020000000000f4030400000000000000f8030400000064000000f9030400000000000000f5030100000001f60302
0000000100f7030100000001fa0301000000fffb030400000001000000fc031000000000000000000000000000000000
000000fd030100000000fe03010000007fd10722000000e9030400000000000000d1070100000024d2070100000064d3
0704000000f0000000d10722000000e90304000000f0000000d1070100000026d207010000005ad30704000000f00000
00d10722000000e90304000000e0010000d107010000007fd2070100000040d30704000000e0010000d9070a000000e9
0304000000c0030000ed030400000032000000ee030100000064ef0304000000e0010000f2030e000000e80301000000
00e9030100000000ee0300000000f0031a000000e80300000000e9030400000001000000ea030400000001000000f903
420000006400010000000065000100000000660001000000006700040000006400000068000100000000690004000000
640000006a000c000000050505050505050505050505",
        )
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
            .collect()
    }
}
