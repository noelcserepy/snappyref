use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{
    DRAGDROP_E_ALREADYREGISTERED, DV_E_FORMATETC, E_NOINTERFACE, HGLOBAL, HWND, POINT, POINTL, S_OK,
};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY, DROPEFFECT_NONE, OleInitialize, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows_sys::core::{GUID, HRESULT};

type IUnknown = c_void;

static STATE: OnceLock<Mutex<NativeDropState>> = OnceLock::new();
static REGISTRATION: OnceLock<Mutex<Option<DropTargetRegistration>>> = OnceLock::new();
static WINDOW_HWND: OnceLock<isize> = OnceLock::new();

pub(super) fn install(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as HWND;
    if hwnd.is_null() {
        return;
    }
    let _ = WINDOW_HWND.set(hwnd as isize);

    let registration = REGISTRATION.get_or_init(|| Mutex::new(None));
    let mut registration = registration
        .lock()
        .expect("native drop registration mutex poisoned");
    if registration.is_some() {
        return;
    }

    unsafe {
        let _ = OleInitialize(ptr::null());
    }
    let mut target = DropTargetRegistration::new(hwnd);
    let result = unsafe { RegisterDragDrop(hwnd, target.data as *mut c_void) };
    if result >= 0 {
        target.registered = true;
        *registration = Some(target);
    } else if result == DRAGDROP_E_ALREADYREGISTERED {
        // Winit already registered the window for file drops. Keep that
        // working rather than replacing it with a custom bridge that is
        // only needed for richer URL payloads.
        drop(target);
    }
}

fn hovered_urls() -> Vec<String> {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .hovered_urls
        .clone()
}

fn hovered_paths() -> Vec<std::path::PathBuf> {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .hovered_paths
        .clone()
}

fn hovered_screen_pos() -> Option<egui::Pos2> {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .hovered_pos
}

pub(super) fn cursor_screen_pos() -> Option<egui::Pos2> {
    let hwnd = *WINDOW_HWND.get()? as HWND;
    let mut cursor = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0 {
        return None;
    }
    unsafe {
        client_point(
            hwnd,
            POINTL {
                x: cursor.x,
                y: cursor.y,
            },
        )
    }
}

fn take_dropped_urls() -> Vec<String> {
    std::mem::take(
        &mut state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped_urls,
    )
}

fn take_dropped_paths() -> Vec<std::path::PathBuf> {
    std::mem::take(
        &mut state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped_paths,
    )
}

fn take_dropped_screen_pos() -> Option<egui::Pos2> {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .dropped_pos
        .take()
}

fn state() -> &'static Mutex<NativeDropState> {
    STATE.get_or_init(|| Mutex::new(NativeDropState::default()))
}

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

#[repr(C)]
struct IUnknownVtbl {
    QueryInterface: unsafe extern "system" fn(
        this: *mut IUnknown,
        riid: *const GUID,
        ppv_object: *mut *mut c_void,
    ) -> HRESULT,
    AddRef: unsafe extern "system" fn(this: *mut IUnknown) -> u32,
    Release: unsafe extern "system" fn(this: *mut IUnknown) -> u32,
}

#[repr(C)]
struct IDataObjectVtbl {
    parent: IUnknownVtbl,
    GetData: unsafe extern "system" fn(
        this: *mut c_void,
        format: *const FORMATETC,
        medium: *mut STGMEDIUM,
    ) -> HRESULT,
}

