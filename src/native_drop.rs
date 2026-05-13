use std::path::PathBuf;

use eframe::egui;

fn append_http_urls_from_text(text: &str, urls: &mut Vec<String>) {
    let mut remainder = text;
    while let Some(start) = remainder
        .find("http://")
        .or_else(|| remainder.find("https://"))
    {
        let candidate = &remainder[start..];
        let end = candidate
            .find(|ch: char| {
                ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']')
            })
            .unwrap_or(candidate.len());
        append_http_url_candidate(&candidate[..end], urls);
        remainder = &candidate[end..];
    }
}

fn append_http_url_candidate(raw: &str, urls: &mut Vec<String>) {
    let trimmed = raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';'
        )
    });
    let unescaped = trimmed.replace("&amp;", "&");
    let Ok(parsed) = url::Url::parse(&unescaped) else {
        return;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return;
    }
    for (_, value) in parsed.query_pairs().filter(|(key, _)| {
        matches!(
            key.as_ref().to_ascii_lowercase().as_str(),
            "imgurl" | "mediaurl" | "imageurl" | "src"
        )
    }) {
        append_http_url_candidate(&value, urls);
    }
    let normalized = parsed.to_string();
    if !urls.contains(&normalized) {
        urls.push(normalized);
    }
}

fn best_drag_image_urls(urls: Vec<String>) -> Vec<String> {
    urls.into_iter()
        .max_by_key(|url| drag_image_url_score(url))
        .into_iter()
        .collect()
}

fn drag_image_url_score(url: &str) -> i32 {
    let Ok(parsed) = url::Url::parse(url) else {
        return 0;
    };
    let mut score = 0;
    let path = parsed.path().to_ascii_lowercase();
    if matches!(
        path.rsplit('.').next(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "bmp" | "tif" | "tiff")
    ) {
        score += 100;
    }
    if parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.as_ref().to_ascii_lowercase().as_str(),
            "imgurl" | "mediaurl" | "imageurl" | "src"
        )
    }) {
        score -= 25;
    }
    if parsed
        .host_str()
        .is_some_and(|host| host.contains("google.") || host.contains("bing."))
    {
        score -= 10;
    }
    score
}

#[derive(Default)]
struct NativeDropState {
    hovered_urls: Vec<String>,
    dropped_urls: Vec<String>,
    hovered_paths: Vec<PathBuf>,
    dropped_paths: Vec<PathBuf>,
    hovered_pos: Option<egui::Pos2>,
    dropped_pos: Option<egui::Pos2>,
}

impl NativeDropState {
    fn set_hovered(&mut self, payload: NativeDropPayload, pos: Option<egui::Pos2>) {
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

    fn set_dropped(&mut self, payload: NativeDropPayload, pos: Option<egui::Pos2>) {
        self.clear_hovered();
        self.dropped_urls.extend(payload.urls);
        self.dropped_paths.extend(payload.paths);
        self.dropped_pos = pos;
    }
}

#[derive(Default)]
struct NativeDropPayload {
    urls: Vec<String>,
    paths: Vec<PathBuf>,
}

impl NativeDropPayload {
    fn is_empty(&self) -> bool {
        self.urls.is_empty() && self.paths.is_empty()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::collections::HashSet;
    use std::ffi::{CStr, c_char};
    use std::sync::{Mutex, Once, OnceLock};

    use eframe::egui;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Method, NSObject, Sel};
    use objc2::{msg_send, msg_send_id, sel};
    use objc2_app_kit::{
        NSDragOperation, NSFilenamesPboardType, NSPasteboard, NSPasteboardTypeFileURL,
        NSPasteboardTypeHTML, NSPasteboardTypeString, NSPasteboardTypeURL,
    };
    use objc2_foundation::{NSPoint, NSRect};

    static INSTALL: Once = Once::new();
    static STATE: OnceLock<Mutex<super::NativeDropState>> = OnceLock::new();
    static ORIGINAL_DRAGGING_ENTERED: OnceLock<Imp> = OnceLock::new();
    static ORIGINAL_DRAGGING_UPDATED: OnceLock<Imp> = OnceLock::new();
    static ORIGINAL_PERFORM_DRAG_OPERATION: OnceLock<Imp> = OnceLock::new();
    static ORIGINAL_DRAGGING_EXITED: OnceLock<Imp> = OnceLock::new();

    type DragOperationMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> NSDragOperation;
    type DragBoolMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> bool;
    type DragVoidMethod = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSObject>);

