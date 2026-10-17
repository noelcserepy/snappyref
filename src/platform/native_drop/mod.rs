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

/// On Windows the bridge's OLE target is the window's only drop target. Winit
/// would otherwise register a file-only target first and the bridge, which
/// also reads browser URLs, could not register.
pub fn configure_viewport(
    viewport: eframe::egui::ViewportBuilder,
) -> eframe::egui::ViewportBuilder {
    #[cfg(target_os = "windows")]
    let viewport = viewport.with_drag_and_drop(false);
    viewport
}

pub fn install_drop_bridge(cc: &eframe::CreationContext<'_>) {
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

#[cfg(all(test, target_os = "macos"))]
pub(crate) use macos::browser_drop_probe;
