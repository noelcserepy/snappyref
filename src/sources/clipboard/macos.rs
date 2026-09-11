use objc2::rc::Retained;
use objc2::{msg_send, msg_send_id};
use objc2_app_kit::{NSFilenamesPboardType, NSPasteboard, NSPasteboardTypeFileURL};

pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
    let mut paths = Vec::new();
    append_file_paths_from_pasteboard(&pasteboard, &mut paths);
    if let Some(items) = unsafe { pasteboard.pasteboardItems() } {
        for item in items {
            append_file_paths_from_item(&item, &mut paths);
        }
    }
    paths
}

pub(super) fn clipboard_pureref_binary() -> Option<Vec<u8>> {
    None
}

pub(crate) fn append_file_paths_from_pasteboard(
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

pub(crate) fn append_file_paths_from_item(
    item: &objc2_app_kit::NSPasteboardItem,
    paths: &mut Vec<std::path::PathBuf>,
) {
    let Some(value) = (unsafe { item.stringForType(NSPasteboardTypeFileURL) }) else {
        return;
    };
    append_file_path_from_file_url(&value.to_string(), paths);
}

fn append_file_path_from_file_url(raw_url: &str, paths: &mut Vec<std::path::PathBuf>) {
    let Ok(url) = url::Url::parse(raw_url) else {
        return;
    };
    let Ok(path) = url.to_file_path() else {
        return;
    };
    if !paths.contains(&path) {
        paths.push(path);
    }
}

pub(super) fn append_internal_text(text: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::append_file_path_from_file_url;
    #[test]
    fn file_url_clipboard_values_decode_to_paths_once() {
        let mut paths = Vec::new();

        append_file_path_from_file_url("file:///Users/alice/Reference%20A.png", &mut paths);
        append_file_path_from_file_url("file:///Users/alice/Reference%20A.png", &mut paths);
        append_file_path_from_file_url("https://example.com/Reference%20A.png", &mut paths);

        assert_eq!(
            paths,
            vec![std::path::PathBuf::from("/Users/alice/Reference A.png")]
        );
    }
}
