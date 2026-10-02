use eframe::egui;

use crate::ui::shortcut::{Key, Shortcut};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArrangementAction {
    Compact,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SupportedShortcutAction {
    DeleteSelection,
    DuplicateSelection,
    BringSelectionToFront,
    SendSelectionToBack,
    ToggleSidebar,
    ToggleAlwaysOnTop,
    FocusSelection,
    Arrange(ArrangementAction),
    ApplyDefaultAverageArea,
    AverageSelectionArea,
    DedupeLibrary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContextMenuAction {
    Group,
    Ungroup,
    AddToGroup,
    Copy,
    Paste,
    Duplicate,
    DeleteFromBoard,
    BringToFront,
    SendToBack,
    SearchUsingImage,
    RemoveFromLibrary,
}

impl ContextMenuAction {
    pub(super) fn is_destructive(self) -> bool {
        matches!(self, Self::DeleteFromBoard | Self::RemoveFromLibrary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ContextMenuItem {
    pub(super) action: ContextMenuAction,
    pub(super) label: &'static str,
    pub(super) shortcut: Option<Shortcut>,
    pub(super) enabled: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ShortcutBinding {
    key: egui::Key,
    command: bool,
    shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingBoardDelete {
    pub(super) board_id: i64,
    pub(super) board_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingLibraryRemove {
    pub(super) asset_ids: Vec<i64>,
}

#[derive(Clone, Copy)]
pub(crate) struct SupportedShortcut {
    pub action: &'static str,
    pub keys: Shortcut,
    pub(super) binding: Option<ShortcutBinding>,
    pub(super) shortcut_action: Option<SupportedShortcutAction>,
}

// Shortcuts shown in more than one place: the sidebar list, the context
// menu, the Group card, and the sidebar edge.
pub(crate) const GROUP: Shortcut = &[Key::Command, Key::Named("G")];
pub(crate) const UNGROUP: Shortcut = &[Key::Command, Key::Shift, Key::Named("G")];
pub(crate) const ADD_TO_GROUP: Shortcut = &[Key::Named("G")];
pub(crate) const COPY: Shortcut = &[Key::Command, Key::Named("C")];
pub(crate) const PASTE: Shortcut = &[Key::Command, Key::Named("V")];
pub(crate) const SEARCH_IMAGES: Shortcut = &[Key::Command, Key::Named("F")];
pub(crate) const DELETE: Shortcut = &[Key::Delete];
pub(crate) const DUPLICATE: Shortcut = &[Key::Command, Key::Named("D")];
pub(crate) const BRING_TO_FRONT: Shortcut = &[Key::Command, Key::Named(".")];
pub(crate) const SEND_TO_BACK: Shortcut = &[Key::Command, Key::Named(",")];
pub(crate) const TOGGLE_SIDEBAR: Shortcut = &[Key::Command, Key::Named("B")];

pub(crate) const NEW_BOARD: Shortcut = &[Key::Command, Key::Named("N")];
pub(crate) const TOGGLE_ALWAYS_ON_TOP: Shortcut = &[Key::Command, Key::Shift, Key::Named("T")];
/// Command plus a board's digit switches to it. The Boards header shows
/// this modifier; each row's digit cap shows the rest.
pub(crate) const SWITCH_BOARD: Shortcut = &[Key::Command];
/// Board N's digit in sidebar order; the tenth board takes 0.
pub(crate) const BOARD_DIGITS: [Key; 10] = [
    Key::Named("1"),
    Key::Named("2"),
    Key::Named("3"),
    Key::Named("4"),
    Key::Named("5"),
    Key::Named("6"),
    Key::Named("7"),
    Key::Named("8"),
    Key::Named("9"),
    Key::Named("0"),
];

pub(crate) const CORE_SHORTCUTS: &[SupportedShortcut] = &[
    SupportedShortcut {
        action: "Group selection",
        keys: GROUP,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Ungroup selection",
        keys: UNGROUP,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Add to group",
        keys: ADD_TO_GROUP,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Copy selection",
        keys: COPY,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Paste image",
        keys: PASTE,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Search images",
        keys: SEARCH_IMAGES,
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Crop image",
        keys: &[Key::Named("C"), Key::Gesture("Drag")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw arrow",
        keys: &[Key::Named("A"), Key::Gesture("Drag")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw line",
        keys: &[Key::Named("L"), Key::Gesture("Drag")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw textbox",
        keys: &[Key::Named("T"), Key::Gesture("Click/Drag")],
        binding: None,
        shortcut_action: None,
    },
];

pub(crate) const LAYOUT_AND_HISTORY_SHORTCUTS: &[SupportedShortcut] = &[
    SupportedShortcut {
        action: "Rotate selection",
        keys: &[Key::Named("R"), Key::Gesture("Drag")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Scale selection",
        keys: &[Key::Command, Key::Alt, Key::Gesture("Drag")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Focus selection",
        keys: &[Key::Named("Space")],
        binding: Some(ShortcutBinding {
            key: egui::Key::Space,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::FocusSelection),
    },
    SupportedShortcut {
        action: "Delete selection",
        keys: DELETE,
        binding: None,
        shortcut_action: Some(SupportedShortcutAction::DeleteSelection),
    },
    SupportedShortcut {
        action: "Duplicate selection",
        keys: DUPLICATE,
        binding: Some(ShortcutBinding {
            key: egui::Key::D,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DuplicateSelection),
    },
    SupportedShortcut {
        action: "Bring selection to front",
        keys: BRING_TO_FRONT,
        binding: Some(ShortcutBinding {
            key: egui::Key::Period,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::BringSelectionToFront),
    },
    SupportedShortcut {
        action: "Send selection to back",
        keys: SEND_TO_BACK,
        binding: Some(ShortcutBinding {
            key: egui::Key::Comma,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::SendSelectionToBack),
    },
    SupportedShortcut {
        action: "Toggle sidebar",
        keys: TOGGLE_SIDEBAR,
        binding: Some(ShortcutBinding {
            key: egui::Key::B,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleSidebar),
    },
    SupportedShortcut {
        action: "Toggle always on top",
        keys: TOGGLE_ALWAYS_ON_TOP,
        binding: Some(ShortcutBinding {
            key: egui::Key::T,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleAlwaysOnTop),
    },
    SupportedShortcut {
        action: "Compact arrange",
        keys: &[Key::Named("Q")],
        binding: Some(ShortcutBinding {
            key: egui::Key::Q,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(ArrangementAction::Compact)),
    },
    SupportedShortcut {
        action: "Row arrange",
        keys: &[Key::Named("W")],
        binding: Some(ShortcutBinding {
            key: egui::Key::W,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(
            ArrangementAction::Horizontal,
        )),
    },
    SupportedShortcut {
        action: "Column arrange",
        keys: &[Key::Named("E")],
        binding: Some(ShortcutBinding {
            key: egui::Key::E,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(
            ArrangementAction::Vertical,
        )),
    },
    SupportedShortcut {
        action: "Directional settle arrange",
        keys: &[Key::Named("Arrow keys")],
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Dedupe library",
        keys: &[Key::Named("P")],
        binding: Some(ShortcutBinding {
            key: egui::Key::P,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DedupeLibrary),
    },
    SupportedShortcut {
        action: "Default avg area",
        keys: &[Key::Command, Key::Named("L")],
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ApplyDefaultAverageArea),
    },
    SupportedShortcut {
        action: "Selection avg area",
        keys: &[Key::Command, Key::Shift, Key::Named("L")],
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::AverageSelectionArea),
    },
    SupportedShortcut {
        action: "Undo",
        keys: &[Key::Command, Key::Named("Z")],
        binding: None,
        shortcut_action: None,
    },
];

pub(super) fn shortcut_pressed(ctx: &egui::Context, binding: ShortcutBinding) -> bool {
    ctx.input(|input| {
        input.modifiers.command == binding.command
            && input.modifiers.shift == binding.shift
            && input.key_pressed(binding.key)
    })
}

pub(super) fn delete_shortcut_pressed(ctx: &egui::Context) -> bool {
    #[cfg(target_os = "macos")]
    {
        ctx.input(|input| input.key_pressed(egui::Key::Backspace))
    }

    #[cfg(not(target_os = "macos"))]
    {
        ctx.input(|input| input.key_pressed(egui::Key::Delete))
    }
}

pub(super) fn settle_direction_pressed(
    ctx: &egui::Context,
) -> Option<crate::placement::arrangement::SettleDirection> {
    ctx.input(|input| {
        if input.modifiers != egui::Modifiers::NONE {
            return None;
        }
        if input.key_pressed(egui::Key::ArrowRight) {
            Some(crate::placement::arrangement::SettleDirection::Right)
        } else if input.key_pressed(egui::Key::ArrowLeft) {
            Some(crate::placement::arrangement::SettleDirection::Left)
        } else if input.key_pressed(egui::Key::ArrowDown) {
            Some(crate::placement::arrangement::SettleDirection::Down)
        } else if input.key_pressed(egui::Key::ArrowUp) {
            Some(crate::placement::arrangement::SettleDirection::Up)
        } else {
            None
        }
    })
}