    pub(super) fn install(_cc: &eframe::CreationContext<'_>) {
        INSTALL.call_once(|| {
            let Some(class) = AnyClass::get("WinitWindowDelegate") else {
                return;
            };

            unsafe {
                if let Some(method) = class.instance_method(sel!(draggingEntered:)) {
                    swizzle_drag_operation_method(
                        method,
                        dragging_entered_swizzled as DragOperationMethod,
                        &ORIGINAL_DRAGGING_ENTERED,
                    );
                }
                if let Some(method) = class.instance_method(sel!(draggingUpdated:)) {
                    swizzle_drag_operation_method(
                        method,
                        dragging_updated_swizzled as DragOperationMethod,
                        &ORIGINAL_DRAGGING_UPDATED,
                    );
                } else {
                    add_dragging_updated_method();
                }
                if let Some(method) = class.instance_method(sel!(performDragOperation:)) {
                    swizzle_bool_method(
                        method,
                        perform_drag_operation_swizzled as DragBoolMethod,
                        &ORIGINAL_PERFORM_DRAG_OPERATION,
                    );
                }
                if let Some(method) = class.instance_method(sel!(draggingExited:)) {
                    swizzle_void_method(
                        method,
                        dragging_exited_swizzled as DragVoidMethod,
                        &ORIGINAL_DRAGGING_EXITED,
                    );
                }
            }
        });
    }

