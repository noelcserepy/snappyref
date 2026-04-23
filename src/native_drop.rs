#[cfg(target_os = "macos")]
mod platform {
    use std::sync::{Mutex, Once, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Method, NSObject, Sel};
    use objc2::{msg_send_id, sel};
    use objc2_app_kit::{
        NSDragOperation, NSFilenamesPboardType, NSPasteboard, NSPasteboardTypeString,
        NSPasteboardTypeURL, NSURLPboardType,
    };

    #[derive(Default)]
    struct UrlDropState {
        hovered_urls: Vec<String>,
        dropped_urls: Vec<String>,
    }

    static INSTALL: Once = Once::new();
    static STATE: OnceLock<Mutex<UrlDropState>> = OnceLock::new();
    static ORIGINAL_DRAGGING_ENTERED: OnceLock<Imp> = OnceLock::new();
    static ORIGINAL_PERFORM_DRAG_OPERATION: OnceLock<Imp> = OnceLock::new();
    static ORIGINAL_DRAGGING_EXITED: OnceLock<Imp> = OnceLock::new();

    type DragOperationMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> NSDragOperation;
    type DragBoolMethod = unsafe extern "C" fn(&AnyObject, Sel, &NSObject) -> bool;
    type DragVoidMethod = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSObject>);

    pub(super) fn install() {
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

    pub(super) fn take_dropped_urls() -> Vec<String> {
        std::mem::take(
            &mut state()
                .lock()
                .expect("native drop state mutex poisoned")
                .dropped_urls,
        )
    }

    fn state() -> &'static Mutex<UrlDropState> {
        STATE.get_or_init(|| Mutex::new(UrlDropState::default()))
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
        let urls = extract_drag_urls(sender);
        if !urls.is_empty() {
            state()
                .lock()
                .expect("native drop state mutex poisoned")
                .hovered_urls = urls;
            return NSDragOperation::Copy;
        }

        call_original_drag_operation(&ORIGINAL_DRAGGING_ENTERED, this, cmd, sender)
    }

    unsafe extern "C" fn perform_drag_operation_swizzled(
        this: &AnyObject,
        cmd: Sel,
        sender: &NSObject,
    ) -> bool {
        let urls = extract_drag_urls(sender);
        if !urls.is_empty() {
            let mut state = state().lock().expect("native drop state mutex poisoned");
            state.hovered_urls.clear();
            state.dropped_urls.extend(urls);
            return true;
        }

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
            .hovered_urls
            .clear();
        call_original_drag_void(&ORIGINAL_DRAGGING_EXITED, this, cmd, sender);
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

    fn extract_drag_urls(sender: &NSObject) -> Vec<String> {
        let pasteboard: Retained<NSPasteboard> =
            unsafe { msg_send_id![sender, draggingPasteboard] };
        if pasteboard
            .propertyListForType(unsafe { NSFilenamesPboardType })
            .is_some()
        {
            return Vec::new();
        }

        let mut urls = Vec::new();
        for pasteboard_type in
            unsafe { [NSPasteboardTypeURL, NSURLPboardType, NSPasteboardTypeString] }
        {
            let Some(value) = (unsafe { pasteboard.stringForType(pasteboard_type) }) else {
                continue;
            };
            for line in value.to_string().lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(parsed) = url::Url::parse(trimmed) else {
                    continue;
                };
                if parsed.scheme() == "http" || parsed.scheme() == "https" {
                    let normalized = parsed.to_string();
                    if !urls.contains(&normalized) {
                        urls.push(normalized);
                    }
                }
            }
        }
        urls
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub(super) fn install() {}

    pub(super) fn hovered_urls() -> Vec<String> {
        Vec::new()
    }

    pub(super) fn take_dropped_urls() -> Vec<String> {
        Vec::new()
    }
}

pub fn install_url_drop_bridge() {
    platform::install();
}

pub fn hovered_url_strings() -> Vec<String> {
    platform::hovered_urls()
}

pub fn take_dropped_url_strings() -> Vec<String> {
    platform::take_dropped_urls()
}
