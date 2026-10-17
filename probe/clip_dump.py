"""Save Domino's clipboard data (format "MidiPortalSequence") to a file.

Read-only: touches only that one clipboard format, once, when run.
Usage:  python clip_dump.py OUT.bin      then:  python domino_dump.py OUT.bin
"""
import ctypes
import sys
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32.RegisterClipboardFormatW.restype = wintypes.UINT
user32.RegisterClipboardFormatW.argtypes = [wintypes.LPCWSTR]
user32.IsClipboardFormatAvailable.argtypes = [wintypes.UINT]
user32.OpenClipboard.argtypes = [wintypes.HWND]
user32.GetClipboardData.restype = wintypes.HANDLE
user32.GetClipboardData.argtypes = [wintypes.UINT]
kernel32.GlobalLock.restype = ctypes.c_void_p
kernel32.GlobalLock.argtypes = [wintypes.HGLOBAL]
kernel32.GlobalUnlock.argtypes = [wintypes.HGLOBAL]
kernel32.GlobalSize.restype = ctypes.c_size_t
kernel32.GlobalSize.argtypes = [wintypes.HGLOBAL]


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    fmt = user32.RegisterClipboardFormatW("MidiPortalSequence")
    if not user32.IsClipboardFormatAvailable(fmt):
        sys.exit("No Domino data on the clipboard. Copy something in Domino first.")
    if not user32.OpenClipboard(None):
        sys.exit("Clipboard is busy; try again.")
    try:
        h = user32.GetClipboardData(fmt)
        if not h:
            sys.exit("GetClipboardData failed.")
        size = kernel32.GlobalSize(h)
        ptr = kernel32.GlobalLock(h)
        data = ctypes.string_at(ptr, size)
        kernel32.GlobalUnlock(h)
    finally:
        user32.CloseClipboard()
    with open(sys.argv[1], "wb") as f:
        f.write(data)
    print(f"saved {size} bytes to {sys.argv[1]}")


if __name__ == "__main__":
    main()
