//! Per-platform release-shape constants for the auto-updater.
//!
//! The replacement unit is `SnappyRef.app` on macOS and `snappyref.exe`
//! on Windows. The macOS helper replaces the app bundle; the staged Windows
//! executable runs its own replacement mode. Staging and application use
//! these names so they agree on the file or directory being replaced.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetPlatform {
    Macos,
    Windows,
}

impl TargetPlatform {
    /// Resolve the platform for the running target. `None` on hosts that
    /// have no defined release shape (e.g. Linux dev hosts); the auto-
    /// updater treats that as "skip".
    pub(crate) fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Macos)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else {
            None
        }
    }

    /// Name of the live swap unit relative to `<install_root>`:
    /// `SnappyRef.app` on macOS, `snappyref.exe` on Windows.
    pub(crate) fn swap_unit_name(self) -> &'static str {
        match self {
            Self::Macos => "SnappyRef.app",
            Self::Windows => "snappyref.exe",
        }
    }
}
