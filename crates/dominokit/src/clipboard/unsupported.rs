//! Non-Windows stub: the GUI still runs, with Read/Write disabled (§3).

use super::{ClipError, DominoClipboard};

pub struct UnsupportedClipboard;

impl DominoClipboard for UnsupportedClipboard {
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError> {
        Err(ClipError::Unsupported)
    }

    fn write(&mut self, _raw: &[u8]) -> Result<(), ClipError> {
        Err(ClipError::Unsupported)
    }
}
