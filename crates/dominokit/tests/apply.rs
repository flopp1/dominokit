//! Headless tests of the user actions, through the fake clipboard.

use std::path::PathBuf;

use dominokit::app::state::App;
use dominokit::clipboard::FakeClipboard;
use dominokit::settings::Settings;

use dominokit_core::format::clip::{encode_clip, Clip, NoteItem};
use dominokit_core::format::file::{self, FileKind};
use dominokit_core::format::template::fallback_clip;

/// Three tracks, four quarter notes each.
fn demo_clip() -> Clip {
    let tracks: Vec<(Vec<NoteItem>, u32)> = (0..3u32)
        .map(|t| {
            let notes = [(0u32, 60u8), (480, 64), (960, 67), (1440, 72)]
                .iter()
                .map(|&(tick, key)| NoteItem::canonical(tick, 480, key + t as u8, 100))
                .collect();
            (notes, 1920)
        })
        .collect();
    fallback_clip(480, &tracks)
}

fn app_with_clipboard(clip: &Clip) -> App {
    let fake = FakeClipboard { data: Some(file::clipboard_payload(clip, false).unwrap()), fail: false };
    App::new(Box::new(fake), Settings::default())
}

fn keys(app: &App) -> Vec<u8> {
    app.source.as_ref().unwrap().note_views().iter().map(|n| n.key).collect()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dominokit-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn read_then_write_is_identity() {
    let clip = demo_clip();
    let mut app = app_with_clipboard(&clip);
    app.read_from_clipboard();
    assert!(!app.status_is_error, "{}", app.status);
    assert_eq!(app.source.as_ref().unwrap().note_count(), 12);
    app.write_to_clipboard();
    assert!(!app.status_is_error, "{}", app.status);
    // read back what was written: same unpacked bytes
    app.read_from_clipboard();
    assert_eq!(encode_clip(app.source.as_ref().unwrap()), encode_clip(&clip));
}

#[test]
fn flip_range_applies_to_all_tracks_then_undo() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    let before = keys(&app);
    // UI keys 61..=73 = MIDI 60..=72
    app.settings.flip_from = 61;
    app.settings.flip_to = 73;
    app.flip();
    assert!(!app.status_is_error, "{}", app.status);
    let expected: Vec<u8> = before.iter().map(|&k| if (60..=72).contains(&k) { 132 - k } else { k }).collect();
    assert_eq!(keys(&app), expected);
    app.undo();
    assert_eq!(keys(&app), before);
}

#[test]
fn flip_with_no_notes_in_range_changes_nothing() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    app.settings.flip_from = 1;
    app.settings.flip_to = 10;
    app.flip();
    assert!(app.status_is_error);
    assert_eq!(app.undo_len(), 0);
}

#[test]
fn chop_every_n_ticks_across_all_tracks() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    app.settings.chop_text = "120".into();
    app.chop();
    assert!(!app.status_is_error, "{}", app.status);
    let clip = app.source.as_ref().unwrap();
    assert_eq!(clip.note_count(), 48);
    assert!(clip.note_views().iter().all(|n| n.gate == 120 && n.tick % 120 == 0));
    assert_eq!(clip.track_count(), 3);
}

#[test]
fn note_value_fills_the_chop_box() {
    let mut app = app_with_clipboard(&demo_clip());
    app.settings.chop_ppq = 96;
    app.settings.chop_denominator = 8;
    app.settings.chop_modifier = "triplet".into();
    app.fill_chop_from_note_value();
    assert_eq!(app.settings.chop_text, "32");
    assert!(app.chop_matches_note_value());
    // reading a PPQ-480 clip keeps the box in step with the helper
    app.read_from_clipboard();
    assert_eq!(app.settings.chop_ppq, 480);
    assert_eq!(app.settings.chop_text, "160");
    // a hand-typed length is left alone
    app.settings.chop_text = "100".into();
    app.read_from_clipboard();
    assert_eq!(app.settings.chop_text, "100");
}

