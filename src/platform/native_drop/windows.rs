use std::cell::RefCell;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use crate::sources::clipboard::windows::{append_hdrop_paths, registered_clipboard_format};
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{E_POINTER, HWND, POINTL};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, OleInitialize,
    OleUninitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{Ref, Result, implement};
use windows_sys::Win32::Foundation::{HGLOBAL, POINT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows_sys::Win32::UI::Shell::HDROP;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::payload::NativeDropState;

static STATE: OnceLock<Mutex<NativeDropState>> = OnceLock::new();
static WINDOW_HWND: OnceLock<isize> = OnceLock::new();
static EGUI_CONTEXT: OnceLock<egui::Context> = OnceLock::new();
thread_local! {
    // Registration and OLE cleanup belong to the window's apartment thread.
    static REGISTRATION: RefCell<Option<DropTargetRegistration>> = const { RefCell::new(None) };
}

pub(super) fn install(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let _ = WINDOW_HWND.set(handle.hwnd.get());
    let _ = EGUI_CONTEXT.set(cc.egui_ctx.clone());
    REGISTRATION.with_borrow_mut(|registration| {
        if registration.is_some() || unsafe { OleInitialize(None) }.is_err() {
            return;
        }
        let mut target = DropTargetRegistration::new(hwnd);
        target.initialized_ole = true;
        // configure_viewport keeps winit's target off, so this is the window's
        // only drop target for Explorer files and browser URLs alike. A failed
        // bridge releases only itself, never an existing window target.
        if target.register().is_ok() {
            *registration = Some(target);
        }
    });
}

// OLE delivers drag callbacks without window input events, so egui would not
// otherwise draw the hover preview or consume the drop until the next input.
fn request_repaint() {
    if let Some(ctx) = EGUI_CONTEXT.get() {
        ctx.request_repaint();
    }
}

fn state() -> &'static Mutex<NativeDropState> {
    STATE.get_or_init(|| Mutex::new(NativeDropState::default()))
}

pub(super) fn hovered() -> super::NativeDropPayload {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .hovered
        .clone()
}

pub(super) fn take_drop() -> super::NativeDropPayload {
    std::mem::take(
        &mut state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped,
    )
}

// Winit file events have no coordinates, so previews and placement query the OS cursor.
pub(super) fn cursor_screen_pos() -> Option<egui::Pos2> {
    let hwnd = HWND(*WINDOW_HWND.get()? as *mut _);
    let mut cursor = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0 {
        return None;
    }
    client_point(
        hwnd,
        POINTL {
            x: cursor.x,
            y: cursor.y,
        },
    )
}

fn client_point(hwnd: HWND, point: POINTL) -> Option<egui::Pos2> {
    let mut client = POINT {
        x: point.x,
        y: point.y,
    };
    if unsafe { ScreenToClient(hwnd.0, &mut client) } == 0 {
        return None;
    }
    // ScreenToClient returns physical client pixels. Canvas and egui use points,
    // including both monitor scale and the current egui zoom factor.
    let scale = EGUI_CONTEXT
        .get()
        .map_or(1.0, egui::Context::pixels_per_point);
    Some(egui::pos2(client.x as f32 / scale, client.y as f32 / scale))
}

struct DropTargetRegistration {
    target: IDropTarget,
    hwnd: HWND,
    registered: bool,
    initialized_ole: bool,
}

impl DropTargetRegistration {
    fn new(hwnd: HWND) -> Self {
        Self {
            target: DropTarget { hwnd }.into(),
            hwnd,
            registered: false,
            initialized_ole: false,
        }
    }

    fn register(&mut self) -> Result<()> {
        unsafe { RegisterDragDrop(self.hwnd, &self.target) }?;
        self.registered = true;
        Ok(())
    }
}

impl Drop for DropTargetRegistration {
    fn drop(&mut self) {
        unsafe {
            if self.registered {
                let _ = RevokeDragDrop(self.hwnd);
            }
            if self.initialized_ole {
                OleUninitialize();
            }
        }
    }
}

