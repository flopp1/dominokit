//! Clipboard access (§1.2, §3).
//!
//! Rules: DominoKit touches the clipboard only on an explicit Read/Write
//! button press, only ever exchanges the `MidiPortalSequence` format, and
//! never enumerates, listens to, or simulates anything.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ClipError {
    /// This platform has no clipboard backend compiled in.
    #[error("clipboard access is not supported on this platform")]
    Unsupported,
    /// Another application held the clipboard open (after bounded retries).
    #[error("the clipboard is busy (another application is holding it)")]
    Busy,
    /// The format is present but Win32 refused to hand over the data.
    #[error("failed to read the clipboard: {0}")]
    ReadFailed(String),
    /// Writing failed after OpenClipboard succeeded.
    #[error("failed to write the clipboard: {0}")]
    WriteFailed(String),
}

/// The registered clipboard format name — the only format DominoKit ever
/// names in code (§1.2, grep-able).
pub const FORMAT_NAME: &str = "MidiPortalSequence";

pub trait DominoClipboard: Send {
    /// Ok(None) = our format isn't on the clipboard. Err(Busy) after
    /// bounded retries.
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError>;
    fn write(&mut self, raw: &[u8]) -> Result<(), ClipError>;
}

/// Backend actually compiled in.
#[cfg(windows)]
pub mod windows;
#[cfg(not(windows))]
pub mod unsupported;

#[cfg(windows)]
pub use windows::Win32Clipboard;

#[cfg(not(windows))]
pub use unsupported::UnsupportedClipboard;

/// Create the platform backend.
pub fn platform_clipboard() -> impl DominoClipboard {
    #[cfg(windows)]
    {
        windows::Win32Clipboard::new()
    }
    #[cfg(not(windows))]
    {
        unsupported::UnsupportedClipboard
    }
}

/// Trait-based fake for tests and headless driving of the app.
#[derive(Default)]
pub struct FakeClipboard {
    pub data: Option<Vec<u8>>,
    pub fail: bool,
}

impl DominoClipboard for FakeClipboard {
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError> {
        if self.fail {
            Err(ClipError::Busy)
        } else {
            Ok(self.data.clone())
        }
    }

    fn write(&mut self, raw: &[u8]) -> Result<(), ClipError> {
        if self.fail {
            Err(ClipError::Busy)
        } else {
            self.data = Some(raw.to_vec());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_round_trip() {
        let mut clip = FakeClipboard::default();
        assert_eq!(clip.read().unwrap(), None);
        clip.write(b"PortalSequenceData").unwrap();
        assert_eq!(clip.read().unwrap(), Some(b"PortalSequenceData".to_vec()));
    }
}
