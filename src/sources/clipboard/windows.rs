use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_HDROP;
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP as u32) == 0 {
            return Vec::new();
        }
        if OpenClipboard(ptr::null_mut()) == 0 {
            return Vec::new();
        }
        let handle = GetClipboardData(CF_HDROP as u32);
        let mut paths = Vec::new();
        if !handle.is_null() {
            append_hdrop_paths(handle as HDROP, &mut paths);
        }
        let _ = CloseClipboard();
        paths
    }
}

pub(super) fn clipboard_pureref_binary() -> Option<Vec<u8>> {
    let format = registered_clipboard_format("pureref/binary") as u32;
    unsafe {
        if format == 0 || IsClipboardFormatAvailable(format) == 0 {
            return None;
        }
        clipboard_hglobal_bytes(format)
    }
}

pub(crate) unsafe fn append_hdrop_paths(hdrop: HDROP, paths: &mut Vec<std::path::PathBuf>) {
    let item_count = unsafe { DragQueryFileW(hdrop, 0xffffffff, ptr::null_mut(), 0) };
    for index in 0..item_count {
        let character_count = unsafe { DragQueryFileW(hdrop, index, ptr::null_mut(), 0) };
        if character_count == 0 {
            continue;
        }
        let mut buffer = Vec::with_capacity(character_count as usize + 1);
        unsafe {
            DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), character_count + 1);
            buffer.set_len(character_count as usize);
        }
        let path = std::path::PathBuf::from(OsString::from_wide(&buffer));
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
}

unsafe fn clipboard_hglobal_bytes(format: u32) -> Option<Vec<u8>> {
    if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
        return None;
    }

    let bytes = (|| {
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            return None;
        }
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return None;
        }
        let size = unsafe { GlobalSize(handle) };
        let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec();
        let _ = unsafe { GlobalUnlock(handle) };
        Some(bytes)
    })();

    let _ = unsafe { CloseClipboard() };
    bytes
}

pub(crate) fn registered_clipboard_format(name: &str) -> u16 {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe { RegisterClipboardFormatW(wide.as_ptr()) as u16 }
}

pub(super) fn append_internal_text(text: &str) -> Result<(), String> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            return Err("Clipboard unavailable".to_owned());
        }

        let result = (|| {
            let mut wide = text.encode_utf16().collect::<Vec<_>>();
            wide.push(0);
            let byte_len = wide
                .len()
                .checked_mul(std::mem::size_of::<u16>())
                .ok_or_else(|| "clipboard text dimensions overflowed".to_owned())?;
            let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, byte_len);
            if handle.is_null() {
                return Err("Failed to allocate clipboard text".to_owned());
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                let _ = GlobalFree(handle);
                return Err("Failed to lock clipboard text".to_owned());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), pointer.cast(), byte_len);
            let _ = GlobalUnlock(handle);
            if SetClipboardData(CF_UNICODETEXT as u32, handle).is_null() {
                let _ = GlobalFree(handle);
                return Err("Failed to add SnappyRef clipboard metadata".to_owned());
            }
            Ok(())
        })();

        let _ = CloseClipboard();
        result
    }
}
