use std::path::PathBuf;

/// Asks for a `.pur` destination with the native save panel. Runs on the UI
/// thread: AppKit requires it, and egui's Windows thread is already an STA.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn pureref_save_path(frame: &eframe::Frame, file_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_parent(frame)
        .set_file_name(file_name)
        .add_filter("PureRef board", &["pur"])
        .save_file()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn pureref_save_path(_frame: &eframe::Frame, _file_name: &str) -> Option<PathBuf> {
    None
}