#[test]
fn memory_to_files_and_back() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    for kind in [FileKind::Dms, FileKind::Mid] {
        let path = temp(&format!("memory.{}", kind.extension()));
        app.save_file(&path, kind);
        assert!(!app.status_is_error, "{}", app.status);
        let mut other = App::new(Box::new(FakeClipboard::default()), Settings::default());
        other.open_file(&path);
        assert!(!other.status_is_error, "{}", other.status);
        let clip = other.source.as_ref().unwrap();
        // the file is a project: an added conductor track + the 3 tracks
        assert_eq!(clip.track_count(), 4, "{kind:?}");
        assert!(clip.tracks().next().unwrap().is_conductor());
        assert_eq!(clip.note_count(), 12);
        // writing it to the clipboard leaves the conductor out again
        other.write_to_clipboard();
        other.read_from_clipboard();
        assert_eq!(other.source.as_ref().unwrap().track_count(), 3);
    }
}

#[test]
fn clipboard_to_file_and_file_to_clipboard_skip_memory() {
    let clip = demo_clip();
    let mut app = app_with_clipboard(&clip);
    let path = temp("direct.dms");
    app.clipboard_to_file(&path, FileKind::Dms);
    assert!(!app.status_is_error, "{}", app.status);
    assert!(app.source.is_none(), "working copy must stay empty");

    let mut app2 = App::new(Box::new(FakeClipboard::default()), Settings::default());
    app2.file_to_clipboard(&path);
    assert!(!app2.status_is_error, "{}", app2.status);
    assert!(app2.source.is_none(), "working copy must stay empty");
    app2.read_from_clipboard();
    assert_eq!(encode_clip(app2.source.as_ref().unwrap()), encode_clip(&clip));
}

#[test]
fn friendly_errors() {
    let mut app = App::new(Box::new(FakeClipboard::default()), Settings::default());
    app.read_from_clipboard();
    assert!(app.status_is_error);
    app.write_to_clipboard();
    assert!(app.status_is_error);
    app.open_file(&temp("does-not-exist.mid"));
    assert!(app.status_is_error);
    let junk = temp("junk.dms");
    std::fs::write(&junk, b"not a domino file").unwrap();
    app.open_file(&junk);
    assert!(app.status_is_error && app.status.contains("not a Domino"), "{}", app.status);
}

#[test]
fn chop_then_flip_undo_in_order() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    let original = encode_clip(app.source.as_ref().unwrap());
    app.settings.chop_text = "240".into();
    app.chop();
    app.settings.flip_from = 1;
    app.settings.flip_to = 128;
    app.flip();
    assert_eq!(app.undo_len(), 2);
    assert_eq!(app.stats.as_ref().unwrap().notes, 24);
    app.undo(); // flip
    app.undo(); // chop
    assert_eq!(encode_clip(app.source.as_ref().unwrap()), original);
    assert_eq!(app.stats.as_ref().unwrap().notes, 12);
    app.undo();
    assert!(app.status_is_error, "nothing left to undo");
}

#[test]
fn stats_follow_every_change() {
    let mut app = app_with_clipboard(&demo_clip());
    app.read_from_clipboard();
    let g = app.generation;
    assert_eq!(app.stats.as_ref().unwrap().tracks.len(), 3);
    app.settings.chop_text = "120".into();
    app.chop();
    assert!(app.generation > g);
    assert_eq!(app.stats.as_ref().unwrap().notes, 48);
    app.clear();
    assert!(app.stats.is_none());
}

#[test]
fn actions_run_on_a_worker_and_hand_the_state_back() {
    use dominokit::app::jobs::{Action, Job};
    let app = app_with_clipboard(&demo_clip());
    let main = std::thread::current().id();
    let (tx, rx) = std::sync::mpsc::channel();
    let job = Job::start(app, Action::Read, move || tx.send(std::thread::current().id()).unwrap());
    let worker = rx.recv().unwrap();
    assert_ne!(worker, main, "the action must not run on the UI thread");
    let mut app = job.join(|| unreachable!());
    assert_eq!(app.stats.as_ref().unwrap().notes, 12);
    app.request(Action::Chop);
    let action = app.requests.remove(0);
    let job = Job::start(app, action, || {});
    let app = job.join(|| unreachable!());
    assert!(!app.status_is_error, "{}", app.status);
}
