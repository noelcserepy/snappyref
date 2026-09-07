use super::{build_selection_copy_payload, write_payload};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasStrokeWidth,
    CanvasTextSize,
};
use crate::test_support::TempRoot;
use eframe::egui;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::CF_HDROP;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

struct ClipboardCleanup(HWND);
impl Drop for ClipboardCleanup {
    fn drop(&mut self) {
        unsafe {
            if OpenClipboard(self.0) != 0 {
                EmptyClipboard();
                CloseClipboard();
            }
            DestroyWindow(self.0);
        }
    }
}
struct OpenNativeClipboard;
impl Drop for OpenNativeClipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

fn publish_bytes(format: u32, bytes: &[u8]) -> usize {
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len());
        assert!(!handle.is_null());
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            GlobalFree(handle);
            panic!("could not lock test clipboard allocation");
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
        GlobalUnlock(handle);
        if SetClipboardData(format, handle).is_null() {
            GlobalFree(handle);
            panic!("could not publish test clipboard allocation");
        }
        // Successful publication transfers this allocation to Windows.
        GlobalSize(handle)
    }
}

#[test]
#[ignore = "writes the native Windows clipboard"]
fn native_windows_clipboard_image_metadata_files_and_pureref_bytes() {
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("SnappyRef clipboard test").as_ptr(),
            0,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    let _cleanup = ClipboardCleanup(hwnd);
    let root = TempRoot::new("snappyref-native-windows-clipboard");
    let library = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
    let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let source = library
        .store_rgba_image("clipboard-source", &rgba, 2, 1)
        .unwrap();
    let placement = library
        .persist_imported_image(
            &source,
            "image/png",
            2,
            1,
            std::fs::metadata(&source).unwrap().len(),
            egui::Pos2::ZERO,
            1.0,
        )
        .unwrap();
    let placement_id = placement.state.id;
    let mut canvas = crate::canvas::CanvasState::default();
    canvas.add_prepared_placement_image(
        &egui::Context::default(),
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba: rgba.clone(),
            preview_width: 2,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );
    canvas.select_placements(&[placement_id]);
    let image_payload = build_selection_copy_payload(&canvas, Some(&library))
        .unwrap()
        .unwrap();
    assert!(image_payload.external_image.is_some());
    write_payload(&image_payload).unwrap();
    {
        let mut clipboard = arboard::Clipboard::new().unwrap();
        let image = clipboard.get_image().unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.bytes.as_ref(), rgba.as_slice());
        assert_eq!(clipboard.get_text().unwrap(), image_payload.internal_json);
    }

    canvas.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        egui::pos2(10.0, 10.0),
        egui::pos2(70.0, 40.0),
        "caption".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        CanvasAnnotationColor::Default,
        false,
        false,
    ));
    canvas.select_content(&[placement_id], &[20]);
    let mixed_payload = build_selection_copy_payload(&canvas, Some(&library))
        .unwrap()
        .unwrap();
    assert_eq!(mixed_payload.item_count, 2);
    assert!(mixed_payload.external_image.is_none());
    write_payload(&mixed_payload).unwrap();
    {
        let mut clipboard = arboard::Clipboard::new().unwrap();
        assert_eq!(clipboard.get_text().unwrap(), mixed_payload.internal_json);
        assert!(clipboard.get_image().is_err());
    }

    let paths = [
        std::path::PathBuf::from(r"C:\SnappyRef\first.png"),
        std::path::PathBuf::from(r"C:\SnappyRef\second image.png"),
    ];
    // DROPFILES header followed by a double-null-terminated UTF-16 path list.
    let mut files: Vec<u8> = [20u32, 0, 0, 0, 1]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    for path in &paths {
        files.extend(
            wide(path.to_str().unwrap())
                .into_iter()
                .flat_map(u16::to_ne_bytes),
        );
    }
    files.extend([0, 0]);
    let binary = [0, 1, 255, 128, 42, 0];
    let pureref_format = unsafe { RegisterClipboardFormatW(wide("pureref/binary").as_ptr()) };
    assert_ne!(pureref_format, 0);
    let binary_size;
    {
        assert_ne!(unsafe { OpenClipboard(hwnd) }, 0);
        let _open = OpenNativeClipboard;
        assert_ne!(unsafe { EmptyClipboard() }, 0);
        publish_bytes(CF_HDROP as u32, &files);
        binary_size = publish_bytes(pureref_format, &binary);
    }
    assert_eq!(crate::platform::native_drop::clipboard_paths(), paths);
    let mut expected_binary = binary.to_vec();
    expected_binary.resize(binary_size, 0);
    assert_eq!(
        crate::platform::native_drop::clipboard_pureref_binary(),
        Some(expected_binary)
    );
    // This verifies the registered binary format, not the PureRef application.
    assert_ne!(unsafe { OpenClipboard(hwnd) }, 0);
    let _open = OpenNativeClipboard;
    assert_ne!(unsafe { EmptyClipboard() }, 0);
}
