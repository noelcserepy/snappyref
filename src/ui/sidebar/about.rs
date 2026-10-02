//! The About section: the running version and the update it can move to,
//! in the same status row as Semantic Search.

use super::settings::StatusRow;
use crate::ui::theme;

/// What About shows. The app owns the data; the sidebar only reads it.
#[derive(Clone, Debug)]
pub(crate) struct AboutView {
    /// Running app version, normally `env!("CARGO_PKG_VERSION")`.
    pub version: String,
    /// Unix-epoch milliseconds of the last completed check.
    pub last_checked_unix_millis: Option<u64>,
    /// False while a manual check is throttled.
    pub check_enabled: bool,
    pub update: AppUpdateStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AppUpdateStatus {
    Unchecked,
    UpToDate,
    Checking,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        percent: u8,
    },
    /// Downloaded while the user asked to update, so the app restarts on
    /// its own.
    Finishing {
        version: String,
    },
    Ready {
        version: String,
    },
    CheckFailed,
    InstallFailed {
        version: String,
        reason: String,
    },
}

/// The About row: the running version, the update it can move to, and when
/// the app last checked.
pub(super) fn status(about: &AboutView) -> StatusRow {
    let v = &about.version;
    let checked = match about.last_checked_unix_millis {
        Some(millis) => format!("Checked {}.", format_last_checked(millis)),
        None => "Not checked yet.".to_owned(),
    };
    let check = |dot, line: String, detail: String, enabled| StatusRow {
        line,
        detail: Some(detail),
        action: "Check",
        primary: false,
        enabled,
        dot,
        progress: None,
    };
    let update = |line: String, detail: &str, action, primary, progress| StatusRow {
        line,
        detail: Some(detail.to_owned()),
        action,
        primary,
        enabled: primary,
        dot: theme::ACCENT,
        progress,
    };
    match &about.update {
        AppUpdateStatus::Unchecked => {
            check(theme::TEXT_SUBTLE, v.clone(), checked, about.check_enabled)
        }
        AppUpdateStatus::UpToDate => check(
            theme::GREEN,
            format!("{v} · Up to date"),
            checked,
            about.check_enabled,
        ),
        AppUpdateStatus::Checking => check(
            theme::TEXT_MUTED,
            format!("{v} · Checking for updates"),
            checked,
            false,
        ),
        AppUpdateStatus::Available { version } => update(
            format!("{v} · {version} available"),
            &checked,
            "Update",
            true,
            None,
        ),
        AppUpdateStatus::Downloading { version, percent } => update(
            format!("Downloading {version} · {percent}%"),
            "SnappyRef restarts when the download finishes.",
            "Update",
            false,
            Some(f32::from(*percent) / 100.0),
        ),
        AppUpdateStatus::Finishing { version } => update(
            format!("Restarting into {version}"),
            "Your boards reopen where you left them.",
            "Update",
            false,
            None,
        ),
        AppUpdateStatus::Ready { version } => update(
            format!("{version} ready"),
            "Restart to finish the update.",
            "Restart",
            true,
            None,
        ),
        AppUpdateStatus::InstallFailed { version, reason } => check(
            theme::DESTRUCTIVE,
            format!("Update to {version} failed"),
            reason.clone(),
            about.check_enabled,
        ),
        AppUpdateStatus::CheckFailed => check(
            theme::DESTRUCTIVE,
            format!("{v} · Check failed"),
            checked,
            about.check_enabled,
        ),
    }
}

fn format_last_checked(millis: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(millis);
    let seconds = now.saturating_sub(millis) / 1_000;
    let (count, unit) = match seconds {
        0..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}
