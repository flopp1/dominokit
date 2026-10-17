//! Application state and every user action, free of GUI code so it can be
//! driven headlessly in tests.
//!
//! DominoKit holds one **working copy** in memory. It is filled from the
//! clipboard or a file, transformed by Flip / Chop (each undoable), and sent
//! back to the clipboard or a file. Two direct transfers move data between
//! the clipboard and a file without touching the working copy.
//!
//! Nothing here copies the working copy: writes encode straight from it,
//! Flip works in place (and is undone by flipping again), and Chop's undo
//! keeps the clip it replaced, within a memory budget.

use std::path::Path;
use std::time::Instant;

use dominokit_core::format::clip::{encode_clip, Clip};
use dominokit_core::format::file::{self, FileKind, Loaded};
use dominokit_core::ops::chop::{chop_clip, ChopStep, Modifier, NoteValue};
use dominokit_core::ops::flip::{flip_in_place, KeyRange};

use super::jobs::{Action, Progress};
use super::stats::ClipStats;
use crate::clipboard::{ClipError, DominoClipboard};
use crate::settings::Settings;

/// Memory the undo history may keep in replaced clips (the latest one is
/// always kept, whatever its size).
pub const UNDO_BUDGET_BYTES: usize = 512 << 20;
pub const UNDO_MAX_STEPS: usize = 64;

/// Where the working copy came from (shown in the header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Clipboard,
    File(String),
    Demo,
}

impl Origin {
    pub fn label(&self) -> String {
        match self {
            Origin::Clipboard => "from the clipboard".into(),
            Origin::File(name) => format!("from {name}"),
            Origin::Demo => "demo clip".into(),
        }
    }
}

/// One undoable step.
pub enum Undo {
    /// Flip is its own inverse: undoing it flips the same range again.
    Flip(KeyRange),
    /// The clip a Chop replaced.
    Replaced(Clip),
}

pub struct App {
    pub clipboard: Box<dyn DominoClipboard>,

    /// The working copy. Change it only through `set_source` / `changed`, so
    /// the cached statistics and the overview stay in step.
    pub source: Option<Clip>,
    pub origin: Option<Origin>,
    history: Vec<Undo>,
    /// Bumped whenever the working copy changes (views cache by it).
    pub generation: u64,
    /// Bumped whenever a new clip is loaded (the overview resets its view).
    pub load_id: u64,
    /// Statistics of the working copy, computed once per change.
    pub stats: Option<ClipStats>,

    pub settings: Settings,

    pub status: String,
    pub status_is_error: bool,

    /// Actions the UI asked for; the main loop runs them on a worker thread.
    pub requests: Vec<Action>,
    /// What the running action is doing (shown in the progress window).
    pub progress: Progress,
}

/// A clipboard that does nothing, for the UI's read-only stand-in.
struct NoClipboard;

impl DominoClipboard for NoClipboard {
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError> {
        Err(ClipError::Busy)
    }

    fn write(&mut self, _: &[u8]) -> Result<(), ClipError> {
        Err(ClipError::Busy)
    }
}

impl App {
    pub fn new(clipboard: Box<dyn DominoClipboard>, settings: Settings) -> App {
        App {
            clipboard,
            source: None,
            origin: None,
            history: Vec::new(),
            generation: 0,
            load_id: 0,
            stats: None,
            settings,
            status: "Copy notes in Domino (Ctrl+C), then press Read — or open a .dms / .mid file.".into(),
            status_is_error: false,
            requests: Vec::new(),
            progress: Progress::default(),
        }
    }

    /// Ask for an action (run on the worker thread by the main loop).
    pub fn request(&mut self, action: Action) {
        self.requests.push(action);
    }

    /// Report what the running action is doing now.
    pub fn stage(&self, what: &str) {
        self.progress.set(what);
    }