#[repr(C)]
struct IDropTargetVtbl {
    parent: IUnknownVtbl,
    DragEnter: unsafe extern "system" fn(
        this: *mut IDropTarget,
        data_object: *const c_void,
        key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT,
    DragOver: unsafe extern "system" fn(
        this: *mut IDropTarget,
        key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT,
    DragLeave: unsafe extern "system" fn(this: *mut IDropTarget) -> HRESULT,
    Drop: unsafe extern "system" fn(
        this: *mut IDropTarget,
        data_object: *const c_void,
        key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT,
}

#[repr(C)]
struct IDropTarget {
    vtbl: *const IDropTargetVtbl,
}

struct DropTargetData {
    interface: IDropTarget,
    refcount: AtomicUsize,
    hwnd: HWND,
    cursor_effect: u32,
    hovered_is_valid: bool,
}

struct DropTargetRegistration {
    data: *mut DropTargetData,
    registered: bool,
}

unsafe impl Send for DropTargetRegistration {}

impl DropTargetRegistration {
    fn new(hwnd: HWND) -> Self {
        let data = Box::new(DropTargetData {
            interface: IDropTarget {
                vtbl: &DROP_TARGET_VTBL,
            },
            refcount: AtomicUsize::new(1),
            hwnd,
            cursor_effect: DROPEFFECT_NONE,
            hovered_is_valid: false,
        });
        Self {
            data: Box::into_raw(data),
            registered: false,
        }
    }
}

impl Drop for DropTargetRegistration {
    fn drop(&mut self) {
        unsafe {
            if self.registered {
                let hwnd = (*self.data).hwnd;
                let _ = RevokeDragDrop(hwnd);
            }
            DropTarget::release(self.data as *mut IUnknown);
        }
    }
}

struct DropTarget;

impl DropTarget {
    unsafe extern "system" fn query_interface(
        this: *mut IUnknown,
        riid: *const GUID,
        ppv_object: *mut *mut c_void,
    ) -> HRESULT {
        const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
        const IID_IDROPTARGET: GUID = GUID::from_u128(0x00000122_0000_0000_c000_000000000046);
        if ppv_object.is_null() {
            return E_NOINTERFACE;
        }
        unsafe {
            if !riid.is_null()
                && (guid_eq(&*riid, &IID_IUNKNOWN) || guid_eq(&*riid, &IID_IDROPTARGET))
            {
                *ppv_object = this.cast();
                Self::add_ref(this);
                S_OK
            } else {
                *ppv_object = ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }

    unsafe extern "system" fn add_ref(this: *mut IUnknown) -> u32 {
        let data = unsafe { Self::from_interface(this) };
        (data.refcount.fetch_add(1, Ordering::Release) + 1) as u32
    }

    unsafe extern "system" fn release(this: *mut IUnknown) -> u32 {
        let data = unsafe { Self::from_interface(this) };
        let count = data.refcount.fetch_sub(1, Ordering::Release) - 1;
        if count == 0 {
            drop(unsafe { Box::from_raw(data as *mut DropTargetData) });
        }
        count as u32
    }

    unsafe extern "system" fn drag_enter(
        this: *mut IDropTarget,
        data_object: *const c_void,
        _key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        let data = unsafe { Self::from_interface(this) };
        let payload = unsafe { extract_drop_payload(data_object) };
        data.hovered_is_valid = !payload.is_empty();
        let pos = unsafe { client_point_from_ptr(data.hwnd, point) };
        data.cursor_effect = if data.hovered_is_valid {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered(payload, pos);
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        };
        unsafe {
            *effect = data.cursor_effect;
        }
        S_OK
    }

    unsafe extern "system" fn drag_over(
        this: *mut IDropTarget,
        _key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        let data = unsafe { Self::from_interface(this) };
        let pos = unsafe { client_point_from_ptr(data.hwnd, point) };
        if data.hovered_is_valid {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered_pos(pos);
        }
        unsafe {
            *effect = data.cursor_effect;
        }
        S_OK
    }

    unsafe extern "system" fn drag_leave(this: *mut IDropTarget) -> HRESULT {
        let data = unsafe { Self::from_interface(this) };
        if data.hovered_is_valid {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .clear_hovered();
        }
        data.hovered_is_valid = false;
        data.cursor_effect = DROPEFFECT_NONE;
        S_OK
    }

    unsafe extern "system" fn drop(
        this: *mut IDropTarget,
        data_object: *const c_void,
        _key_state: u32,
        point: *const POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        let data = unsafe { Self::from_interface(this) };
        let payload = unsafe { extract_drop_payload(data_object) };
        let pos = unsafe { client_point_from_ptr(data.hwnd, point) };
        if !payload.is_empty() {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_dropped(payload, pos);
            unsafe {
                *effect = DROPEFFECT_COPY;
            }
        } else {
            unsafe {
                *effect = DROPEFFECT_NONE;
            }
        }
        data.hovered_is_valid = false;
        data.cursor_effect = DROPEFFECT_NONE;
        S_OK
    }

    unsafe fn from_interface<'a, InterfaceT>(this: *mut InterfaceT) -> &'a mut DropTargetData {
        unsafe { &mut *(this as *mut DropTargetData) }
    }
}

unsafe fn client_point_from_ptr(hwnd: HWND, point: *const POINTL) -> Option<egui::Pos2> {
    let point = unsafe { point.as_ref()? };
    unsafe { client_point(hwnd, *point) }
}

unsafe fn client_point(hwnd: HWND, point: POINTL) -> Option<egui::Pos2> {
    let mut client = POINT {
        x: point.x,
        y: point.y,
    };
    if unsafe { ScreenToClient(hwnd, &mut client) } == 0 {
        return None;
    }
    Some(egui::pos2(client.x as f32, client.y as f32))
}

static DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    parent: IUnknownVtbl {
        QueryInterface: DropTarget::query_interface,
        AddRef: DropTarget::add_ref,
        Release: DropTarget::release,
    },
    DragEnter: DropTarget::drag_enter,
    DragOver: DropTarget::drag_over,
    DragLeave: DropTarget::drag_leave,
    Drop: DropTarget::drop,
};

unsafe fn extract_drop_payload(data_object: *const c_void) -> super::NativeDropPayload {
    let mut payload = super::NativeDropPayload::default();
    unsafe {
        append_paths(data_object, &mut payload.paths);
        for text in drop_text_values(data_object) {
            super::payload::append_http_urls_from_text(&text, &mut payload.urls);
        }
    }
    payload.urls = super::payload::best_drag_image_urls(payload.urls);
    payload
}

unsafe fn append_paths(data_object: *const c_void, paths: &mut Vec<std::path::PathBuf>) {
    let _ = unsafe {
        with_hglobal_data(data_object, CF_HDROP, |hglobal, _pointer, _size| {
            append_hdrop_paths(hglobal as HDROP, paths);
        })
    };
}

unsafe fn append_hdrop_paths(hdrop: HDROP, paths: &mut Vec<std::path::PathBuf>) {
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

unsafe fn drop_text_values(data_object: *const c_void) -> Vec<String> {
    let mut values = Vec::new();
    for format in [
        CF_UNICODETEXT,
        registered_clipboard_format("UniformResourceLocatorW"),
        registered_clipboard_format("UniformResourceLocator"),
        registered_clipboard_format("text/html"),
        registered_clipboard_format("HTML Format"),
    ] {
        if format == 0 {
            continue;
        }
        let is_utf16 = format == CF_UNICODETEXT
            || format == registered_clipboard_format("UniformResourceLocatorW");
        let value = unsafe {
            with_hglobal_data(data_object, format, |_hglobal, pointer, size| {
                if is_utf16 {
                    utf16_hglobal_text(pointer, size)
                } else {
                    bytes_hglobal_text(pointer, size)
                }
            })
        }
        .flatten();
        if let Some(value) = value {
            if !value.is_empty() {
                values.push(value);
            }
        }
    }
    values
}

unsafe fn with_hglobal_data<T>(
    data_object: *const c_void,
    format: u16,
    read: impl FnOnce(HGLOBAL, *const u8, usize) -> T,
) -> Option<T> {
    let format_etc = FORMATETC {
        cfFormat: format,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL as u32,
    };
    let mut medium: STGMEDIUM = unsafe { std::mem::zeroed() };
    let get_data = unsafe { (*(data_object as *mut *mut IDataObjectVtbl)).as_ref()? }.GetData;
    let result = unsafe { get_data(data_object.cast_mut(), &format_etc, &mut medium) };
    if result == DV_E_FORMATETC || result < 0 {
        return None;
    }
    let hglobal = unsafe { medium.u.hGlobal };
    unsafe {
        let pointer = GlobalLock(hglobal);
        if pointer.is_null() {
            ReleaseStgMedium(&mut medium);
            return None;
        }
        let size = GlobalSize(hglobal);
        let value = read(hglobal, pointer.cast::<u8>(), size);
        let _ = GlobalUnlock(hglobal);
        ReleaseStgMedium(&mut medium);
        Some(value)
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

fn registered_clipboard_format(name: &str) -> u16 {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe { RegisterClipboardFormatW(wide.as_ptr()) as u16 }
}

fn guid_eq(left: &GUID, right: &GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

fn utf16_hglobal_text(pointer: *const u8, size: usize) -> Option<String> {
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) };
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    (!units.is_empty())
        .then(|| String::from_utf16_lossy(&units).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn bytes_hglobal_text(pointer: *const u8, size: usize) -> Option<String> {
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) };
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    (!value.is_empty()).then_some(value)
}
// Preserve the established Windows read order until native verification.
#[derive(Default)]
struct NativeDropState {
    hovered_urls: Vec<String>,
    dropped_urls: Vec<String>,
    hovered_paths: Vec<std::path::PathBuf>,
    dropped_paths: Vec<std::path::PathBuf>,
    hovered_pos: Option<egui::Pos2>,
    dropped_pos: Option<egui::Pos2>,
}

impl NativeDropState {
    fn set_hovered(&mut self, payload: super::NativeDropPayload, pos: Option<egui::Pos2>) {
        self.hovered_urls = payload.urls;
        self.hovered_paths = payload.paths;
        self.hovered_pos = pos;
    }

    fn set_hovered_pos(&mut self, pos: Option<egui::Pos2>) {
        self.hovered_pos = pos;
    }

    fn clear_hovered(&mut self) {
        self.hovered_urls.clear();
        self.hovered_paths.clear();
        self.hovered_pos = None;
    }

    fn set_dropped(&mut self, payload: super::NativeDropPayload, pos: Option<egui::Pos2>) {
        self.clear_hovered();
        self.dropped_urls.extend(payload.urls);
        self.dropped_paths.extend(payload.paths);
        self.dropped_pos = pos;
    }
}

pub(super) fn hovered() -> super::NativeDropPayload {
    let paths = hovered_paths();
    let urls = hovered_urls();
    let position = hovered_screen_pos();
    let cursor = cursor_screen_pos();
    super::NativeDropPayload {
        paths,
        urls,
        screen_pos: position.or(cursor),
    }
}

pub(super) fn take_drop() -> super::NativeDropPayload {
    let paths = take_dropped_paths();
    let urls = take_dropped_urls();
    let position = take_dropped_screen_pos();
    let cursor = cursor_screen_pos();
    super::NativeDropPayload {
        paths,
        urls,
        screen_pos: position.or(cursor),
    }
}
