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
    Group,
    Ungroup,
    AddToGroup,
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
    /// Key label for the running platform; see `platform_shortcut_label`.
    pub keys: &'static str,
    pub(super) binding: Option<ShortcutBinding>,
    pub(super) shortcut_action: Option<SupportedShortcutAction>,
}

/// Picks the shortcut label for the running platform: macOS symbol glyphs
/// (`⌘V`), `Ctrl`-style text everywhere else.
pub(crate) const fn platform_shortcut_label(
    mac: &'static str,
    windows: &'static str,
) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        windows
    }
}

pub(crate) const SUPPORTED_SHORTCUTS: &[SupportedShortcut] = &[
    SupportedShortcut {
        action: "Group selection",
        keys: platform_shortcut_label("⌘G", "Ctrl+G"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Ungroup selection",
        keys: platform_shortcut_label("⌘⇧G", "Ctrl+Shift+G"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Add to group",
        keys: "G",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Copy selection",
        keys: platform_shortcut_label("⌘C", "Ctrl+C"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Paste image",
        keys: platform_shortcut_label("⌘V", "Ctrl+V"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Search images",
        keys: platform_shortcut_label("⌘F", "Ctrl+F"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Crop image",
        keys: "C + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw arrow",
        keys: "A + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw line",
        keys: "L + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw textbox",
        keys: "T + Click/Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Rotate selection",
        keys: "R + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Scale selection",
        keys: platform_shortcut_label("⌘⌥ Drag", "Ctrl+Alt+Drag"),
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Focus selection",
        keys: "Space",
        binding: Some(ShortcutBinding {
            key: egui::Key::Space,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::FocusSelection),
    },
    SupportedShortcut {
        action: "Delete selection",
        keys: platform_shortcut_label("Backspace", "Delete"),
        binding: None,
        shortcut_action: Some(SupportedShortcutAction::DeleteSelection),
    },
    SupportedShortcut {
        action: "Duplicate selection",
        keys: platform_shortcut_label("⌘D", "Ctrl+D"),
        binding: Some(ShortcutBinding {
            key: egui::Key::D,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DuplicateSelection),
    },
    SupportedShortcut {
        action: "Move selection to front",
        keys: platform_shortcut_label("⌘.", "Ctrl+."),
        binding: Some(ShortcutBinding {
            key: egui::Key::Period,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToFront),
    },
    SupportedShortcut {
        action: "Move selection to back",
        keys: platform_shortcut_label("⌘,", "Ctrl+,"),
        binding: Some(ShortcutBinding {
            key: egui::Key::Comma,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToBack),
    },
    SupportedShortcut {
        action: "Toggle sidebar",
        keys: platform_shortcut_label("⌘B", "Ctrl+B"),
        binding: Some(ShortcutBinding {
            key: egui::Key::B,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleSidebar),
    },
    SupportedShortcut {
        action: "Toggle always on top",
        keys: platform_shortcut_label("⇧⌘T", "Ctrl+Shift+T"),
        binding: Some(ShortcutBinding {
            key: egui::Key::T,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleAlwaysOnTop),
    },
    SupportedShortcut {
        action: "Compact arrange",
        keys: "Q",
        binding: Some(ShortcutBinding {
            key: egui::Key::Q,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(ArrangementAction::Compact)),
    },
    SupportedShortcut {
        action: "Row arrange",
        keys: "W",
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
        keys: "E",
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
        keys: "Arrow keys",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Dedupe library",
        keys: "P",
        binding: Some(ShortcutBinding {
            key: egui::Key::P,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DedupeLibrary),
    },
    SupportedShortcut {
        action: "Default avg area",
        keys: platform_shortcut_label("⌘L", "Ctrl+L"),
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ApplyDefaultAverageArea),
    },
    SupportedShortcut {
        action: "Selection avg area",
        keys: platform_shortcut_label("⇧⌘L", "Ctrl+Shift+L"),
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::AverageSelectionArea),
    },
    SupportedShortcut {
        action: "Undo",
        keys: platform_shortcut_label("⌘Z", "Ctrl+Z"),
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
