//! Peak-memory benchmark of the heavy path: chop to 1 tick, then export.
//! `cargo run --release --example membench -- [notes] [tracks] [out-dir]`
//!
//! Notes get gates of 1..=19 ticks (10 on average), so chopping every tick
//! makes ~10× as many notes. Prints the process's working set after each
//! stage and the peak since the previous line (Windows).

use std::time::Instant;

use dominokit_core::format::clip::NoteItem;
use dominokit_core::format::file::{self, FileKind};
use dominokit_core::format::template::fallback_clip;
use dominokit_core::ops::chop::{chop_clip, ChopStep};

#[cfg(windows)]
mod mem {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    /// (current, peak) private + working set in MB.
    pub fn now() -> (f64, f64) {
        let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (c.pagefile as f64 / 1e6, c.peak_pagefile as f64 / 1e6)
    }
}
#[cfg(not(windows))]
mod mem {
    pub fn now() -> (f64, f64) {
        (0.0, 0.0)
    }
}

fn report(stage: &str, t: Instant) {
    let (cur, peak) = mem::now();
    println!("{stage:<26} {:>7.0} ms   committed {cur:>8.0} MB   peak so far {peak:>8.0} MB", t.elapsed().as_secs_f64() * 1e3);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(10_000_000);
    let tracks: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(16);
    let dir = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| std::env::temp_dir().display().to_string()));
    let mut rng = 0x2545F491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let t = Instant::now();
    let per = n / tracks;
    let data: Vec<(Vec<NoteItem>, u32)> = (0..tracks)
        .map(|_| {
            let mut tick = 0u32;
            let v = (0..per)
                .map(|_| {
                    tick += next() % 4;
                    NoteItem::canonical(tick, 1 + next() % 19, (next() % 128) as u8, 100)
                })
                .collect();
            (v, tick + 100)
        })
        .collect();
    let clip = fallback_clip(480, &data);
    drop(data);
    report(&format!("built {} notes", n), t);

    let t = Instant::now();
    let (chopped, r) = chop_clip(&clip, ChopStep::ticks(1).unwrap()).unwrap();
    report(&format!("chopped -> {} notes", r.notes_after), t);
    // the app keeps the source for undo, so keep it alive like the app does

    let t = Instant::now();
    file::save_to(&chopped, FileKind::Mid, &dir.join("membench.mid")).unwrap();
    let mid_len = std::fs::metadata(dir.join("membench.mid")).unwrap().len();
    report(&format!("export .mid ({} MB)", mid_len >> 20), t);

    let t = Instant::now();
    match file::save_to(&chopped, FileKind::Dms, &dir.join("membench.dms")) {
        Ok(_) => {
            let len = std::fs::metadata(dir.join("membench.dms")).unwrap().len();
            report(&format!("export .dms ({} MB)", len >> 20), t);
        }
        Err(e) => println!("export .dms refused: {e}"),
    }
    let t = Instant::now();
    match file::clipboard_payload(&chopped, true) {
        Ok(p) => report(&format!("clipboard payload ({} MB)", p.len() >> 20), t),
        Err(e) => println!("clipboard payload refused: {e}"),
    }
    drop(clip);
    let _ = std::fs::remove_file(dir.join("membench.mid"));
    let _ = std::fs::remove_file(dir.join("membench.dms"));
}
