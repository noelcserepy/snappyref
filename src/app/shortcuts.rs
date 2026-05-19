use eframe::egui;

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
    MoveSelectionToFront,
    MoveSelectionToBack,
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
    Copy,
    Paste,
    Duplicate,
    DeleteFromBoard,
    MoveToFront,
    MoveToBack,
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
    pub(super) shortcut: Option<&'static str>,
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
    #[allow(dead_code)]
    pub windows: &'static str,
    #[allow(dead_code)]
    pub mac: &'static str,
    pub(super) binding: Option<ShortcutBinding>,
    pub(super) shortcut_action: Option<SupportedShortcutAction>,
}

pub(crate) const SUPPORTED_SHORTCUTS: &[SupportedShortcut] = &[
    SupportedShortcut {
        action: "Copy selection",
        windows: "Ctrl+C",
        mac: "⌘C",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Paste image",
        windows: "Ctrl+V",
        mac: "⌘V",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Search images",
        windows: "Ctrl+F",
        mac: "⌘F",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Crop image",
        windows: "C + Drag",
        mac: "C + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw arrow",
        windows: "A + Drag",
        mac: "A + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw line",
        windows: "L + Drag",
        mac: "L + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw textbox",
        windows: "T + Click/Drag",
        mac: "T + Click/Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Rotate selection",
        windows: "R + Drag",
        mac: "R + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Scale selection",
        windows: "Ctrl+Alt+Drag",
        mac: "⌘⌥ Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Focus selection",
        windows: "Space",
        mac: "Space",
        binding: Some(ShortcutBinding {
            key: egui::Key::Space,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::FocusSelection),
    },
    SupportedShortcut {
        action: "Delete selection",
        windows: "Delete",
        mac: "Backspace",
        binding: None,
        shortcut_action: Some(SupportedShortcutAction::DeleteSelection),
    },
    SupportedShortcut {
        action: "Duplicate selection",
        windows: "Ctrl+D",
        mac: "⌘D",
        binding: Some(ShortcutBinding {
            key: egui::Key::D,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DuplicateSelection),
    },
    SupportedShortcut {
        action: "Move selection to front",
        windows: "Ctrl+.",
        mac: "⌘.",
        binding: Some(ShortcutBinding {
            key: egui::Key::Period,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToFront),
    },
    SupportedShortcut {
        action: "Move selection to back",
        windows: "Ctrl+,",
        mac: "⌘,",
        binding: Some(ShortcutBinding {
            key: egui::Key::Comma,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToBack),
    },
    SupportedShortcut {
        action: "Toggle sidebar",
        windows: "Ctrl+B",
        mac: "⌘B",
        binding: Some(ShortcutBinding {
            key: egui::Key::B,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleSidebar),
    },
    SupportedShortcut {
        action: "Toggle always on top",
        windows: "Ctrl+Shift+T",
        mac: "⇧⌘T",
        binding: Some(ShortcutBinding {
            key: egui::Key::T,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleAlwaysOnTop),
    },
    SupportedShortcut {
        action: "Compact arrange",
        windows: "Q",
        mac: "Q",
        binding: Some(ShortcutBinding {
            key: egui::Key::Q,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(ArrangementAction::Compact)),
    },
    SupportedShortcut {
        action: "Row arrange",
        windows: "W",
        mac: "W",
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
        windows: "E",
        mac: "E",
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
        windows: "Arrow keys",
        mac: "Arrow keys",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Dedupe library",
        windows: "P",
        mac: "P",
        binding: Some(ShortcutBinding {
            key: egui::Key::P,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DedupeLibrary),
    },
    SupportedShortcut {
        action: "Default avg area",
        windows: "Ctrl+L",
        mac: "⌘L",
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ApplyDefaultAverageArea),
    },
    SupportedShortcut {
        action: "Selection avg area",
        windows: "Ctrl+Shift+L",
        mac: "⇧⌘L",
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::AverageSelectionArea),
    },
    SupportedShortcut {
        action: "Undo",
        windows: "Ctrl+Z",
        mac: "⌘Z",
        binding: Some(ShortcutBinding {
            key: egui::Key::Z,
            command: true,
            shift: false,
        }),
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
