//! Statistics of the working copy, computed once per change (in parallel)
//! so the UI never walks the notes while drawing a frame.

use rayon::prelude::*;

use dominokit_core::format::clip::{Clip, Track, TrackChild};

use super::state::{count, plural};

#[derive(Debug, Clone, Default)]
pub struct TrackStats {
    pub name: String,
    pub conductor: bool,
    pub channel: Option<u8>,
    pub notes: usize,
    pub events: usize,
    /// Lowest / highest key byte, if the track has notes.
    pub keys: Option<(u8, u8)>,
}

#[derive(Debug, Clone, Default)]
pub struct ClipStats {
    pub tracks: Vec<TrackStats>,
    pub notes: usize,
    pub span: u32,
    pub ppq: Option<u16>,
    pub keys: Option<(u8, u8)>,
}

impl ClipStats {
    pub fn compute(clip: &Clip) -> ClipStats {
        let tracks: Vec<TrackStats> = clip.tracks().collect::<Vec<_>>().par_iter().map(|t| track_stats(t)).collect();
        let keys = tracks.iter().filter_map(|t| t.keys).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
        ClipStats {
            notes: tracks.iter().map(|t| t.notes).sum(),
            span: clip.span(),
            ppq: clip.ppq(),
            keys,
            tracks,
        }
    }

    /// "3 tracks · 1,280 notes"
    pub fn summary(&self) -> String {
        format!(
            "{} track{} · {} note{}",
            count(self.tracks.len()),
            plural(self.tracks.len()),
            count(self.notes),
            plural(self.notes)
        )
    }
}

fn track_stats(t: &Track) -> TrackStats {
    #[derive(Clone, Copy)]
    struct Acc {
        notes: usize,
        events: usize,
        lo: u8,
        hi: u8,
    }
    let id = || Acc { notes: 0, events: 0, lo: u8::MAX, hi: 0 };
    let add = |mut a: Acc, c: &TrackChild| {
        if let Some(n) = c.as_note() {
            a.notes += 1;
            a.lo = a.lo.min(n.key);
            a.hi = a.hi.max(n.key);
        } else if c.as_event().is_some() {
            a.events += 1;
        }
        a
    };
    let merge = |a: Acc, b: Acc| Acc { notes: a.notes + b.notes, events: a.events + b.events, lo: a.lo.min(b.lo), hi: a.hi.max(b.hi) };
    let acc = t.children.par_iter().fold(id, add).reduce(id, merge);
    TrackStats {
        name: t.display_name().unwrap_or_default(),
        conductor: t.is_conductor(),
        channel: t.channel(),
        notes: acc.notes,
        events: acc.events,
        keys: (acc.notes > 0).then_some((acc.lo, acc.hi)),
    }
}
