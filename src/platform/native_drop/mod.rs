#[cfg(target_os = "macos")]
mod macos;
mod payload;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fallback as platform;

pub(crate) use payload::NativeDropPayload;

pub fn install_url_drop_bridge(cc: &eframe::CreationContext<'_>) {
    platform::install(cc);
}

pub fn hovered() -> NativeDropPayload {
    let mut payload = platform::hovered();
    payload.screen_pos = payload.screen_pos.or_else(platform::cursor_screen_pos);
    payload
}

pub fn take_drop() -> NativeDropPayload {
    let mut payload = platform::take_drop();
    payload.screen_pos = payload.screen_pos.or_else(platform::cursor_screen_pos);
    payload
}

pub fn cursor_screen_pos() -> Option<eframe::egui::Pos2> {
    platform::cursor_screen_pos()
}

pub fn clipboard_paths() -> Vec<std::path::PathBuf> {
    platform::clipboard_paths()
}

pub fn clipboard_pureref_binary() -> Option<Vec<u8>> {
    platform::clipboard_pureref_binary()
}
