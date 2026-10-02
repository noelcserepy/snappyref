use crate::ui::sidebar::SidebarTab;

pub(super) fn sidebar_tab_session_value(tab: SidebarTab) -> &'static str {
    match tab {
        SidebarTab::Boards => "boards",
        SidebarTab::Settings => "settings",
    }
}

pub(super) fn sidebar_tab_from_session(value: &str) -> SidebarTab {
    match value {
        "settings" => SidebarTab::Settings,
        _ => SidebarTab::Boards,
    }
}
