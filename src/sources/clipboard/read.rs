use crate::image_preparation::ImportSource;

use super::{ClipboardBoardSelection, decode_board_selection, native};

pub(crate) enum ClipboardContent {
    BoardSelection(ClipboardBoardSelection),
    Sources {
        sources: Vec<ImportSource>,
        pureref: bool,
    },
}

/// Normalize one clipboard interpretation. Event text is the native paste
/// event's snapshot; context-menu paste reads text directly when absent.
pub(crate) fn read(event_text: Option<&str>) -> Result<Option<ClipboardContent>, String> {
    read_from(&mut NativeClipboard, event_text)
}

// Platform reads remain lazy: an internal selection never reads raster pixels.
trait ClipboardReader {
    fn text(&mut self) -> Option<String>;
    fn paths(&mut self) -> Vec<std::path::PathBuf>;
    fn pureref_binary(&mut self) -> Option<Vec<u8>>;
    fn image(&mut self) -> Result<Option<ImportSource>, String>;
}

fn read_from(
    clipboard: &mut impl ClipboardReader,
    event_text: Option<&str>,
) -> Result<Option<ClipboardContent>, String> {
    let native_text = event_text.is_none().then(|| clipboard.text()).flatten();
    if let Some(text) = event_text.or(native_text.as_deref()) {
        if let Some(selection) = decode_board_selection(text)? {
            return Ok(Some(ClipboardContent::BoardSelection(selection)));
        }
        if let Some(url) = crate::sources::url::normalize_import_url(text) {
            return Ok(Some(ClipboardContent::Sources {
                sources: vec![ImportSource::Url(url)],
                pureref: false,
            }));
        }
    }
    let paths = clipboard.paths();
    if !paths.is_empty() {
        return Ok(Some(ClipboardContent::Sources {
            sources: paths.into_iter().map(ImportSource::Path).collect(),
            pureref: false,
        }));
    }
    if let Some(bytes) = clipboard.pureref_binary() {
        let sources = crate::sources::pureref::sources_from_binary(&bytes);
        if !sources.is_empty() {
            return Ok(Some(ClipboardContent::Sources {
                sources,
                pureref: true,
            }));
        }
    }
    Ok(clipboard.image()?.map(|image| ClipboardContent::Sources {
        sources: vec![image],
        pureref: false,
    }))
}

struct NativeClipboard;

