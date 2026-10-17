//! Background work: every user action runs on a worker thread so the window
//! keeps repainting (and can be moved, resized or closed) while millions of
//! notes are processed.
//!
//! The UI never calls the heavy `App` methods itself: it queues an
//! [`Action`] with [`App::request`]. The main loop then moves the whole
//! `App` into a worker thread, runs the action there, and gets the `App`
//! back when it is done. Meanwhile the window draws a read-only stand-in
//! (the last statistics and overview picture, controls disabled) and, once
//! the work has taken more than a second, a progress window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use dominokit_core::format::file::FileKind;

use super::state::App;

/// Something the user asked for.
#[derive(Debug, Clone)]
pub enum Action {
    Read,
    Write,
    Open(PathBuf),
    Save(PathBuf, FileKind),
    ClipboardToFile(PathBuf, FileKind),
    FileToClipboard(PathBuf),
    Flip,
    Chop,
    Undo,
    Clear,
    Dump(PathBuf),
}

impl Action {
    /// Title of the progress window.
    pub fn title(&self) -> String {
        let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match self {
            Action::Read => "Reading the clipboard".into(),
            Action::Write => "Writing to the clipboard".into(),
            Action::Open(p) => format!("Opening {}", name(p)),
            Action::Save(p, _) => format!("Saving {}", name(p)),
            Action::ClipboardToFile(p, _) => format!("Saving the clipboard as {}", name(p)),
            Action::FileToClipboard(p) => format!("Putting {} on the clipboard", name(p)),
            Action::Flip => "Flipping".into(),
            Action::Chop => "Chopping".into(),
            Action::Undo => "Undoing".into(),
            Action::Clear => "Clearing".into(),
            Action::Dump(p) => format!("Saving {}", name(p)),
        }
    }

    pub fn run(self, app: &mut App) {
        match self {
            Action::Read => app.read_from_clipboard(),
            Action::Write => app.write_to_clipboard(),
            Action::Open(p) => app.open_file(&p),
            Action::Save(p, kind) => app.save_file(&p, kind),
            Action::ClipboardToFile(p, kind) => app.clipboard_to_file(&p, kind),
            Action::FileToClipboard(p) => app.file_to_clipboard(&p),
            Action::Flip => app.flip(),
            Action::Chop => app.chop(),
            Action::Undo => app.undo(),
            Action::Clear => app.clear(),
            Action::Dump(p) => {
                app.stage("Encoding");
                match std::fs::write(&p, app.dump_bytes()) {
                    Ok(()) => app.set_status(format!("Dump saved to {}", p.display()), false),
                    Err(e) => app.set_status(format!("Could not save the dump: {e}"), true),
                }
            }
        }
    }
}

/// What the worker is doing right now, readable from the UI thread.
#[derive(Clone, Default)]
pub struct Progress(Arc<Mutex<String>>);

impl Progress {
    pub fn set(&self, stage: &str) {
        if let Ok(mut s) = self.0.lock() {
            stage.clone_into(&mut s);
        }
    }

    pub fn get(&self) -> String {
        self.0.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// An action running on the worker thread.
pub struct Job {
    pub title: String,
    pub started: Instant,
    pub progress: Progress,
    handle: JoinHandle<App>,
}

impl Job {
    /// Move `app` to a worker thread and run `action` there. `on_done` is
    /// called from the worker when it finishes (to wake the UI).
    pub fn start(mut app: App, action: Action, on_done: impl FnOnce() + Send + 'static) -> Job {
        let title = action.title();
        let progress = app.progress.clone();
        progress.set("Starting");
        let handle = std::thread::Builder::new()
            .name("dominokit-worker".into())
            .spawn(move || {
                // testing aid: DOMINOKIT_SLOW_JOBS=<ms> makes every action take longer
                if let Some(ms) = std::env::var("DOMINOKIT_SLOW_JOBS").ok().and_then(|v| v.parse().ok()) {
                    app.progress.set("Waiting (DOMINOKIT_SLOW_JOBS)");
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                action.run(&mut app);
                app.progress.set("");
                on_done();
                app
            })
            .expect("spawn the worker thread");
        Job { title, started: Instant::now(), progress, handle }
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Wait for the worker and take the `App` back. If the action panicked,
    /// the clip is lost but the program goes on with an empty working copy.
    pub fn join(self, fallback: impl FnOnce() -> App) -> App {
        match self.handle.join() {
            Ok(app) => app,
            Err(_) => {
                let mut app = fallback();
                app.set_status(format!("{} failed unexpectedly; the working copy was lost.", self.title), true);
                app
            }
        }
    }
}
