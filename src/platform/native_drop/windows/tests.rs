use super::{DropTargetRegistration, client_point, hovered, state, take_drop};
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::Mutex;
use windows::Win32::Foundation::{
    DRAGDROP_E_ALREADYREGISTERED, DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, HWND, POINTL,
};
use windows::Win32::System::Com::{
    FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
    STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    DROPEFFECT_COPY, DROPEFFECT_NONE, OleInitialize, OleUninitialize,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{BOOL, HRESULT, Ref, implement};
use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW,
};

#[implement(IDataObject)]
struct TestPayload(Vec<(u16, Vec<u8>)>);

#[allow(non_snake_case)]
impl IDataObject_Impl for TestPayload_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let format = unsafe { &*format };
        let bytes = self
            .0
            .iter()
            .find(|(id, _)| *id == format.cfFormat)
            .filter(|_| format.tymed & TYMED_HGLOBAL.0 as u32 != 0)
            .map(|(_, bytes)| bytes)
            .ok_or_else(|| windows::core::Error::from_hresult(DV_E_FORMATETC))?;
        unsafe {
            let allocation = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            assert!(!allocation.is_null());
            let pointer = GlobalLock(allocation);
            assert!(!pointer.is_null());
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            GlobalUnlock(allocation);
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: HGLOBAL(allocation),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }
    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn QueryGetData(&self, _: *const FORMATETC) -> HRESULT {
        E_NOTIMPL
    }
    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
        E_NOTIMPL
    }
    fn SetData(
        &self,
        _: *const FORMATETC,
        _: *const STGMEDIUM,
        _: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumFormatEtc(&self, _: u32) -> windows::core::Result<IEnumFORMATETC> {
        Err(E_NOTIMPL.into())
    }
    fn DAdvise(
        &self,
        _: *const FORMATETC,
        _: u32,
        _: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(E_NOTIMPL.into())
    }
    fn DUnadvise(&self, _: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

// Drop state is process-wide, and libtest runs these tests in parallel.
static DROP_STATE: Mutex<()> = Mutex::new(());

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn wide_bytes(value: &str) -> Vec<u8> {
    wide(value).into_iter().flat_map(u16::to_ne_bytes).collect()
}

struct TestWindow(HWND);
impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0.0);
            OleUninitialize();
        }
    }
}

#[test]
fn generated_drop_target_preserves_payload_coordinates_and_registration() {
    verify_native_drop(false);
}

#[test]
#[ignore = "requires the interactive Windows desktop"]
fn native_desktop_drop_cursor_fallback() {
    verify_native_drop(true);
}

fn verify_native_drop(check_desktop_cursor: bool) {
    let _drop_state = DROP_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe {
        OleInitialize(None).unwrap();
    }
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("SnappyRef drop test").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            140,
            180,
            320,
            240,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    let window = TestWindow(HWND(hwnd));
    *state().lock().unwrap() = Default::default();

    let path = PathBuf::from(r"C:\SnappyRef\drop-test.png");
    // DROPFILES is five 32-bit fields. The final field selects UTF-16 paths.
    let mut files: Vec<u8> = [20u32, 0, 0, 0, 1]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    files.extend(wide_bytes(path.to_str().unwrap()));
    files.extend([0, 0]);
    let html_format = unsafe { RegisterClipboardFormatW(wide("text/html").as_ptr()) as u16 };
    let payload: IDataObject = TestPayload(vec![
        (15, files),
        (13, wide_bytes("https://example.com/drop.png")),
        (
            html_format,
            b"<img src=\"https://example.com/drop.png\">\0".to_vec(),
        ),
    ])
    .into();

    let mut registration = DropTargetRegistration::new(window.0);
    registration.register().unwrap();
    let mut rejected = DropTargetRegistration::new(window.0);
    assert_eq!(
        rejected.register().unwrap_err().code(),
        DRAGDROP_E_ALREADYREGISTERED
    );
    drop(rejected);
    // A failed registration must not revoke the incumbent when destroyed.
    let mut still_rejected = DropTargetRegistration::new(window.0);
    assert_eq!(
        still_rejected.register().unwrap_err().code(),
        DRAGDROP_E_ALREADYREGISTERED
    );
    drop(still_rejected);

    let target = &registration.target;
    let enter = POINTL { x: -37, y: 219 };
    let over = POINTL { x: 271, y: -43 };
    let dropped = POINTL { x: 193, y: 257 };
    let context = eframe::egui::Context::default();
    context.set_pixels_per_point(1.5);
    let _ = crate::ui::test_frame::run_ui(&context, Default::default(), |_| {});
    assert_eq!(context.pixels_per_point(), 1.5);
    let _ = super::EGUI_CONTEXT.set(context.clone());
    let _ = super::WINDOW_HWND.set(hwnd as isize);
    let expected = |point: POINTL| {
        let mut origin = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        assert_ne!(
            unsafe { windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin) },
            0
        );
        Some(eframe::egui::pos2(
            (point.x - origin.x) as f32 / context.pixels_per_point(),
            (point.y - origin.y) as f32 / context.pixels_per_point(),
        ))
    };
    assert_eq!(client_point(window.0, enter), expected(enter));
    assert_eq!(client_point(window.0, over), expected(over));
    assert_eq!(client_point(window.0, dropped), expected(dropped));
    let mut effect = DROPEFFECT_COPY;
    unsafe {
        target
            .DragEnter(&payload, MODIFIERKEYS_FLAGS(0), enter, &mut effect)
            .unwrap();
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(hovered().paths, vec![path.clone()]);
    // Files win over the URL formats offered with them.
    assert_eq!(hovered().urls, Vec::<String>::new());
    assert_eq!(hovered().screen_pos, expected(enter));
    unsafe {
        target
            .DragOver(MODIFIERKEYS_FLAGS(0), over, &mut effect)
            .unwrap();
    }
    assert_eq!(hovered().screen_pos, expected(over));
    unsafe {
        target.DragLeave().unwrap();
    }
    assert!(hovered().is_empty());
    assert_eq!(hovered().screen_pos, None);
    unsafe {
        target
            .DragEnter(&payload, MODIFIERKEYS_FLAGS(0), enter, &mut effect)
            .unwrap();
        target
            .Drop(&payload, MODIFIERKEYS_FLAGS(0), dropped, &mut effect)
            .unwrap();
    }
    let received = take_drop();
    assert_eq!(received.paths, vec![path]);
    assert_eq!(received.urls, Vec::<String>::new());
    assert_eq!(received.screen_pos, expected(dropped));
    assert!(hovered().is_empty());
    let consumed = take_drop();
    assert!(consumed.is_empty());
    assert_eq!(consumed.screen_pos, None);
    if check_desktop_cursor {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};
        let mut original = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        assert_ne!(unsafe { GetCursorPos(&mut original) }, 0);
        struct RestoreCursor(windows_sys::Win32::Foundation::POINT);
        impl Drop for RestoreCursor {
            fn drop(&mut self) {
                unsafe {
                    SetCursorPos(self.0.x, self.0.y);
                }
            }
        }
        let _restore = RestoreCursor(original);
        assert_ne!(unsafe { SetCursorPos(dropped.x, dropped.y) }, 0);
        assert_eq!(super::cursor_screen_pos(), expected(dropped));
        assert_eq!(super::super::hovered().screen_pos, expected(dropped));
        assert_eq!(super::super::take_drop().screen_pos, expected(dropped));
    }
    drop(registration);
    let mut replacement = DropTargetRegistration::new(window.0);
    replacement.register().unwrap();
}

