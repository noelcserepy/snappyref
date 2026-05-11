//! Per-platform release-shape constants for the auto-updater.
//!
//! The updater operates on a "swap unit": a single filesystem entity the
//! helper renames into place. On macOS that's `SnappyRef.app` (a directory
//! bundle); on Windows it's `snappyref.exe` (a regular file). The staging
//! and rollback flows both encode these names — this module is the single
//! source of truth so the two halves cannot disagree.

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

    /// Name of the backup slot the previous swap left behind:
    /// `SnappyRef.app.previous` / `snappyref.exe.old`.
    pub(crate) fn backup_name(self) -> &'static str {
        match self {
            Self::Macos => "SnappyRef.app.previous",
            Self::Windows => "snappyref.exe.old",
        }
    }

    /// Name of the temporary slot used by rollback during the three-step
    /// swap. Sits next to the current/backup pair so the rename is on the
    /// same filesystem.
    pub(crate) fn temp_name(self) -> &'static str {
        match self {
            Self::Macos => "SnappyRef.app.rolling",
            Self::Windows => "snappyref.exe.rolling",
        }
    }
}
