pub(crate) mod import_completion;
pub(crate) mod perf;

pub(crate) fn search_drag_debug(message: impl AsRef<str>) {
    let Some(path) =
        std::env::var_os("SNAPPYREF_SEARCH_DRAG_DEBUG_LOG").map(std::path::PathBuf::from)
    else {
        return;
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "[DEBUG-search-drag] {timestamp} {}", message.as_ref());
    }
}
