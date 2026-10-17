//! Win32 clipboard backend (§3): Win32 level, only `MidiPortalSequence`.
//!
//! - RegisterClipboardFormatW once (by name; the id is session-specific).
//! - Read: IsClipboardFormatAvailable first (distinguishes "absent" from
//!   "busy"), OpenClipboard with retries, GetClipboardData -> GlobalLock ->
//!   copy GlobalSize bytes (may include padding) -> unlock. CloseClipboard
//!   always runs (RAII guard).
//! - Write: same retry on OpenClipboard, EmptyClipboard, GlobalAlloc
//!   GMEM_MOVEABLE, SetClipboardData (ownership passes to the system on
//!   success; freed here on failure).
//! - No EnumClipboardFormats, no listeners, no messages.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HGLOBAL, HANDLE};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

use super::{ClipError, DominoClipboard, FORMAT_NAME};

pub struct Win32Clipboard {
    format_id: u32,
}

impl Win32Clipboard {
    pub fn new() -> Self {
        let wide: Vec<u16> = OsStr::new(FORMAT_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let format_id =
            unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(wide.as_ptr())) };
        Win32Clipboard { format_id }
    }

    /// OpenClipboard with bounded retries (10 x 20 ms per §3).
    fn open_with_retry(&self) -> Result<(), ClipError> {
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(ClipError::Busy);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Default for Win32Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard: CloseClipboard always runs.
struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

impl DominoClipboard for Win32Clipboard {
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError> {
        if self.format_id == 0 {
            return Err(ClipError::ReadFailed(
                "RegisterClipboardFormatW failed".into(),
            ));
        }
        // Format absent -> Ok(None), not an error (§3).
        if unsafe { IsClipboardFormatAvailable(self.format_id) }.is_err() {
            return Ok(None);
        }
        self.open_with_retry()?;
        let _guard = ClipboardGuard;
        let handle = unsafe { GetClipboardData(self.format_id) }
            .map_err(|e| ClipError::ReadFailed(format!("{e}")))?;
        let hglobal = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(hglobal) };
        if size == 0 {
            return Ok(None);
        }
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            return Err(ClipError::ReadFailed("GlobalLock failed".into()));
        }
        let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
        unsafe {
            let _ = GlobalUnlock(hglobal);
        }
        Ok(Some(data))
    }

    fn write(&mut self, raw: &[u8]) -> Result<(), ClipError> {
        if self.format_id == 0 {
            return Err(ClipError::WriteFailed(
                "RegisterClipboardFormatW failed".into(),
            ));
        }
        self.open_with_retry()?;
        let _guard = ClipboardGuard;
        unsafe {
            EmptyClipboard().map_err(|e| ClipError::WriteFailed(format!("{e}")))?;
        }
        let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, raw.len().max(1)) }
            .map_err(|e| ClipError::WriteFailed(format!("GlobalAlloc: {e}")))?;
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            unsafe {
                let _ = GlobalFree(hglobal);
            }
            return Err(ClipError::WriteFailed("GlobalLock failed".into()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(raw.as_ptr(), ptr as *mut u8, raw.len());
            let _ = GlobalUnlock(hglobal);
        }
        match unsafe { SetClipboardData(self.format_id, HANDLE(hglobal.0)) } {
            Ok(_) => Ok(()), // ownership passed to the system
            Err(e) => {
                unsafe {
                    let _ = GlobalFree(hglobal);
                }
                Err(ClipError::WriteFailed(format!("{e}")))
            }
        }
    }
}
