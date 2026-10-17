//! Segment flip: mirror every note whose key lies inside `lo..=hi`, so `lo`
//! and `hi` swap, `lo + 1` and `hi − 1` swap, and so on. Applies to every
//! track of the clip. Notes outside the range, and everything that is not a
//! note, pass through byte-for-byte. The flip is an involution.
//!
//! Keys are the raw key byte (0..=255). The UI numbers keys 1..=256, so UI
//! key `n` is byte `n − 1` (UI 1..128 is the MIDI range 0..127).

use rayon::prelude::*;

use crate::format::clip::{Clip, TopNode};

/// The inclusive key range to mirror within (raw key bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyRange {
    pub lo: u8,
    pub hi: u8,
}

impl KeyRange {
    /// Range from either order of endpoints.
    pub fn new(a: u8, b: u8) -> KeyRange {
        KeyRange { lo: a.min(b), hi: a.max(b) }
    }

    /// From the UI's 1-based key numbers (1..=256).
    pub fn from_ui(a: u16, b: u16) -> KeyRange {
        let byte = |k: u16| (k.clamp(1, 256) - 1) as u8;
        KeyRange::new(byte(a), byte(b))
    }

    pub fn contains(self, key: u8) -> bool {
        (self.lo..=self.hi).contains(&key)
    }

    pub fn mirror(self, key: u8) -> u8 {
        (self.lo as u16 + self.hi as u16 - key as u16) as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlipReport {
    /// Notes inside the range (mirrored; a note on the centre key keeps its key).
    pub notes_flipped: usize,
    /// Notes outside the range (untouched).
    pub notes_outside: usize,
}

/// Flip the clip in place, all tracks in parallel. Root items and track
/// settings are untouched. Since the flip is an involution, flipping again
/// with the same range undoes it exactly.
pub fn flip_in_place(clip: &mut Clip, range: KeyRange) -> FlipReport {
    let (flipped, total) = clip
        .nodes
        .par_iter_mut()
        .filter_map(|n| match n {
            TopNode::Track(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| t.children.par_chunks_mut(1 << 15))
        .map(|chunk| {
            let (mut flipped, mut total) = (0usize, 0usize);
            for child in chunk {
                if let Some(note) = child.as_note_mut() {
                    total += 1;
                    if range.contains(note.key) {
                        note.key = range.mirror(note.key);
                        flipped += 1;
                    }
                }
            }
            (flipped, total)
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    FlipReport { notes_flipped: flipped, notes_outside: total - flipped }
}

/// Flip a copy of the clip.
pub fn flip_clip(clip: &Clip, range: KeyRange) -> (Clip, FlipReport) {
    let mut out = clip.clone();
    let report = flip_in_place(&mut out, range);
    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::clip::{decode_clip, encode_clip, NoteItem, Track, TrackChild};
    use crate::format::template::tests::golden_unpacked;

    fn clip_with_keys(keys: &[u8]) -> Clip {
        let mut track = Track::default();
        for (i, &k) in keys.iter().enumerate() {
            track.children.push(TrackChild::Note(NoteItem::canonical(i as u32 * 10, 10, k, 100)));
        }
        let mut clip = Clip::default();
        clip.push(TopNode::Track(track));
        clip
    }

    fn keys(clip: &Clip) -> Vec<u8> {
        clip.tracks().flat_map(|t| t.notes().map(|n| n.key)).collect()
    }

    #[test]
    fn full_midi_range_is_127_minus_k_and_touches_only_key_bytes() {
        let clip = decode_clip(&golden_unpacked());
        let (flipped, report) = flip_clip(&clip, KeyRange::from_ui(1, 128));
        assert_eq!(report.notes_flipped, 3);
        assert_eq!(keys(&flipped), vec![91, 89, 0]);
        let (before, after) = (golden_unpacked(), encode_clip(&flipped));
        assert_eq!(before.len(), after.len());
        assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 3);
    }

    #[test]
    fn only_notes_inside_the_segment_move() {
        // UI keys 61..=72 -> bytes 60..=71
        let clip = clip_with_keys(&[59, 60, 61, 65, 71, 72]);
        let (flipped, report) = flip_clip(&clip, KeyRange::from_ui(61, 72));
        assert_eq!(keys(&flipped), vec![59, 71, 70, 66, 60, 72]);
        assert_eq!((report.notes_flipped, report.notes_outside), (4, 2));
    }

    #[test]
    fn supports_the_whole_byte_range_and_either_order() {
        let clip = clip_with_keys(&[0, 127, 128, 255]);
        let (flipped, _) = flip_clip(&clip, KeyRange::from_ui(256, 1));
        assert_eq!(keys(&flipped), vec![255, 128, 127, 0]);
    }

    #[test]
    fn flip_is_an_involution() {
        let clip = decode_clip(&golden_unpacked());
        for (a, b) in [(1, 128), (30, 50), (1, 256), (40, 40)] {
            let r = KeyRange::from_ui(a, b);
            let twice = flip_clip(&flip_clip(&clip, r).0, r).0;
            assert_eq!(encode_clip(&twice), golden_unpacked());
        }
    }
}
