//! DominoKit — flip and chop note data copied out of Domino, and move it
//! between the clipboard, `.dms` and `.mid` files.
//!
//! `dominokit [file]` opens a `.dms` / `.mid`; `--demo` loads a small
//! synthetic clip; `--stress N [tracks]` loads N random notes to try the UI
//! at extreme sizes.

use dominokit::app::state::{App, Origin};
use dominokit::app::DominoKitApp;
use dominokit::clipboard;
use dominokit::settings::Settings;

use dominokit_core::format::clip::NoteItem;
use dominokit_core::format::template::fallback_clip;

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name);
    let number = |i: usize| args.get(i).and_then(|a| a.parse::<usize>().ok());
    let settings = Settings::load();
    let window_size = settings.window_size.unwrap_or([1180.0, 780.0]);
    let mut app = App::new(Box::new(clipboard::platform_clipboard()), settings);
    if flag("--demo").is_some() {
        load_demo(&mut app);
    } else if let Some(i) = flag("--stress") {
        load_stress(&mut app, number(i + 1).unwrap_or(1_000_000), number(i + 2).unwrap_or(16));
    } else if let Some(path) = args.iter().find(|a| !a.starts_with("--")) {
        // `dominokit song.dms` (or a file dropped on the exe) opens that file
        app.request(dominokit::app::jobs::Action::Open(path.into()));
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DominoKit")
            .with_inner_size(window_size)
            .with_min_inner_size([820.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("DominoKit", options, Box::new(move |_cc| Box::new(DominoKitApp::new(app))))
}

/// `--demo`: a synthetic 3-track clip so the UI can be explored without Domino.
fn load_demo(app: &mut App) {
    let melodies: [&[(u32, u32, u8, u8)]; 3] = [
        &[(0, 240, 60, 100), (240, 240, 64, 100), (480, 240, 67, 100), (720, 240, 72, 100), (960, 960, 71, 90)],
        &[(0, 480, 36, 90), (480, 480, 43, 90), (960, 960, 48, 80)],
        &[(120, 120, 84, 60), (360, 120, 88, 60), (600, 120, 91, 60), (840, 120, 88, 60)],
    ];
    let tracks: Vec<(Vec<NoteItem>, u32)> = melodies
        .iter()
        .map(|m| (m.iter().map(|&(t, g, k, v)| NoteItem::canonical(t, g, k, v)).collect(), 1920))
        .collect();
    let mut clip = fallback_clip(480, &tracks);
    for (i, t) in clip.tracks_mut().enumerate() {
        t.set_setting(1002, format!("Demo {}", i + 1).into_bytes());
        t.set_setting(1001, vec![i as u8]);
    }
    app.set_source(Some(clip), Some(Origin::Demo));
    app.set_status("Demo clip loaded (3 tracks).", false);
}

/// `--stress N [tracks]`: N random notes, dense and overlapping.
fn load_stress(app: &mut App, notes: usize, tracks: usize) {
    let tracks = tracks.max(1);
    let mut rng = 0x2545_F491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let per = notes / tracks;
    let data: Vec<(Vec<NoteItem>, u32)> = (0..tracks)
        .map(|_| {
            let mut tick = 0u32;
            let v: Vec<NoteItem> = (0..per)
                .map(|_| {
                    tick += next() % 24;
                    NoteItem::canonical(tick, 30 + next() % 900, (next() % 128) as u8, 1 + (next() % 127) as u8)
                })
                .collect();
            (v, tick + 960)
        })
        .collect();
    app.set_source(Some(fallback_clip(480, &data)), Some(Origin::Demo));
    let summary = app.summary();
    app.set_status(format!("Stress test: {summary}."), false);
}
