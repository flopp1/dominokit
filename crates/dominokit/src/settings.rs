//! JSON settings persistence: last-used options and window size. Clip
//! contents are never saved.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window_size: Option<[f32; 2]>,
    /// Flip range in the UI's 1-based key numbers (1..=256).
    pub flip_from: u16,
    pub flip_to: u16,
    /// Contents of the chop-length box (ticks, e.g. `120` or `100/3`).
    pub chop_text: String,
    /// The note-value helper that fills the chop box.
    pub chop_ppq: u32,
    /// 1 = whole … 64.
    pub chop_denominator: u32,
    /// "plain" | "dotted" | "triplet"
    pub chop_modifier: String,
    /// Leave the conductor track out when writing to the clipboard.
    pub skip_conductor: bool,
    /// Folder of the last opened/saved file.
    pub last_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            window_size: None,
            flip_from: 1,
            flip_to: 128,
            chop_text: "120".into(),
            chop_ppq: 480,
            chop_denominator: 16,
            chop_modifier: "plain".into(),
            skip_conductor: true,
            last_dir: None,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        match config_path() {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
                Err(_) => Settings::default(),
            },
            None => Settings::default(),
        }
    }

    pub fn save(&self) {
        if let Some(path) = config_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(&path, json);
            }
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "DominoKit")
        .map(|d| d.config_dir().join("settings.json"))
}
