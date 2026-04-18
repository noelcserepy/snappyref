use std::path::{Path, PathBuf};

use eframe::egui;

use crate::native_capture::NativeWindowCapture;

pub struct ScreenshotCapture {
    next_path: Option<PathBuf>,
    pending_path: Option<PathBuf>,
    armed: bool,
}

impl ScreenshotCapture {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            next_path: path,
            pending_path: None,
            armed: false,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending_path.is_some()
    }

    pub fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        window_capture: Option<&NativeWindowCapture>,
        status: &mut String,
    ) -> bool {
        if self.pending_path.is_some() {
            return false;
        }

        let Some(_path) = self.next_path.clone() else {
            return false;
        };

        if !self.armed {
            self.armed = true;
            ctx.request_repaint();
            return false;
        }

        let Some(path) = self.next_path.take() else {
            return false;
        };
        self.armed = false;

        if let Some(window_capture) = window_capture {
            match window_capture.capture_png(&path) {
                Ok(()) => {
                    *status = format!("Saved screenshot to {}", path.display());
                }
                Err(error) => {
                    *status = error;
                }
            }
            return true;
        }

        self.pending_path = Some(path);
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        ctx.request_repaint();
        false
    }

    pub fn handle_event(&mut self, ctx: &egui::Context, status: &mut String) -> bool {
        let Some(path) = self.pending_path.clone() else {
            return false;
        };

        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });

        let Some(image) = screenshot else {
            return false;
        };

        let pixels = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect::<Vec<_>>();

        match save_egui_screenshot(&path, &pixels, image.width() as u32, image.height() as u32) {
            Ok(()) => {
                *status = format!("Saved screenshot to {}", path.display());
                self.pending_path = None;
            }
            Err(error) => {
                *status = error;
                self.pending_path = None;
            }
        }

        true
    }
}

fn save_egui_screenshot(path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    write_parent_dir(path)?;
    image::save_buffer_with_format(
        path,
        pixels,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("failed to write screenshot to {}: {error}", path.display()))
}

fn write_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    Ok(())
}
