use std::path::Path;

#[cfg(target_os = "macos")]
use std::{fs, ptr::NonNull};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSBitmapImageFileType, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::NSDictionary;
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub struct NativeWindowCapture {
    #[cfg(target_os = "macos")]
    ns_view_ptr: NonNull<std::ffi::c_void>,
}

impl NativeWindowCapture {
    pub fn from_creation_context(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            let handle = cc.window_handle().ok()?.as_raw();
            let RawWindowHandle::AppKit(appkit) = handle else {
                return None;
            };
            let ns_view_ptr = NonNull::new(appkit.ns_view.as_ptr())?;
            return Some(Self { ns_view_ptr });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = cc;
            None
        }
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn capture_png(&self, path: &Path) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
            }

            let ns_view = unsafe {
                self.ns_view_ptr
                    .cast::<NSView>()
                    .as_ref()
            };

            let bounds = ns_view.bounds();
            let bitmap = unsafe {
                ns_view.bitmapImageRepForCachingDisplayInRect(bounds)
            }
            .ok_or_else(|| "failed to allocate bitmap image rep for window capture".to_owned())?;

            unsafe {
                ns_view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
            }

            let properties = NSDictionary::new();
            let png_data = unsafe {
                bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
            }
            .ok_or_else(|| "failed to encode window capture as PNG".to_owned())?;

            fs::write(path, png_data.bytes())
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err("native window capture is only implemented on macOS".to_owned())
        }
    }
}
