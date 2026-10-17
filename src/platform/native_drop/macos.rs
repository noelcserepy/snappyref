use std::ffi::c_char;
use std::sync::{Mutex, Once, OnceLock};

use crate::sources::clipboard::macos::{
    append_file_paths_from_item, append_file_paths_from_pasteboard,
};
use eframe::egui;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Method, NSObject, Sel};
use objc2::{msg_send, msg_send_id, sel};
use objc2_app_kit::{
    NSDragOperation, NSFilenamesPboardType, NSPasteboard, NSPasteboardTypeFileURL,
    NSPasteboardTypeHTML, NSPasteboardTypeString, NSPasteboardTypeURL,
};
use objc2_foundation::{NSArray, NSCopying, NSPoint, NSRect, NSString};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

static INSTALL: Once = Once::new();
static EGUI_CONTEXT: OnceLock<egui::Context> = OnceLock::new();
static STATE: OnceLock<Mutex<super::payload::NativeDropState>> = OnceLock::new();
static ORIGINAL_DRAGGING_ENTERED: OnceLock<Imp> = OnceLock::new();
static ORIGINAL_DRAGGING_UPDATED: OnceLock<Imp> = OnceLock::new();
static ORIGINAL_PERFORM_DRAG_OPERATION: OnceLock<Imp> = OnceLock::new();
static ORIGINAL_DRAGGING_EXITED: OnceLock<Imp> = OnceLock::new();

type DragOperationMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> NSDragOperation;
type DragBoolMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> bool;
type DragVoidMethod = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSObject>);

// Winit initially registers only filename drops. This list is also used by
// the Cocoa negotiation probe so browser-only payloads exercise that boundary.
fn registered_drag_types() -> Retained<NSArray<NSString>> {
    unsafe {
        NSArray::from_id_slice(&[
            NSFilenamesPboardType.copy(),
            NSPasteboardTypeFileURL.copy(),
            NSPasteboardTypeURL.copy(),
            NSPasteboardTypeHTML.copy(),
            NSPasteboardTypeString.copy(),
        ])
    }
}

#[cfg(test)]
pub(crate) fn browser_drop_probe(url: &str) -> super::NativeDropPayload {
    let pasteboard = unsafe { NSPasteboard::pasteboardWithUniqueName() };
    unsafe {
        pasteboard.clearContents();
        pasteboard.setString_forType(&NSString::from_str(url), NSPasteboardTypeURL);
    }
    let accepted = unsafe { pasteboard.availableTypeFromArray(&registered_drag_types()) };
    let payload = if accepted.is_some() {
        extract_pasteboard_payload(&pasteboard).0
    } else {
        super::NativeDropPayload::default()
    };
    unsafe { pasteboard.clearContents() };
    payload
}

pub(super) fn install(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: eframe keeps the content view and its window alive for this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<AnyObject>() };
    let window: Option<Retained<NSObject>> = unsafe { msg_send_id![view, window] };
    let Some(window) = window else {
        return;
    };
    unsafe {
        let _: () = msg_send![&*window, registerForDraggedTypes: &*registered_drag_types()];
    }
    let _ = EGUI_CONTEXT.set(cc.egui_ctx.clone());
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

pub(super) fn hovered() -> super::NativeDropPayload {
    state()
        .lock()
        .expect("native drop state mutex poisoned")
        .hovered
        .clone()
}

pub(super) fn cursor_screen_pos() -> Option<egui::Pos2> {
    let event_class = AnyClass::get("NSEvent")?;
    let app_class = AnyClass::get("NSApplication")?;
    let screen_point: NSPoint = unsafe { msg_send![event_class, mouseLocation] };
    let app: *mut NSObject = unsafe { msg_send![app_class, sharedApplication] };
    let app = unsafe { app.as_ref()? };
    let mut window: Option<Retained<NSObject>> = unsafe { msg_send_id![app, keyWindow] };
    if window.is_none() {
        window = unsafe { msg_send_id![app, mainWindow] };
    }
    let window = window?;
    let window_point: NSPoint =
        unsafe { msg_send![&*window, convertPointFromScreen: screen_point] };
    let content_view: Option<Retained<NSObject>> = unsafe { msg_send_id![&*window, contentView] };
    let content_view = content_view?;
    let bounds: NSRect = unsafe { msg_send![&*content_view, bounds] };
    let view_point: NSPoint = unsafe {
        msg_send![
            &*content_view,
            convertPoint: window_point,
            fromView: Option::<&NSObject>::None
        ]
    };
    let is_flipped: bool = unsafe { msg_send![&*content_view, isFlipped] };
    Some(view_point_to_egui_pos(
        view_point.x,
        view_point.y,
        bounds.size.height,
        is_flipped,
    ))
}

pub(super) fn take_drop() -> super::NativeDropPayload {
    std::mem::take(
        &mut state()
            .lock()
            .expect("native drop state mutex poisoned")
            .dropped,
    )
}

fn request_repaint() {
    if let Some(ctx) = EGUI_CONTEXT.get() {
        ctx.request_repaint();
    }
}

fn state() -> &'static Mutex<super::payload::NativeDropState> {
    STATE.get_or_init(|| Mutex::new(super::payload::NativeDropState::default()))
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
    let class_name = c"WinitWindowDelegate";
    let class = unsafe { objc2::ffi::objc_getClass(class_name.as_ptr()) };
    if class.is_null() {
        return;
    }

    let types = c"Q@:@";
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
    request_repaint();
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
    request_repaint();
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
    request_repaint();
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
    request_repaint();
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
        .dropped
        .screen_pos = pos;
    call_original_drag_bool(&ORIGINAL_PERFORM_DRAG_OPERATION, this, cmd, sender)
}