    pub(super) fn hovered_urls() -> Vec<String> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_urls
            .clone()
    }

    pub(super) fn hovered_paths() -> Vec<std::path::PathBuf> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_paths
            .clone()
    }

    pub(super) fn hovered_screen_pos() -> Option<egui::Pos2> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_pos
    }

    pub(super) fn take_dropped_urls() -> Vec<String> {
        std::mem::take(
            &mut state()
                .lock()
                .expect("native drop state mutex poisoned")
                .dropped_urls,
        )
    }

    pub(super) fn take_dropped_paths() -> Vec<std::path::PathBuf> {
        std::mem::take(
            &mut state()
                .lock()
                .expect("native drop state mutex poisoned")
                .dropped_paths,
        )
    }

    pub(super) fn take_dropped_screen_pos() -> Option<egui::Pos2> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped_pos
            .take()
    }

    pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    fn state() -> &'static Mutex<super::NativeDropState> {
        STATE.get_or_init(|| Mutex::new(super::NativeDropState::default()))
    }

    unsafe fn swizzle_drag_operation_method(
        method: &Method,
        replacement: DragOperationMethod,
        original: &OnceLock<Imp>,
    ) {
        let previous = unsafe {
            method.set_implementation(std::mem::transmute::<DragOperationMethod, Imp>(replacement))
        };
        let _ = original.set(previous);
    }

    unsafe fn add_dragging_updated_method() {
        let class_name = CStr::from_bytes_with_nul(b"WinitWindowDelegate\0")
            .expect("static Objective-C class name");
        let class = unsafe { objc2::ffi::objc_getClass(class_name.as_ptr()) };
        if class.is_null() {
            return;
        }

        let types =
            CStr::from_bytes_with_nul(b"Q@:@\0").expect("static Objective-C method encoding");
        unsafe {
            let _ = objc2::ffi::class_addMethod(
                class as *mut _,
                sel!(draggingUpdated:).as_ptr(),
                std::mem::transmute::<DragOperationMethod, objc2::ffi::IMP>(
                    dragging_updated_added as DragOperationMethod,
                ),
                types.as_ptr() as *const c_char,
            );
        }
    }

    unsafe fn swizzle_bool_method(
        method: &Method,
        replacement: DragBoolMethod,
        original: &OnceLock<Imp>,
    ) {
        let previous = unsafe {
            method.set_implementation(std::mem::transmute::<DragBoolMethod, Imp>(replacement))
        };
        let _ = original.set(previous);
    }

    unsafe fn swizzle_void_method(
        method: &Method,
        replacement: DragVoidMethod,
        original: &OnceLock<Imp>,
    ) {
        let previous = unsafe {
            method.set_implementation(std::mem::transmute::<DragVoidMethod, Imp>(replacement))
        };
        let _ = original.set(previous);
    }

    unsafe extern "C" fn dragging_entered_swizzled(
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> NSDragOperation {
        let (payload, should_claim_drag) = extract_drag_payload(sender);
        let pos = dragging_screen_pos(sender);
        if !payload.is_empty() {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered(payload, pos);
            return NSDragOperation::Copy;
        }
        if should_claim_drag {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .clear_hovered();
            return NSDragOperation::Copy;
        }

        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .set_hovered_pos(pos);
        call_original_drag_operation(&ORIGINAL_DRAGGING_ENTERED, this, cmd, sender)
    }

    unsafe extern "C" fn dragging_updated_swizzled(
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> NSDragOperation {
        let (payload, should_claim_drag) = extract_drag_payload(sender);
        let pos = dragging_screen_pos(sender);
        if !payload.is_empty() {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered(payload, pos);
            return NSDragOperation::Copy;
        }
        if should_claim_drag {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered_pos(pos);
            return NSDragOperation::Copy;
        }

        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .set_hovered_pos(pos);
        call_original_drag_operation(&ORIGINAL_DRAGGING_UPDATED, this, cmd, sender)
    }

    unsafe extern "C" fn dragging_updated_added(
        _this: &AnyObject,
        _cmd: Sel,
        sender: &NSObject,
    ) -> NSDragOperation {
        let accepts_file_payload = has_file_payload(sender);
        let (payload, should_claim_drag) = extract_drag_payload(sender);
        let pos = dragging_screen_pos(sender);
        if !payload.is_empty() {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .set_hovered(payload, pos);
            return NSDragOperation::Copy;
        }

        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .set_hovered_pos(pos);
        if accepts_file_payload || should_claim_drag {
            NSDragOperation::Copy
        } else {
            NSDragOperation::None
        }
    }

    unsafe extern "C" fn perform_drag_operation_swizzled(
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> bool {
        let (payload, should_claim_drag) = extract_drag_payload(sender);
        let pos = dragging_screen_pos(sender);
        if !payload.is_empty() {
            let mut state = state().lock().expect("native drop state mutex poisoned");
            state.set_dropped(payload, pos);
            return true;
        }
        if should_claim_drag {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .clear_hovered();
            return true;
        }

        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped_pos = pos;
        call_original_drag_bool(&ORIGINAL_PERFORM_DRAG_OPERATION, this, cmd, sender)
    }

    unsafe extern "C" fn dragging_exited_swizzled(
        this: &AnyObject,
        cmd: Sel,
        sender: Option<&NSObject>,
    ) {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .clear_hovered();
        call_original_drag_void(&ORIGINAL_DRAGGING_EXITED, this, cmd, sender);
    }

    fn dragging_screen_pos(sender: &NSObject) -> Option<egui::Pos2> {
        let window_point: NSPoint = unsafe { msg_send![sender, draggingLocation] };
        let window: Option<Retained<NSObject>> =
            unsafe { msg_send_id![sender, draggingDestinationWindow] };
        let content_view: Option<Retained<NSObject>> = window
            .as_ref()
            .and_then(|window| unsafe { msg_send_id![&**window, contentView] });
        let bounds: NSRect = unsafe { msg_send![&**content_view.as_ref()?, bounds] };
        let view_point: NSPoint = unsafe {
            msg_send![
                &**content_view.as_ref()?,
                convertPoint: window_point,
                fromView: Option::<&NSObject>::None
            ]
        };
        let is_flipped: bool = unsafe { msg_send![&**content_view.as_ref()?, isFlipped] };
        Some(view_point_to_egui_pos(
            view_point.x,
            view_point.y,
            bounds.size.height,
            is_flipped,
        ))
    }

    fn view_point_to_egui_pos(x: f64, y: f64, bounds_height: f64, is_flipped: bool) -> egui::Pos2 {
        egui::pos2(
            x as f32,
            if is_flipped {
                y as f32
            } else {
                (bounds_height - y) as f32
            },
        )
    }

    fn call_original_drag_operation(
        original: &OnceLock<Imp>,
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> NSDragOperation {
        let imp = *original.get().expect("missing original drag operation IMP");
        let function = unsafe { std::mem::transmute::<Imp, DragOperationMethod>(imp) };
        unsafe { function(this, cmd, sender) }
    }

    fn call_original_drag_bool(
        original: &OnceLock<Imp>,
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> bool {
        let imp = *original.get().expect("missing original drag bool IMP");
        let function = unsafe { std::mem::transmute::<Imp, DragBoolMethod>(imp) };
        unsafe { function(this, cmd, sender) }
    }

    fn call_original_drag_void(
        original: &OnceLock<Imp>,
        this: &AnyObject,
        cmd: Sel,
        sender: Option<&NSObject>,
    ) {
        let imp = *original.get().expect("missing original drag void IMP");
        let function = unsafe { std::mem::transmute::<Imp, DragVoidMethod>(imp) };
        unsafe { function(this, cmd, sender) };
    }

    fn extract_drag_payload(sender: &NSObject) -> (super::NativeDropPayload, bool) {
        let pasteboard: Retained<NSPasteboard> =
            unsafe { msg_send_id![sender, draggingPasteboard] };
        let mut payload = super::NativeDropPayload::default();
        append_file_paths_from_pasteboard(&pasteboard, &mut payload.paths);
        if !payload.paths.is_empty() {
            return (payload, true);
        }

        let mut urls = Vec::new();
        let mut should_claim_drag = true;
        if let Some(items) = unsafe { pasteboard.pasteboardItems() } {
            for item in items {
                append_file_paths_from_item(&item, &mut payload.paths);
                should_claim_drag |= extract_drag_urls_from_item(&item, &mut urls);
            }
            if !payload.paths.is_empty() {
                return (payload, true);
            }
            if !urls.is_empty() {
                return (
                    super::NativeDropPayload {
                        urls: super::best_drag_image_urls(urls),
                        paths: Vec::new(),
                    },
                    true,
                );
            }
        }

        let Some(types) = (unsafe { pasteboard.types() }) else {
            return (super::NativeDropPayload::default(), should_claim_drag);
        };
        for pasteboard_type in types {
            should_claim_drag |= is_claimable_drag_type(&pasteboard_type);
            if !is_text_pasteboard_type(&pasteboard_type) {
                continue;
            }
            let Some(value) = (unsafe { pasteboard.stringForType(&pasteboard_type) }) else {
                continue;
            };
            super::append_http_urls_from_text(&value.to_string(), &mut urls);
        }

        (
            super::NativeDropPayload {
                urls: super::best_drag_image_urls(urls),
                paths: payload.paths,
            },
            should_claim_drag,
        )
    }

    fn has_file_payload(sender: &NSObject) -> bool {
        let pasteboard: Retained<NSPasteboard> =
            unsafe { msg_send_id![sender, draggingPasteboard] };
        has_file_payload_on_pasteboard(&pasteboard)
    }

    fn has_file_payload_on_pasteboard(pasteboard: &NSPasteboard) -> bool {
        pasteboard
            .propertyListForType(unsafe { NSFilenamesPboardType })
            .is_some()
    }

    fn append_file_paths_from_pasteboard(
        pasteboard: &NSPasteboard,
        paths: &mut Vec<std::path::PathBuf>,
    ) {
        let Some(plist) = pasteboard.propertyListForType(unsafe { NSFilenamesPboardType }) else {
            return;
        };
        let count: usize = unsafe { msg_send![&*plist, count] };
        for index in 0..count {
            let path: Option<Retained<objc2_foundation::NSString>> =
                unsafe { msg_send_id![&*plist, objectAtIndex: index] };
            if let Some(path) = path {
                let path = std::path::PathBuf::from(path.to_string());
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }

    fn append_file_paths_from_item(
        item: &objc2_app_kit::NSPasteboardItem,
        paths: &mut Vec<std::path::PathBuf>,
    ) {
        let Some(value) = (unsafe { item.stringForType(NSPasteboardTypeFileURL) }) else {
            return;
        };
        let Ok(url) = url::Url::parse(&value.to_string()) else {
            return;
        };
        let Ok(path) = url.to_file_path() else {
            return;
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    fn extract_drag_urls_from_item(
        item: &objc2_app_kit::NSPasteboardItem,
        urls: &mut Vec<String>,
    ) -> bool {
        let mut visited_types = HashSet::new();
        let mut should_claim_drag = false;
        let item_types = unsafe { item.types() }.into_iter().collect::<Vec<_>>();
        let preferred_type_names = unsafe {
            [
                NSPasteboardTypeURL.to_string(),
                NSPasteboardTypeHTML.to_string(),
                NSPasteboardTypeString.to_string(),
            ]
        };
        for pasteboard_type in item_types
            .iter()
            .filter(|pasteboard_type| preferred_type_names.contains(&pasteboard_type.to_string()))
        {
            should_claim_drag |=
                extract_drag_urls_from_item_type(item, pasteboard_type, urls, &mut visited_types);
        }

        for pasteboard_type in item_types {
            let key = pasteboard_type.to_string();
            if !visited_types.insert(key) {
                continue;
            }
            should_claim_drag |= is_claimable_drag_type(&pasteboard_type);
            if !is_text_pasteboard_type(&pasteboard_type) {
                continue;
            }
            let Some(value) = (unsafe { item.stringForType(&pasteboard_type) }) else {
                continue;
            };
            super::append_http_urls_from_text(&value.to_string(), urls);
        }
        should_claim_drag
    }

    fn extract_drag_urls_from_item_type(
        item: &objc2_app_kit::NSPasteboardItem,
        pasteboard_type: &objc2_app_kit::NSPasteboardType,
        urls: &mut Vec<String>,
        visited_types: &mut HashSet<String>,
    ) -> bool {
        let key = pasteboard_type.to_string();
        if !visited_types.insert(key) {
            return false;
        }
        let should_claim_drag = is_claimable_drag_type(pasteboard_type);
        if !is_text_pasteboard_type(pasteboard_type) {
            return should_claim_drag;
        }
        let Some(value) = (unsafe { item.stringForType(pasteboard_type) }) else {
            return should_claim_drag;
        };
        super::append_http_urls_from_text(&value.to_string(), urls);
        should_claim_drag
    }

    fn is_claimable_drag_type(pasteboard_type: &objc2_app_kit::NSPasteboardType) -> bool {
        let name = pasteboard_type.to_string();
        if name.starts_with("Apple ") {
            return false;
        }
        let lower = name.to_ascii_lowercase();
        lower.contains("url")
            || lower.contains("html")
            || lower.contains("text")
            || lower.contains("string")
            || lower.contains("image")
            || lower.contains("png")
            || lower.contains("jpeg")
            || lower.contains("tiff")
            || lower.contains("gif")
            || lower.contains("webp")
    }

    fn is_text_pasteboard_type(pasteboard_type: &objc2_app_kit::NSPasteboardType) -> bool {
        let name = pasteboard_type.to_string();
        if name.starts_with("Apple ") {
            return false;
        }
        let lower = name.to_ascii_lowercase();
        lower.contains("url")
            || lower.contains("html")
            || lower.contains("text")
            || lower.contains("string")
    }

    #[cfg(test)]
    mod tests {
        use super::view_point_to_egui_pos;

        #[test]
        fn flipped_winit_view_points_are_already_top_left_origin() {
            let pos = view_point_to_egui_pos(240.0, 520.0, 900.0, true);

            assert_eq!(pos.x, 240.0);
            assert_eq!(pos.y, 520.0);
        }

        #[test]
        fn unflipped_appkit_points_are_converted_to_top_left_origin() {
            let pos = view_point_to_egui_pos(240.0, 520.0, 900.0, false);

            assert_eq!(pos.x, 240.0);
            assert_eq!(pos.y, 380.0);
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};

    use eframe::egui;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{
        DV_E_FORMATETC, E_NOINTERFACE, HGLOBAL, HWND, POINT, POINTL, S_OK,
    };
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::{
        CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY, DROPEFFECT_NONE, OleInitialize,
        RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
    };
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows_sys::core::{GUID, HRESULT};

    type IUnknown = c_void;

    static STATE: OnceLock<Mutex<super::NativeDropState>> = OnceLock::new();
    static REGISTRATION: OnceLock<Mutex<Option<DropTargetRegistration>>> = OnceLock::new();

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

        let registration = REGISTRATION.get_or_init(|| Mutex::new(None));
        let mut registration = registration
            .lock()
            .expect("native drop registration mutex poisoned");
        if registration.is_some() {
            return;
        }

        unsafe {
            let _ = OleInitialize(ptr::null());
            let _ = RevokeDragDrop(hwnd);
        }
        let target = DropTargetRegistration::new(hwnd);
        let result = unsafe { RegisterDragDrop(hwnd, target.data as *mut c_void) };
        if result >= 0 {
            *registration = Some(target);
        }
    }

    pub(super) fn hovered_urls() -> Vec<String> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_urls
            .clone()
    }

    pub(super) fn hovered_paths() -> Vec<std::path::PathBuf> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_paths
            .clone()
    }

    pub(super) fn hovered_screen_pos() -> Option<egui::Pos2> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .hovered_pos
    }

    pub(super) fn take_dropped_urls() -> Vec<String> {
        std::mem::take(
            &mut state()
                .lock()
                .expect("native drop state mutex poisoned")
                .dropped_urls,
        )
    }

    pub(super) fn take_dropped_paths() -> Vec<std::path::PathBuf> {
        std::mem::take(
            &mut state()
                .lock()
                .expect("native drop state mutex poisoned")
                .dropped_paths,
        )
    }

    pub(super) fn take_dropped_screen_pos() -> Option<egui::Pos2> {
        state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped_pos
            .take()
    }

    fn state() -> &'static Mutex<super::NativeDropState> {
        STATE.get_or_init(|| Mutex::new(super::NativeDropState::default()))
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
            }
        }
    }

    impl Drop for DropTargetRegistration {
        fn drop(&mut self) {
            unsafe {
                let hwnd = (*self.data).hwnd;
                let _ = RevokeDragDrop(hwnd);
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
            let pos = unsafe { client_point(data.hwnd, point) };
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
            if data.hovered_is_valid {
                state()
                    .lock()
                    .expect("native drop state mutex poisoned")
                    .set_hovered_pos(unsafe { client_point(data.hwnd, point) });
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
            if !payload.is_empty() {
                state()
                    .lock()
                    .expect("native drop state mutex poisoned")
                    .set_dropped(payload, unsafe { client_point(data.hwnd, point) });
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

    unsafe fn client_point(hwnd: HWND, point: *const POINTL) -> Option<egui::Pos2> {
        if point.is_null() {
            return None;
        }
        let point = unsafe { &*point };
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
                super::append_http_urls_from_text(&text, &mut payload.urls);
            }
        }
        payload.urls = super::best_drag_image_urls(payload.urls);
        payload
    }

    unsafe fn append_paths(data_object: *const c_void, paths: &mut Vec<std::path::PathBuf>) {
        let _ = unsafe {
            with_hglobal_data(data_object, CF_HDROP, |hglobal, _pointer, _size| {
                unsafe { append_hdrop_paths(hglobal as HDROP, paths) };
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
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use eframe::egui;

    pub(super) fn install(_cc: &eframe::CreationContext<'_>) {}

    pub(super) fn hovered_urls() -> Vec<String> {
        Vec::new()
    }

    pub(super) fn hovered_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    pub(super) fn hovered_screen_pos() -> Option<egui::Pos2> {
        None
    }

    pub(super) fn take_dropped_urls() -> Vec<String> {
        Vec::new()
    }

    pub(super) fn take_dropped_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    pub(super) fn take_dropped_screen_pos() -> Option<egui::Pos2> {
        None
    }

    pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }
}

pub fn install_url_drop_bridge(cc: &eframe::CreationContext<'_>) {
    platform::install(cc);
}

pub fn hovered_url_strings() -> Vec<String> {
    platform::hovered_urls()
}

pub fn hovered_paths() -> Vec<PathBuf> {
    platform::hovered_paths()
}

pub fn hovered_screen_pos() -> Option<egui::Pos2> {
    platform::hovered_screen_pos()
}

pub fn take_dropped_url_strings() -> Vec<String> {
    platform::take_dropped_urls()
}

pub fn take_dropped_paths() -> Vec<PathBuf> {
    platform::take_dropped_paths()
}

pub fn take_dropped_screen_pos() -> Option<egui::Pos2> {
    platform::take_dropped_screen_pos()
}

pub fn clipboard_paths() -> Vec<PathBuf> {
    platform::clipboard_paths()
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_plain_text_http_urls() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "https://example.com/a.jpg\nhttps://example.com/b.png",
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://example.com/a.jpg".to_owned(),
                "https://example.com/b.png".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_http_urls_embedded_in_html() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            r#"<img src="https://images.example.com/photo.webp"><a href="https://example.com/page">"#,
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://images.example.com/photo.webp".to_owned(),
                "https://example.com/page".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_nested_image_urls_from_wrapper_urls() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&amp;imgrefurl=https%3A%2F%2Fexample.com%2Fpage",
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/photo.jpg".to_owned(),
                "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&imgrefurl=https%3A%2F%2Fexample.com%2Fpage".to_owned()
            ]
        );
    }

    #[test]
    fn drag_url_selection_prefers_actual_image_over_page_url() {
        let urls = super::best_drag_image_urls(vec![
            "https://example.com/page".to_owned(),
            "https://cdn.example.com/photo.webp".to_owned(),
        ]);
        assert_eq!(urls, vec!["https://cdn.example.com/photo.webp".to_owned()]);
    }

    #[test]
    fn ignores_non_http_urls() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "file:///tmp/test.png ftp://example.com/photo.png https://example.com/ok.jpg",
            &mut urls,
        );
        assert_eq!(urls, vec!["https://example.com/ok.jpg".to_owned()]);
    }
}