// Generated COM owns layout, IUnknown lifetime, vtable order and the by-value
// POINTL ABI. The implementation trait receives a reference after that boundary.
#[implement(IDropTarget)]
struct DropTarget {
    hwnd: HWND,
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        object: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let effect = unsafe { effect.as_mut() }.ok_or(E_POINTER)?;
        let payload = object
            .as_ref()
            .map(|object| unsafe { extract_drop_payload(object) })
            .unwrap_or_default();
        *effect = if payload.is_empty() {
            DROPEFFECT_NONE
        } else {
            DROPEFFECT_COPY
        };
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .set_hovered(payload, client_point(self.hwnd, *point));
        request_repaint();
        Ok(())
    }

    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let effect = unsafe { effect.as_mut() }.ok_or(E_POINTER)?;
        let mut state = state().lock().expect("native drop state mutex poisoned");
        *effect = if state.hovered.is_empty() {
            DROPEFFECT_NONE
        } else {
            state.set_hovered_pos(client_point(self.hwnd, *point));
            DROPEFFECT_COPY
        };
        request_repaint();
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .clear_hovered();
        request_repaint();
        Ok(())
    }

    fn Drop(
        &self,
        object: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let effect = unsafe { effect.as_mut() }.ok_or(E_POINTER)?;
        let payload = object
            .as_ref()
            .map(|object| unsafe { extract_drop_payload(object) })
            .unwrap_or_default();
        let mut state = state().lock().expect("native drop state mutex poisoned");
        state.clear_hovered();
        *effect = if payload.is_empty() {
            DROPEFFECT_NONE
        } else {
            state.set_dropped(payload, client_point(self.hwnd, *point));
            DROPEFFECT_COPY
        };
        drop(state);
        request_repaint();
        Ok(())
    }
}

unsafe fn extract_drop_payload(data_object: &IDataObject) -> super::NativeDropPayload {
    let mut payload = super::NativeDropPayload::default();
    unsafe {
        let _ = with_hglobal_data(data_object, CF_HDROP, |hglobal, _, _| {
            append_hdrop_paths(hglobal as HDROP, &mut payload.paths);
        });
        // Files win over URLs, as on macOS, so a browser that also offers a
        // downloaded copy of the image does not place it twice.
        if !payload.paths.is_empty() {
            return payload;
        }
        let mut urls = super::payload::DragUrls::default();
        for (text, kind) in drop_text_values(data_object) {
            match kind {
                DropText::Url => urls.append_url(&text),
                DropText::Prose => urls.append_text(&text),
            }
        }
        payload.urls = urls.best();
    }
    payload
}

#[derive(Clone, Copy)]
enum DropText {
    // Holds one complete URL, which may contain delimiters that end a URL in prose.
    Url,
    Prose,
}

unsafe fn drop_text_values(data_object: &IDataObject) -> Vec<(String, DropText)> {
    let mut values = Vec::new();
    for (format, utf16, kind) in [
        (
            registered_clipboard_format("UniformResourceLocatorW"),
            true,
            DropText::Url,
        ),
        (
            registered_clipboard_format("UniformResourceLocator"),
            false,
            DropText::Url,
        ),
        (CF_UNICODETEXT, true, DropText::Prose),
        (
            registered_clipboard_format("text/html"),
            false,
            DropText::Prose,
        ),
        (
            registered_clipboard_format("HTML Format"),
            false,
            DropText::Prose,
        ),
    ] {
        if format == 0 {
            continue;
        }
        let value = unsafe {
            with_hglobal_data(data_object, format, |_hglobal, pointer, size| {
                if utf16 {
                    utf16_hglobal_text(pointer, size)
                } else {
                    bytes_hglobal_text(pointer, size)
                }
            })
        }
        .flatten();
        if let Some(value) = value
            && !value.is_empty()
        {
            values.push((value, kind));
        }
    }
    values
}

unsafe fn with_hglobal_data<T>(
    data_object: &IDataObject,
    format: u16,
    read: impl FnOnce(HGLOBAL, *const u8, usize) -> T,
) -> Option<T> {
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    let format_etc = FORMATETC {
        cfFormat: format,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut medium = unsafe { data_object.GetData(&format_etc) }.ok()?;
    if medium.tymed != TYMED_HGLOBAL.0 as u32 {
        unsafe { ReleaseStgMedium(&mut medium) };
        return None;
    }
    let hglobal = unsafe { medium.u.hGlobal }.0;
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

fn utf16_hglobal_text(pointer: *const u8, size: usize) -> Option<String> {
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) };
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_ne_bytes(*chunk))
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

#[cfg(test)]
mod tests;
