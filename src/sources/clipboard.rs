use std::borrow::Cow;

use crate::canvas::CanvasState;
use crate::library::storage::Library;

const MAX_EXTERNAL_CLIPBOARD_IMAGE_BYTES: usize = 200 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardCopyPayload {
    pub(crate) internal_json: String,
    pub(crate) item_count: usize,
    pub(crate) external_image: Option<ExternalClipboardImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExternalClipboardImage {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn build_selection_copy_payload(
    canvas: &CanvasState,
    library: Option<&Library>,
) -> Result<Option<ClipboardCopyPayload>, String> {
    let Some(outcome) = crate::active_board::commands::copy_selection_to_json(canvas)? else {
        return Ok(None);
    };
    let external_image = external_image_for_selection(canvas, library)?;
    Ok(Some(ClipboardCopyPayload {
        internal_json: outcome.payload_json,
        item_count: outcome.item_count,
        external_image,
    }))
}

fn external_image_for_selection(
    canvas: &CanvasState,
    library: Option<&Library>,
) -> Result<Option<ExternalClipboardImage>, String> {
    let selected_placement_ids = canvas.selected_placement_ids();
    if selected_placement_ids.len() != 1 || !canvas.selected_annotation_ids().is_empty() {
        return Ok(None);
    }

    let Some(library) = library else {
        return Ok(None);
    };
    let placement = canvas
        .placement_for_id(selected_placement_ids[0])
        .ok_or_else(|| "missing selected placement".to_owned())?;
    let byte_len = external_image_byte_len(
        placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .width,
        placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .height,
    )?;
    if byte_len > MAX_EXTERNAL_CLIPBOARD_IMAGE_BYTES {
        return Ok(None);
    }

    let Ok((rgba, width, height)) = crate::jobs::load_canvas_snapshot_pixels(
        library,
        &placement,
        placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .width,
        placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .height,
    ) else {
        return Ok(None);
    };
    Ok(Some(ExternalClipboardImage {
        rgba,
        width,
        height,
    }))
}

fn external_image_byte_len(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "external clipboard image dimensions overflowed".to_owned())
}

pub(crate) fn write_payload(payload: &ClipboardCopyPayload) -> Result<(), String> {
    match &payload.external_image {
        Some(image) => write_image_payload_with_text(image, &payload.internal_json),
        None => {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|error| format!("Clipboard unavailable: {error}"))?;
            clipboard
                .set_text(payload.internal_json.clone())
                .map_err(|error| format!("Failed to copy selection: {error}"))
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_image_payload_with_text(image: &ExternalClipboardImage, text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: Cow::Borrowed(&image.rgba),
        })
        .map_err(|error| format!("Failed to copy image: {error}"))?;
    append_internal_text(text)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_image_payload_with_text(
    _image: &ExternalClipboardImage,
    text: &str,
) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?;
    clipboard
        .set_text(text.to_owned())
        .map_err(|error| format!("Failed to copy selection: {error}"))
}

#[cfg(target_os = "macos")]
fn append_internal_text(text: &str) -> Result<(), String> {
    use objc2::msg_send;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;

    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
    let text = NSString::from_str(text);
    let success: bool =
        unsafe { msg_send![&pasteboard, setString: &*text, forType: NSPasteboardTypeString] };
    if success {
        Ok(())
    } else {
        Err("Failed to add SnappyRef clipboard metadata".to_owned())
    }
}

#[cfg(target_os = "windows")]
fn append_internal_text(text: &str) -> Result<(), String> {
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn append_internal_text(_text: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_selection_copy_payload;
    use crate::data::{ImageAsset, Placement, PlacementState};

    #[test]
    fn single_selected_placement_copies_internal_json_and_external_image() {
        let ctx = eframe::egui::Context::default();
        let root = temp_library_root("snappyref-copy-external-image");
        let library = crate::library::storage::Library::initialize(root).unwrap();
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let source = library
            .store_rgba_image("copy-source", &rgba, 2, 1)
            .unwrap();
        let placement = library
            .persist_imported_image(
                &source,
                "image/png",
                2,
                1,
                std::fs::metadata(&source).unwrap().len(),
                eframe::egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let placement_id = placement.state.id;
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(&ctx, loaded_item(placement, rgba.clone(), 2, 1));
        canvas.select_placements(&[placement_id]);

        let payload = build_selection_copy_payload(&canvas, Some(&library))
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 1);
        assert!(
            payload
                .internal_json
                .contains(crate::active_board::commands::COPY_PAYLOAD_KIND)
        );
        assert_eq!(
            payload.external_image,
            Some(super::ExternalClipboardImage {
                rgba,
                width: 2,
                height: 1,
            })
        );
    }

    #[test]
    fn mixed_selection_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(
            &ctx,
            loaded_item(test_record(1, 1), vec![255, 255, 255, 255], 1, 1),
        );
        canvas.add_annotation(crate::data::CanvasAnnotation::new(
            20,
            crate::data::CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(10.0, 10.0),
            eframe::egui::pos2(70.0, 40.0),
            "caption".to_owned(),
            crate::data::CanvasTextSize::M,
            crate::data::CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
        canvas.select_content(&[1], &[20]);

        let payload = build_selection_copy_payload(&canvas, None)
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 2);
        assert!(payload.external_image.is_none());
    }

    #[test]
    fn oversized_single_placement_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let root = temp_library_root("snappyref-copy-oversized-image");
        let library = crate::library::storage::Library::initialize(root).unwrap();
        let mut canvas = crate::canvas::CanvasState::default();
        let mut placement = test_record(1, 1);
        placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .width = 20_000;
        placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .height = 20_000;
        placement.state.geometry.size = eframe::egui::vec2(20_000.0, 20_000.0);
        canvas.add_prepared_placement_image(
            &ctx,
            loaded_item(placement, vec![255, 255, 255, 255], 1, 1),
        );
        canvas.select_placements(&[1]);

        let payload = build_selection_copy_payload(&canvas, Some(&library))
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 1);
        assert!(payload.external_image.is_none());
    }

    #[test]
    fn unsupported_single_placement_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let root = temp_library_root("snappyref-copy-unsupported-image");
        let library = crate::library::storage::Library::initialize(root).unwrap();
        let mut placement = test_record(1, 1);
        placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .mime_type = "image/svg+xml".to_owned();

        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(
            &ctx,
            loaded_item(placement, vec![255, 255, 255, 255], 1, 1),
        );
        canvas.select_placements(&[1]);

        let payload = build_selection_copy_payload(&canvas, Some(&library))
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 1);
        assert!(payload.external_image.is_none());
    }

    fn loaded_item(
        placement: Placement,
        preview_rgba: Vec<u8>,
        preview_width: u32,
        preview_height: u32,
    ) -> crate::jobs::PreparedPlacementImage {
        crate::jobs::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba,
            preview_width,
            preview_height,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        }
    }

    fn test_record(placement_id: i64, asset_id: i64) -> Placement {
        Placement::loaded(
            PlacementState {
                id: placement_id,
                asset_id: Some(asset_id),
                geometry: crate::data::RectGeometry::new(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::Vec2::ZERO,
                    1.0,
                    0.0,
                    0,
                ),
                archived: false,
            },
            ImageAsset {
                id: asset_id,
                original_path: std::path::PathBuf::from(format!("fixture-{asset_id}.png")),
                filename: format!("fixture-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width: 1,
                height: 1,
                file_size: 4,
                imported_at: 0,
            },
        )
    }

    fn temp_library_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
