//! The About page: the running version and the update it can move to, in
//! the same status panel as Semantic Search.

use eframe::egui;

use super::icons::{self, Icon};
use crate::ui::{card, theme};

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

/// The page's panel and whether its next step waits on the user.
pub(super) struct Panel {
    line: String,
    detail: String,
    action: &'static str,
    primary: bool,
    enabled: bool,
    dot: egui::Color32,
    progress: Option<f32>,
}

impl Panel {
    pub(super) fn wants_attention(&self) -> bool {
        self.primary
    }
}

pub(super) fn panel(about: &AboutView) -> Panel {
    let v = &about.version;
    let checked = match about.last_checked_unix_millis {
        Some(millis) => format!("Checked {}.", format_last_checked(millis)),
        None => "Not checked yet.".to_owned(),
    };
    let check = |dot, line: String, detail: String, enabled| Panel {
        line,
        detail,
        action: "Check",
        primary: false,
        enabled,
        dot,
        progress: None,
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
        AppUpdateStatus::Available { version } => Panel {
            line: format!("{v} · {version} available"),
            detail: checked,
            action: "Update",
            primary: true,
            enabled: true,
            dot: theme::ACCENT,
            progress: None,
        },
        AppUpdateStatus::Downloading { version, percent } => Panel {
            line: format!("Downloading {version} · {percent}%"),
            detail: "SnappyRef restarts when the download finishes.".to_owned(),
            action: "Update",
            primary: false,
            enabled: false,
            dot: theme::ACCENT,
            progress: Some(f32::from(*percent) / 100.0),
        },
        AppUpdateStatus::Finishing { version } => Panel {
            line: format!("Restarting into {version}"),
            detail: "Your boards reopen where you left them.".to_owned(),
            action: "Update",
            primary: false,
            enabled: false,
            dot: theme::ACCENT,
            progress: None,
        },
        AppUpdateStatus::Ready { version } => Panel {
            line: format!("{version} ready"),
            detail: "Restart to finish the update.".to_owned(),
            action: "Restart",
            primary: true,
            enabled: true,
            dot: theme::ACCENT,
            progress: None,
        },
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

/// Returns true when the update action was clicked.
pub(super) fn show(ui: &mut egui::Ui, panel: &Panel) -> bool {
    card::status_panel(
        ui,
        super::INNER_WIDTH,
        &card::StatusPanel {
            title: "SnappyRef",
            action: panel.action,
            primary: panel.primary,
            enabled: panel.enabled,
            dot: panel.dot,
            line: &panel.line,
            progress: panel.progress,
            detail: &panel.detail,
        },
        |p, center, color| icons::paint(p, center, Icon::About, color),
    )
    .clicked()
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