    /// A display-only copy for the UI while the real `App` is busy on the
    /// worker: same statistics, settings and view identity, no clip.
    pub fn stand_in(&self, status: String) -> App {
        App {
            clipboard: Box::new(NoClipboard),
            source: None,
            origin: self.origin.clone(),
            history: Vec::new(),
            generation: self.generation,
            load_id: self.load_id,
            stats: self.stats.clone(),
            settings: self.settings.clone(),
            status,
            status_is_error: false,
            requests: Vec::new(),
            progress: Progress::default(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status = msg.into();
        self.status_is_error = is_error;
    }

    fn fail(&mut self, msg: impl Into<String>) {
        self.set_status(msg, true);
    }

    /// Replace the working copy with a newly loaded clip.
    pub fn set_source(&mut self, clip: Option<Clip>, origin: Option<Origin>) {
        self.source = clip;
        self.origin = origin;
        self.history.clear();
        self.load_id += 1;
        self.changed();
    }

    /// Call after changing `source` in place.
    pub fn changed(&mut self) {
        self.stage("Updating statistics");
        self.generation += 1;
        self.stats = self.source.as_ref().map(ClipStats::compute);
    }

    pub fn undo_len(&self) -> usize {
        self.history.len()
    }

    fn load_into_memory(&mut self, loaded: Loaded, origin: Origin, started: Instant) {
        // follow the clip's PPQ; keep the chop box in step if it showed a
        // note value at the old PPQ
        if let Some(ppq) = loaded.clip.ppq() {
            let synced = self.chop_matches_note_value();
            self.settings.chop_ppq = ppq as u32;
            if synced {
                self.fill_chop_from_note_value();
            }
        }
        let warnings = loaded.clip.warnings.clone();
        let label = origin.label();
        self.set_source(Some(loaded.clip), Some(origin));
        let mut msg = format!("Loaded {} {label} in {}.", self.summary(), elapsed(started));
        if !warnings.is_empty() {
            msg.push_str(&format!(" Notes: {}.", warnings.join("; ")));
        }
        self.set_status(msg, false);
    }

    /// "3 tracks · 128 notes" for the working copy.
    pub fn summary(&self) -> String {
        self.stats.as_ref().map(ClipStats::summary).unwrap_or_default()
    }

    // ---- clipboard <-> memory ---------------------------------------------

    /// Raw clipboard payload, or an error already shown in the status line.
    fn read_clipboard_payload(&mut self) -> Option<Loaded> {
        self.stage("Reading the clipboard");
        let raw = match self.clipboard.read() {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                self.fail("Nothing from Domino on the clipboard. In Domino, select notes and press Ctrl+C, then try again.");
                return None;
            }
            Err(e) => {
                self.fail(clip_error_text(&e));
                return None;
            }
        };
        self.stage("Decompressing and decoding");
        match file::decode_payload(&raw) {
            Ok(loaded) => Some(loaded),
            Err(e) => {
                self.fail(format!("The clipboard holds {}.", file::describe_envelope_error(&e)));
                None
            }
        }
    }

    /// Clipboard → working copy (replaces it).
    pub fn read_from_clipboard(&mut self) {
        let started = Instant::now();
        if let Some(loaded) = self.read_clipboard_payload() {
            self.load_into_memory(loaded, Origin::Clipboard, started);
        }
    }

    /// Working copy → clipboard.
    pub fn write_to_clipboard(&mut self) {
        let started = Instant::now();
        let Some(clip) = &self.source else {
            self.fail("Nothing to write yet: read the clipboard or open a file first.");
            return;
        };
        let skip = self.settings.skip_conductor;
        let tracks = kept_tracks(clip, skip);
        self.progress.set("Encoding, checking and compressing");
        match file::clipboard_payload(clip, skip) {
            Ok(payload) => {
                if self.put_on_clipboard(&payload) {
                    self.set_status(format!("{} ({})", written_text(tracks), elapsed(started)), false);
                }
            }
            Err(msg) => self.fail(format!("Not written: {msg}.")),
        }
    }

    fn put_on_clipboard(&mut self, payload: &[u8]) -> bool {
        self.stage("Writing to the clipboard");
        match self.clipboard.write(payload) {
            Ok(()) => true,
            Err(e) => {
                self.fail(clip_error_text(&e));
                false
            }
        }
    }

    // ---- files <-> memory -------------------------------------------------

    /// File → working copy.
    pub fn open_file(&mut self, path: &Path) {
        let started = Instant::now();
        if let Some(loaded) = self.load_file(path) {
            self.load_into_memory(loaded, Origin::File(file_name(path)), started);
        }
    }

    /// Working copy → file (format from `kind`).
    pub fn save_file(&mut self, path: &Path, kind: FileKind) {
        let started = Instant::now();
        self.remember_dir(path);
        let Some(clip) = &self.source else {
            self.fail("Nothing to save yet: read the clipboard or open a file first.");
            return;
        };
        self.progress.set("Encoding and writing the file");
        let result = write_file(clip, path, kind);
        let summary = self.summary();
        match result {
            Ok(warnings) => self.set_status(saved_text(&summary, path, &warnings, started), false),
            Err(msg) => self.fail(msg),
        }
    }

    // ---- clipboard <-> file, bypassing the working copy -----------------

    /// Clipboard → file. The working copy is not touched.
    pub fn clipboard_to_file(&mut self, path: &Path, kind: FileKind) {
        let started = Instant::now();
        self.remember_dir(path);
        let Some(loaded) = self.read_clipboard_payload() else { return };
        let summary = ClipStats::compute(&loaded.clip).summary();
        self.stage("Encoding and writing the file");
        match write_file(&loaded.clip, path, kind) {
            Ok(warnings) => {
                let msg = saved_text(&summary, path, &warnings, started);
                self.set_status(format!("From the clipboard: {msg} Working copy unchanged."), false);
            }
            Err(msg) => self.fail(msg),
        }
    }

    /// File → clipboard. The working copy is not touched.
    pub fn file_to_clipboard(&mut self, path: &Path) {
        let started = Instant::now();
        let Some(loaded) = self.load_file(path) else { return };
        let skip = self.settings.skip_conductor;
        self.stage("Encoding, checking and compressing");
        match file::clipboard_payload(&loaded.clip, skip) {
            Ok(payload) => {
                if self.put_on_clipboard(&payload) {
                    let msg = written_text(kept_tracks(&loaded.clip, skip));
                    self.set_status(
                        format!("From {}: {msg} ({}; working copy unchanged)", file_name(path), elapsed(started)),
                        false,
                    );
                }
            }
            Err(msg) => self.fail(format!("Not written: {msg}.")),
        }
    }

    fn load_file(&mut self, path: &Path) -> Option<Loaded> {
        self.remember_dir(path);
        self.stage("Reading the file");
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.fail(format!("Could not read {}: {e}", file_name(path)));
                return None;
            }
        };
        self.stage("Decoding");
        match file::load(&bytes) {
            Ok(loaded) => Some(loaded),
            Err(e) => {
                self.fail(format!("Could not open {}: {e}.", file_name(path)));
                None
            }
        }
    }

    fn remember_dir(&mut self, path: &Path) {
        if let Some(dir) = path.parent() {
            self.settings.last_dir = Some(dir.to_path_buf());
        }
    }

    // ---- transforms --------------------------------------------------------

    pub fn flip_range(&self) -> KeyRange {
        KeyRange::from_ui(self.settings.flip_from, self.settings.flip_to)
    }

    /// Mirror the notes between the two keys, in every track.
    pub fn flip(&mut self) {
        let started = Instant::now();
        let range = self.flip_range();
        self.stage("Flipping");
        let Some(clip) = &mut self.source else {
            self.fail("Nothing to flip yet: read the clipboard or open a file first.");
            return;
        };
        let report = flip_in_place(clip, range);
        let (a, b) = (range.lo as u16 + 1, range.hi as u16 + 1);
        if report.notes_flipped == 0 {
            self.fail(format!("No notes between key {a} and key {b}; nothing changed."));
            return;
        }
        self.push_undo(Undo::Flip(range));
        self.changed();
        self.set_status(
            format!(
                "Flipped {} note{} between key {a} and key {b}{} in {}.",
                count(report.notes_flipped),
                plural(report.notes_flipped),
                if report.notes_outside > 0 {
                    format!(" ({} outside the range left alone)", count(report.notes_outside))
                } else {
                    String::new()
                },
                elapsed(started)
            ),
            false,
        );
    }

    /// The chop length currently in the box.
    pub fn chop_step(&self) -> Result<ChopStep, String> {
        ChopStep::parse(&self.settings.chop_text).map_err(|e| e.to_string())
    }

    pub fn chop_note_value(&self) -> NoteValue {
        NoteValue {
            denominator: self.settings.chop_denominator,
            modifier: match self.settings.chop_modifier.as_str() {
                "dotted" => Modifier::Dotted,
                "triplet" => Modifier::Triplet,
                _ => Modifier::Plain,
            },
        }
    }

    /// The note value at the helper's PPQ, as ticks.
    pub fn note_value_step(&self) -> Result<ChopStep, String> {
        ChopStep::from_note_value(self.settings.chop_ppq, self.chop_note_value()).map_err(|e| e.to_string())
    }

    /// Put the helper's note value into the chop box.
    pub fn fill_chop_from_note_value(&mut self) {
        if let Ok(step) = self.note_value_step() {
            self.settings.chop_text = step.text();
        }
    }

    /// True when the box still shows exactly what the helper would put there.
    pub fn chop_matches_note_value(&self) -> bool {
        match (self.note_value_step(), self.chop_step()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// Cut every note at each multiple of the chop length, in every track.
    pub fn chop(&mut self) {
        let started = Instant::now();
        let step = match self.chop_step() {
            Ok(s) => s,
            Err(e) => {
                self.fail(format!("Chop length: {e}."));
                return;
            }
        };
        let Some(source) = &self.source else {
            self.fail("Nothing to chop yet: read the clipboard or open a file first.");
            return;
        };
        self.progress.set("Chopping");
        match chop_clip(source, step) {
            Err(e) => self.fail(format!("Not chopped: {e}.")),
            Ok((_, report)) if report.notes_cut == 0 => {
                self.fail(format!("No note is longer than one {}-tick slice; nothing changed.", step.text()));
            }
            Ok((chopped, report)) => {
                let old = self.source.replace(chopped).expect("checked above");
                self.push_undo(Undo::Replaced(old));
                self.changed();
                self.set_status(
                    format!(
                        "Chopped {} note{} every {} ticks: {} notes became {} in {}.",
                        count(report.notes_cut),
                        plural(report.notes_cut),
                        step.text(),
                        count(report.notes_before),
                        count(report.notes_after),
                        elapsed(started)
                    ),
                    false,
                );
            }
        }
    }

    // ---- history -------------------------------------------------------------

    fn push_undo(&mut self, step: Undo) {
        self.history.push(step);
        if self.history.len() > UNDO_MAX_STEPS {
            self.history.remove(0);
        }
        // drop the oldest replaced clips beyond the budget, keeping the latest
        let mut used = 0usize;
        let mut keep_from = 0usize;
        for (i, u) in self.history.iter().enumerate().rev() {
            if let Undo::Replaced(c) = u {
                used += c.approx_bytes();
                if used > UNDO_BUDGET_BYTES && i + 1 < self.history.len() {
                    keep_from = i + 1;
                    break;
                }
            }
        }
        if keep_from > 0 {
            // steps before a dropped snapshot can't be reached any more
            self.history.drain(..keep_from);
        }
    }

    pub fn undo(&mut self) {
        let started = Instant::now();
        match self.history.pop() {
            Some(Undo::Flip(range)) => {
                if let Some(clip) = &mut self.source {
                    flip_in_place(clip, range);
                }
                self.changed();
                self.set_status(format!("Undid the flip ({}).", elapsed(started)), false);
            }
            Some(Undo::Replaced(prev)) => {
                self.source = Some(prev);
                self.changed();
                self.set_status(format!("Undid the chop ({}).", elapsed(started)), false);
            }
            None => self.fail("Nothing to undo."),
        }
    }

    /// Empty the working copy (the clipboard and files are not touched).
    pub fn clear(&mut self) {
        self.stage("Freeing memory");
        self.set_source(None, None);
        self.set_status("Working copy cleared.", false);
    }

    /// Unpacked chunk bytes of the working copy (the Inspector's dump).
    pub fn dump_bytes(&self) -> Vec<u8> {
        self.source.as_ref().map(encode_clip).unwrap_or_default()
    }
}

fn write_file(clip: &Clip, path: &Path, kind: FileKind) -> Result<Vec<String>, String> {
    if clip.track_count() == 0 {
        return Err("Nothing to save: the clip has no tracks.".into());
    }
    file::save_to(clip, kind, path).map_err(|e| format!("Not saved: {e}."))
}

fn kept_tracks(clip: &Clip, skip_conductor: bool) -> usize {
    clip.tracks().filter(|t| !(skip_conductor && t.is_conductor())).count()
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// 1234567 → "1,234,567".
pub fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn elapsed(started: Instant) -> String {
    let ms = started.elapsed().as_secs_f64() * 1e3;
    if ms < 10.0 { format!("{ms:.1} ms") } else { format!("{ms:.0} ms") }
}

fn written_text(tracks: usize) -> String {
    let mut msg = format!(
        "Wrote {tracks} track{} to the clipboard (its previous contents were replaced). Paste with Ctrl+V in Domino.",
        plural(tracks)
    );
    if tracks > 1 {
        msg.push_str(" Leave enough tracks below the highlighted one: Domino drops tracks past the last.");
    }
    msg
}

fn saved_text(summary: &str, path: &Path, warnings: &[String], started: Instant) -> String {
    let mut msg = format!("Saved {summary} to {} in {}.", file_name(path), elapsed(started));
    if !warnings.is_empty() {
        msg.push_str(&format!(" Not representable in MIDI: {}.", warnings.join("; ")));
    }
    msg
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn clip_error_text(e: &ClipError) -> String {
    match e {
        ClipError::Unsupported => "This platform has no Domino clipboard support (Windows only).".into(),
        ClipError::Busy => "The clipboard is busy (another program is holding it). Try again.".into(),
        other => format!("Clipboard error: {other}."),
    }
}
