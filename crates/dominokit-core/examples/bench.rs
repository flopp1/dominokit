//! Throughput benchmark: `cargo run --release --example bench -- [notes] [tracks]`
//!
//! Builds a synthetic clip, then times every pipeline stage and reports
//! notes per second.

use std::time::Instant;

use dominokit_core::format::clip::{decode_clip, encode_clip, Clip, NoteItem};
use dominokit_core::format::envelope::{self, Magic};
use dominokit_core::format::file::{self, FileKind};
use dominokit_core::format::template::fallback_clip;
use dominokit_core::ops::chop::{chop_clip, ChopStep};
use dominokit_core::ops::flip::{flip_clip, flip_in_place, KeyRange};

fn synth(notes: usize, tracks: usize) -> Clip {
    let per = notes / tracks;
    let mut rng = 0x2545F491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let tracks: Vec<(Vec<NoteItem>, u32)> = (0..tracks)
        .map(|_| {
            let mut tick = 0u32;
            let v = (0..per)
                .map(|_| {
                    tick += next() % 120;
                    NoteItem::canonical(tick, 60 + next() % 900, (next() % 128) as u8, 1 + (next() % 127) as u8)
                })
                .collect();
            (v, tick + 1000)
        })
        .collect();
    fallback_clip(480, &tracks)
}

fn time<T>(label: &str, notes: usize, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    let s = t.elapsed().as_secs_f64();
    println!("{label:<28} {:>9.1} ms  {:>8.1} M notes/s", s * 1e3, notes as f64 / s / 1e6);
    out
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let n = args.first().copied().unwrap_or(1_000_000);
    let tracks = args.get(1).copied().unwrap_or(16);
    println!("{n} notes in {tracks} tracks");
    let clip = time("build", n, || synth(n, tracks));
    let n = clip.note_count();
    let unpacked = time("encode clip", n, || encode_clip(&clip));
    println!("  unpacked {:.1} MB", unpacked.len() as f64 / 1e6);
    let decoded = time("decode clip", n, || decode_clip(&unpacked));
    assert_eq!(decoded.note_count(), n);
    let packed = time("zlib compress", n, || envelope::encode(&unpacked, Magic::PortalSequenceData));
    println!("  packed {:.1} MB", packed.len() as f64 / 1e6);
    time("zlib decompress", n, || envelope::decode(&packed, Magic::PortalSequenceData).unwrap());
    time("clipboard payload (all)", n, || file::clipboard_payload(&decoded, true).unwrap());
    time("load payload (all)", n, || file::decode_payload(&packed).unwrap());
    let (flipped, _) = time("flip (copy)", n, || flip_clip(&decoded, KeyRange::from_ui(1, 128)));
    let mut inplace = decoded.clone();
    time("flip (in place)", n, || flip_in_place(&mut inplace, KeyRange::from_ui(1, 128)));
    time("clone clip", n, || decoded.clone());
    let (chopped, r) = time("chop every 120", n, || chop_clip(&flipped, ChopStep::ticks(120).unwrap()).unwrap());
    println!("  chop: {} -> {} notes", r.notes_before, r.notes_after);
    let (mid, _) = time("export .mid", n, || file::save(&decoded, FileKind::Mid).unwrap());
    println!("  mid {:.1} MB", mid.len() as f64 / 1e6);
    let back = time("import .mid", n, || file::load(&mid).unwrap());
    assert_eq!(back.clip.note_count(), n);
    time("export .dms", n, || file::save(&decoded, FileKind::Dms).unwrap());
    drop(chopped);
}