impl ClipboardReader for NativeClipboard {
    fn text(&mut self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn paths(&mut self) -> Vec<std::path::PathBuf> {
        native::clipboard_paths()
    }

    fn pureref_binary(&mut self) -> Option<Vec<u8>> {
        native::clipboard_pureref_binary()
    }

    fn image(&mut self) -> Result<Option<ImportSource>, String> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?;
        let image = match clipboard.get_image() {
            Ok(image) => image,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(error) => return Err(format!("Clipboard does not contain an image: {error}")),
        };
        let width = u32::try_from(image.width)
            .map_err(|_| "Clipboard image dimensions overflowed".to_owned())?;
        let height = u32::try_from(image.height)
            .map_err(|_| "Clipboard image dimensions overflowed".to_owned())?;
        Ok(Some(ImportSource::Clipboard {
            rgba: image.bytes.into_owned(),
            width,
            height,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ClipboardFixture {
        text: Option<String>,
        paths: Vec<std::path::PathBuf>,
        binary: Option<Vec<u8>>,
        image: Option<ImportSource>,
        reads: Vec<&'static str>,
    }

    impl ClipboardReader for ClipboardFixture {
        fn text(&mut self) -> Option<String> {
            self.reads.push("text");
            self.text.clone()
        }
        fn paths(&mut self) -> Vec<std::path::PathBuf> {
            self.reads.push("paths");
            self.paths.clone()
        }
        fn pureref_binary(&mut self) -> Option<Vec<u8>> {
            self.reads.push("pureref");
            self.binary.clone()
        }
        fn image(&mut self) -> Result<Option<ImportSource>, String> {
            self.reads.push("image");
            Ok(self.image.clone())
        }
    }

    fn selection_text(scale: f32) -> String {
        serde_json::json!({
            "kind": super::super::COPY_PAYLOAD_KIND,
            "source_center_x": 0., "source_center_y": 0.,
            "items": [{"asset_id": 1, "center_x": 0., "center_y": 0.,
                "scale": scale, "rotation": 0., "z_index": 0,
                "width": 2., "height": 1.}],
            "annotations": []
        })
        .to_string()
    }

    #[test]
    fn reads_one_representation_in_precedence_order_without_lower_priority_reads() {
        let binary = ["GraphicsImageItem", "https://example.com/pureref.png"]
            .into_iter()
            .flat_map(|text| {
                let bytes = text
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>();
                (bytes.len() as u32).to_be_bytes().into_iter().chain(bytes)
            })
            .collect();
        let pixels = ImportSource::Clipboard {
            rgba: vec![255; 4],
            width: 1,
            height: 1,
        };
        let mut clipboard = ClipboardFixture {
            text: Some(selection_text(1.)),
            paths: vec!["file.png".into()],
            binary: Some(binary),
            image: Some(pixels.clone()),
            ..Default::default()
        };
        assert!(matches!(
            read_from(&mut clipboard, None).unwrap(),
            Some(ClipboardContent::BoardSelection(_))
        ));
        assert_eq!(clipboard.reads, ["text"]);
        clipboard.reads.clear();
        // The event snapshot wins over a later native clipboard text value.
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, Some("https://example.com/event.png")).unwrap()
        else {
            panic!("expected URL");
        };
        assert_eq!(
            sources,
            [ImportSource::Url("https://example.com/event.png".into())]
        );
        assert!(!pureref);
        assert!(clipboard.reads.is_empty());
        clipboard.text = None;
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, None).unwrap()
        else {
            panic!("expected paths");
        };
        assert_eq!(sources, [ImportSource::Path("file.png".into())]);
        assert!(!pureref);
        assert_eq!(clipboard.reads, ["text", "paths"]);
        clipboard.reads.clear();
        clipboard.paths.clear();
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, None).unwrap()
        else {
            panic!("expected PureRef");
        };
        assert_eq!(
            sources,
            [ImportSource::Url("https://example.com/pureref.png".into())]
        );
        assert!(pureref);
        assert_eq!(clipboard.reads, ["text", "paths", "pureref"]);
        clipboard.reads.clear();
        clipboard.binary = Some(vec![0, 1, 255]);
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, Some("{invalid metadata")).unwrap()
        else {
            panic!("expected native image");
        };
        assert_eq!(sources, [pixels]);
        assert!(!pureref);
        assert_eq!(clipboard.reads, ["paths", "pureref", "image"]);
        clipboard.image = None;
        assert!(read_from(&mut clipboard, None).unwrap().is_none());
    }

    #[test]
    fn rejects_invalid_internal_geometry_before_reading_external_formats() {
        let mut clipboard = ClipboardFixture::default();
        assert!(read_from(&mut clipboard, Some(&selection_text(-1.))).is_err());
        assert!(clipboard.reads.is_empty());
    }

    #[test]
    fn fresh_external_contents_replace_internal_selection_and_empty_text_does_nothing() {
        let mut clipboard = ClipboardFixture {
            text: Some(selection_text(1.)),
            ..Default::default()
        };
        assert!(matches!(
            read_from(&mut clipboard, None).unwrap(),
            Some(ClipboardContent::BoardSelection(_))
        ));
        clipboard.text = None;
        clipboard.image = Some(ImportSource::Clipboard {
            rgba: vec![12, 34, 56, 255],
            width: 1,
            height: 1,
        });
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, None).unwrap()
        else {
            panic!("fresh image expected")
        };
        assert_eq!(sources, vec![clipboard.image.clone().unwrap()]);
        assert!(!pureref);
        clipboard.image = None;
        for text in [None, Some(""), Some("ordinary unsupported text")] {
            assert!(read_from(&mut clipboard, text).unwrap().is_none());
        }
        clipboard.paths = vec!["first.png".into(), "second.jpg".into()];
        let Some(ClipboardContent::Sources { sources, pureref }) =
            read_from(&mut clipboard, None).unwrap()
        else {
            panic!("file batch expected")
        };
        assert_eq!(
            sources,
            vec![
                ImportSource::Path("first.png".into()),
                ImportSource::Path("second.jpg".into())
            ]
        );
        assert!(!pureref);
    }
}