// A Chromium <img> drag offers these formats and no CF_HDROP. The image bytes
// arrive only as a virtual FileGroupDescriptorW/FileContents file.
fn chromium_image_drag() -> IDataObject {
    let format = |name: &str| unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) as u16 };
    // Extensionless, like many CDN images, so it scores the same as the page.
    let image = "https://images.example.com/photo_(1)?w=800";
    let fragment = format!(r#"<img src="{image}" alt="drop">"#);
    let html = format!(
        "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\nSourceURL:https://example.com/gallery\r\n<html><body><!--StartFragment-->{fragment}<!--EndFragment--></body></html>\0"
    );
    TestPayload(vec![
        (format("UniformResourceLocatorW"), wide_bytes(image)),
        (
            format("UniformResourceLocator"),
            format!("{image}\0").into_bytes(),
        ),
        (13, wide_bytes(image)),
        (format("HTML Format"), html.into_bytes()),
    ])
    .into()
}

#[test]
fn app_window_bridge_accepts_browser_image_drags() {
    use eframe::egui;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::application::ApplicationHandler;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows as _;

    #[derive(Default)]
    struct BrowserDrop {
        effect: Option<windows::Win32::System::Ole::DROPEFFECT>,
        received: Option<super::super::NativeDropPayload>,
    }
    impl ApplicationHandler for BrowserDrop {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.received.is_some() {
                return;
            }
            // The window gets the attributes eframe derives from the app's viewport.
            let viewport = super::super::configure_viewport(
                egui::ViewportBuilder::default().with_visible(false),
            );
            let attributes =
                egui_winit::create_winit_window_attributes(&egui::Context::default(), viewport);
            let window = event_loop.create_window(attributes).unwrap();
            let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() else {
                panic!("a Windows window has a Win32 handle");
            };
            unsafe { OleInitialize(None).unwrap() };
            let mut registration = DropTargetRegistration::new(HWND(handle.hwnd.get() as *mut _));
            registration.register().unwrap();
            let payload = chromium_image_drag();
            let point = POINTL { x: 0, y: 0 };
            let mut effect = DROPEFFECT_NONE;
            unsafe {
                let target = &registration.target;
                target
                    .DragEnter(&payload, MODIFIERKEYS_FLAGS(0), point, &mut effect)
                    .unwrap();
                self.effect = Some(effect);
                target
                    .Drop(&payload, MODIFIERKEYS_FLAGS(0), point, &mut effect)
                    .unwrap();
            }
            self.received = Some(take_drop());
            drop(registration);
            unsafe { OleUninitialize() };
            event_loop.exit();
        }

        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }

    let _drop_state = DROP_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *state().lock().unwrap() = Default::default();
    let mut browser_drop = BrowserDrop::default();
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .unwrap()
        .run_app(&mut browser_drop)
        .unwrap();
    assert_eq!(browser_drop.effect, Some(DROPEFFECT_COPY));
    let received = browser_drop.received.unwrap();
    assert_eq!(
        received.urls,
        ["https://images.example.com/photo_(1)?w=800"]
    );
    assert_eq!(received.paths, Vec::<PathBuf>::new());
}
