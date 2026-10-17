//! `cargo run --release --example chopbench -- [notes] [tracks] [step]`
use std::time::Instant;

use dominokit_core::format::clip::NoteItem;
use dominokit_core::format::template::fallback_clip;
use dominokit_core::ops::chop::{chop_clip, ChopStep};

fn main() {
    let a: Vec<u64> = std::env::args().skip(1).filter_map(|x| x.parse().ok()).collect();
    let (n, tracks, step) = (a.first().copied().unwrap_or(10_000_000) as usize, a.get(1).copied().unwrap_or(1) as usize, a.get(2).copied().unwrap_or(120) as u32);
    let mut rng = 0x2545F491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let per = n / tracks;
    let t: Vec<(Vec<NoteItem>, u32)> = (0..tracks)
        .map(|_| {
            let mut tick = 0u32;
            ((0..per).map(|_| {
                tick += next() % 120;
                NoteItem::canonical(tick, 60 + next() % 900, (next() % 128) as u8, 100)
            }).collect(), tick + 1000)
        })
        .collect();
    let clip = fallback_clip(480, &t);
    for _ in 0..3 {
        let t0 = Instant::now();
        let (out, r) = chop_clip(&clip, ChopStep::ticks(step).unwrap()).unwrap();
        let s = t0.elapsed().as_secs_f64();
        println!("chop {} -> {} notes: {:.1} ms = {:.1} M in/s, {:.1} M out/s", r.notes_before, r.notes_after, s * 1e3, n as f64 / s / 1e6, r.notes_after as f64 / s / 1e6);
        drop(out);
    }
}