unsafe extern "C" fn dragging_exited_swizzled(
    this: &AnyObject,
    cmd: Sel,
    sender: Option<&NSObject>,
) {
    request_repaint();
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
    let pasteboard: Retained<NSPasteboard> = unsafe { msg_send_id![sender, draggingPasteboard] };
    extract_pasteboard_payload(&pasteboard)
}

fn extract_pasteboard_payload(pasteboard: &NSPasteboard) -> (super::NativeDropPayload, bool) {
    let mut payload = super::NativeDropPayload::default();
    append_file_paths_from_pasteboard(pasteboard, &mut payload.paths);
    if !payload.paths.is_empty() {
        return (payload, true);
    }

    let mut urls = super::payload::DragUrls::default();
    let mut should_claim_drag = true;
    if let Some(items) = unsafe { pasteboard.pasteboardItems() } {
        for item in items {
            append_file_paths_from_item(&item, &mut payload.paths);
            should_claim_drag |= extract_drag_urls_from_item(&item, &mut urls);
        }
        if !payload.paths.is_empty() {
            return (payload, true);
        }
        let best = urls.best();
        if !best.is_empty() {
            return (
                super::NativeDropPayload {
                    urls: best,
                    paths: Vec::new(),
                    screen_pos: None,
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
        append_drag_urls(&pasteboard_type, &value.to_string(), &mut urls);
    }

    (
        super::NativeDropPayload {
            urls: urls.best(),
            paths: payload.paths,
            screen_pos: None,
        },
        should_claim_drag,
    )
}

fn has_file_payload(sender: &NSObject) -> bool {
    let pasteboard: Retained<NSPasteboard> = unsafe { msg_send_id![sender, draggingPasteboard] };
    has_file_payload_on_pasteboard(&pasteboard)
}

fn has_file_payload_on_pasteboard(pasteboard: &NSPasteboard) -> bool {
    pasteboard
        .propertyListForType(unsafe { NSFilenamesPboardType })
        .is_some()
}

fn extract_drag_urls_from_item(
    item: &objc2_app_kit::NSPasteboardItem,
    urls: &mut super::payload::DragUrls,
) -> bool {
    let mut should_claim_drag = false;
    for pasteboard_type in unsafe { item.types() } {
        should_claim_drag |= is_claimable_drag_type(&pasteboard_type);
        if !is_text_pasteboard_type(&pasteboard_type) {
            continue;
        }
        let Some(value) = (unsafe { item.stringForType(&pasteboard_type) }) else {
            continue;
        };
        append_drag_urls(&pasteboard_type, &value.to_string(), urls);
    }
    should_claim_drag
}

fn append_drag_urls(
    pasteboard_type: &objc2_app_kit::NSPasteboardType,
    value: &str,
    urls: &mut super::payload::DragUrls,
) {
    if pasteboard_type.to_string() == unsafe { NSPasteboardTypeURL }.to_string() {
        urls.append_url(value);
    } else {
        urls.append_text(value);
    }
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
    fn browser_payloads_and_finder_file_urls_negotiate_and_extract() {
        use objc2_app_kit::{
            NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeHTML, NSPasteboardTypeString,
            NSPasteboardTypeURL,
        };
        use objc2_foundation::NSString;
        let pasteboard = unsafe { NSPasteboard::pasteboardWithUniqueName() };
        for (kind, text) in unsafe {
            [
                (
                    NSPasteboardTypeURL,
                    "https://cdn.example.com/image_(1)?width=800",
                ),
                (
                    NSPasteboardTypeHTML,
                    r#"<a href="https://example.com/page"><img src="https://cdn.example.com/image.webp?width=800"></a>"#,
                ),
                (
                    NSPasteboardTypeString,
                    "https://cdn.example.com/image.webp?width=800",
                ),
                (NSPasteboardTypeFileURL, "file:///tmp/reference.png"),
            ]
        } {
            unsafe {
                pasteboard.clearContents();
                assert!(pasteboard.setString_forType(&NSString::from_str(text), kind));
                pasteboard
                    .availableTypeFromArray(&super::registered_drag_types())
                    .expect("offered payload must negotiate with the destination");
            }
            let payload = super::extract_pasteboard_payload(&pasteboard).0;
            if kind == unsafe { NSPasteboardTypeFileURL } {
                assert_eq!(
                    payload.paths,
                    [std::path::PathBuf::from("/tmp/reference.png")]
                );
            } else {
                let expected = if kind == unsafe { NSPasteboardTypeURL } {
                    "https://cdn.example.com/image_(1)?width=800"
                } else {
                    "https://cdn.example.com/image.webp?width=800"
                };
                assert_eq!(payload.urls, [expected]);
            }
        }
        unsafe { pasteboard.clearContents() };
    }

    #[test]
    fn linked_image_drag_selects_the_img_source_over_the_link() {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeURL};
        use objc2_foundation::NSString;
        // A linked image: the URL type names the link, the HTML the image.
        let image = "https://images.example.com/photo_(1)?w=800";
        let pasteboard = unsafe { NSPasteboard::pasteboardWithUniqueName() };
        unsafe {
            pasteboard.clearContents();
            let html = format!(r#"<img src="{image}">"#);
            assert!(pasteboard.setString_forType(&NSString::from_str(&html), NSPasteboardTypeHTML));
            assert!(pasteboard.setString_forType(
                &NSString::from_str("https://example.com/gallery"),
                NSPasteboardTypeURL
            ));
        }
        assert_eq!(
            super::extract_pasteboard_payload(&pasteboard).0.urls,
            [image]
        );
        unsafe { pasteboard.clearContents() };
    }

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
