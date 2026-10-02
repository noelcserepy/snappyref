//! Shortcuts as data, drawn one chip per key.
//!
//! A shortcut is a list of [`Key`]s that names roles, not glyphs:
//! [`Key::Command`] reads ⌘ on macOS and Ctrl elsewhere, and each platform
//! orders its modifiers its own way. Every on-screen shortcut goes through
//! [`paint`], [`show`], [`hint`], or [`cap`]; tooltips spell it with [`text`].

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, vec2};

use super::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// ⌘ on macOS, Ctrl elsewhere.
    Command,
    Alt,
    Shift,
    /// ⌫ (Backspace) on macOS, Del elsewhere.
    Delete,
    Enter,
    Up,
    Down,
    /// A key named by its cap: a letter, digit, punctuation mark, or word.
    Named(&'static str),
    /// A pointer gesture held with the keys, or a family of keys such as
    /// "digit". It follows a bare "+", unchipped.
    Gesture(&'static str),
}

pub(crate) type Shortcut = &'static [Key];

const MAC: bool = cfg!(target_os = "macos");
const HEIGHT: f32 = 18.0;
const PAD: f32 = 5.0;
const GAP: f32 = 3.0;
/// Hint labels sit this far after their keys.
const LABEL_GAP: f32 = 6.0;
/// Hints in a row sit this far apart.
pub(crate) const HINT_GAP: f32 = 14.0;

impl Key {
    /// macOS reads ⌥⇧⌘, Windows Ctrl+Alt+Shift. Keys follow modifiers.
    fn rank(self) -> u8 {
        match (self, MAC) {
            (Key::Alt, true) | (Key::Command, false) => 0,
            (Key::Shift, true) | (Key::Alt, false) => 1,
            (Key::Command, true) | (Key::Shift, false) => 2,
            _ => 3,
        }
    }

    /// Keys drawn as path glyphs on a square chip, because the bundled fonts
    /// lack them or set them low and thin next to DM Sans.
    fn drawn(self) -> bool {
        matches!(self, Key::Enter | Key::Up | Key::Down) || (MAC && self == Key::Delete)
    }

    fn label(self) -> &'static str {
        match self {
            Key::Command if MAC => "⌘",
            Key::Command => "Ctrl",
            Key::Alt if MAC => "⌥",
            Key::Alt => "Alt",
            Key::Shift if MAC => "⇧",
            Key::Shift => "Shift",
            Key::Delete if MAC => "⌫",
            Key::Delete => "Del",
            Key::Enter => "↵",
            Key::Up => "↑",
            Key::Down => "↓",
            Key::Named(name) | Key::Gesture(name) => name,
        }
    }
}

fn ordered(keys: &[Key]) -> Vec<Key> {
    let mut keys = keys.to_vec();
    keys.sort_by_key(|key| key.rank());
    keys
}

/// The shortcut as one line of text: "⇧⌘G" on macOS, "Ctrl+Shift+G" elsewhere.
pub(crate) fn text(keys: &[Key]) -> String {
    let keys = ordered(keys);
    let chord = keys
        .iter()
        .filter(|key| !matches!(key, Key::Gesture(_)))
        .map(|key| key.label())
        .collect::<Vec<_>>()
        .join(if MAC { "" } else { "+" });
    keys.iter().fold(chord, |text, key| match key {
        Key::Gesture(gesture) => format!("{text} + {gesture}"),
        _ => text,
    })
}

fn font() -> FontId {
    FontId::proportional(theme::TYPE_CHIP)
}

/// The macOS modifier symbols come from a monospace subset that sets them
/// smaller than DM Sans capitals, so they draw two points up to match.
fn key_font(key: Key) -> FontId {
    if MAC && matches!(key, Key::Command | Key::Alt | Key::Shift) {
        FontId::proportional(theme::TYPE_CONTROL)
    } else {
        font()
    }
}

fn gesture_text(gesture: &str) -> String {
    format!("+ {gesture}")
}

fn key_width(painter: &egui::Painter, key: Key) -> f32 {
    let text_width = |text: String, font| painter.layout_no_wrap(text, font, theme::TEXT).size().x;
    match key {
        key if key.drawn() => HEIGHT,
        Key::Gesture(gesture) => text_width(gesture_text(gesture), font()),
        key => (text_width(key.label().to_owned(), key_font(key)) + 2.0 * PAD).max(HEIGHT),
    }
}

/// Width of the chips for `keys`.
pub(crate) fn width(painter: &egui::Painter, keys: &[Key]) -> f32 {
    let widths: f32 = keys.iter().map(|&key| key_width(painter, key)).sum();
    widths + GAP * keys.len().saturating_sub(1) as f32
}

/// Paints `keys` as chips placed at `pos` by `align`, and returns their rect.
/// `ink` colors the key glyphs; callers fade disabled shortcuts through the
/// painter's opacity so the chip fill fades with them.
pub(crate) fn paint(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    keys: &[Key],
    ink: Color32,
) -> Rect {
    let rect = align.anchor_size(pos, vec2(width(painter, keys), HEIGHT));
    let mut left = rect.left();
    for key in ordered(keys) {
        let chip = Rect::from_min_size(
            egui::pos2(left, rect.top()),
            vec2(key_width(painter, key), HEIGHT),
        );
        left = chip.right() + GAP;
        if let Key::Gesture(gesture) = key {
            painter.text(
                chip.left_center(),
                Align2::LEFT_CENTER,
                gesture_text(gesture),
                font(),
                ink,
            );
            continue;
        }
        paint_chip(painter, chip, key, theme::SURFACE_RAISED, ink);
    }
    rect
}

/// One key alone, centered at `center`: the key a row answers to. A lit cap
/// fills gold, marking the row the user is on.
pub(crate) fn cap(painter: &egui::Painter, center: Pos2, key: Key, lit: bool) -> Rect {
    let chip = Rect::from_center_size(center, vec2(key_width(painter, key), HEIGHT));
    let (fill, ink) = if lit {
        (theme::ACCENT, theme::BACKGROUND)
    } else {
        (theme::SURFACE_RAISED, theme::TEXT_MUTED)
    };
    paint_chip(painter, chip, key, fill, ink);
    chip
}

fn paint_chip(painter: &egui::Painter, chip: Rect, key: Key, fill: Color32, ink: Color32) {
    painter.rect_filled(chip, theme::RADIUS_INLINE, fill);
    if key.drawn() {
        paint_glyph(painter, chip.center(), key, ink);
    } else {
        let galley = painter.layout_no_wrap(key.label().to_owned(), key_font(key), ink);
        let pos = chip.center() - label_center(&galley, key);
        painter.galley(pos, galley, ink);
    }
}

/// The point of a key's label that sits on its chip's center. Letters and
/// words center on the line box, so every chip shares one baseline.
/// Punctuation sits on that baseline with no cap height above it, so it
/// centers on its own ink instead.
fn label_center(galley: &egui::Galley, key: Key) -> egui::Vec2 {
    let ink = galley
        .rows
        .iter()
        .flat_map(|row| {
            row.row.glyphs.iter().map(move |glyph| {
                Rect::from_min_size(
                    row.pos + glyph.pos.to_vec2() + glyph.uv_rect.offset,
                    glyph.uv_rect.size,
                )
            })
        })
        .reduce(Rect::union);
    match ink {
        Some(ink) if key.label().chars().all(|c| c.is_ascii_punctuation()) => {
            ink.center().to_vec2()
        }
        _ => galley.size() * 0.5,
    }
}

/// Allocates and paints `keys` in muted ink.
pub(crate) fn show(ui: &mut egui::Ui, keys: &[Key]) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(width(ui.painter(), keys), HEIGHT), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text(keys)));
    paint(
        ui.painter(),
        rect.left_center(),
        Align2::LEFT_CENTER,
        keys,
        theme::TEXT_MUTED,
    );
    response
}

/// A key hint: the chips, then what they do.
pub(crate) fn hint(ui: &mut egui::Ui, keys: &[Key], label: &str) -> Response {
    let font = FontId::proportional(theme::TYPE_DETAIL);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme::TEXT_MUTED);
    let keys_width = width(ui.painter(), keys);
    let (rect, response) = ui.allocate_exact_size(
        vec2(keys_width + LABEL_GAP + galley.size().x, HEIGHT),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    paint(
        ui.painter(),
        rect.left_center(),
        Align2::LEFT_CENTER,
        keys,
        theme::TEXT_MUTED,
    );
    let label_pos = egui::pos2(
        rect.left() + keys_width + LABEL_GAP,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(label_pos, galley, theme::TEXT_MUTED);
    response
}

/// Drawn keys sit on a 16 unit grid at 10 points.
fn paint_glyph(painter: &egui::Painter, center: Pos2, key: Key, color: Color32) {
    let size = 10.0;
    let origin = center - vec2(size, size) * 0.5;
    let point = |x: f32, y: f32| origin + vec2(x, y) * (size / 16.0);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            Stroke::new(1.3, color),
        ));
    };
    match key {
        Key::Enter => {
            line(&[(13.0, 2.0), (13.0, 10.0), (3.0, 10.0)]);
            line(&[(6.5, 6.5), (3.0, 10.0), (6.5, 13.5)]);
        }
        Key::Up => {
            line(&[(8.0, 14.0), (8.0, 2.0)]);
            line(&[(3.5, 6.5), (8.0, 2.0), (12.5, 6.5)]);
        }
        Key::Down => {
            line(&[(8.0, 2.0), (8.0, 14.0)]);
            line(&[(3.5, 9.5), (8.0, 14.0), (12.5, 9.5)]);
        }
        Key::Delete => {
            line(&[
                (5.5, 3.0),
                (15.0, 3.0),
                (15.0, 13.0),
                (5.5, 13.0),
                (1.0, 8.0),
                (5.5, 3.0),
            ]);
            line(&[(8.0, 5.5), (12.0, 10.5)]);
            line(&[(12.0, 5.5), (8.0, 10.5)]);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, text};

    #[test]
    fn text_orders_modifiers_for_the_platform() {
        let ungroup = [Key::Command, Key::Shift, Key::Named("G")];
        let crop = [Key::Named("C"), Key::Gesture("Drag")];
        let scale = [Key::Command, Key::Alt, Key::Gesture("Drag")];
        if cfg!(target_os = "macos") {
            assert_eq!(text(&ungroup), "⇧⌘G");
            assert_eq!(text(&scale), "⌥⌘ + Drag");
        } else {
            assert_eq!(text(&ungroup), "Ctrl+Shift+G");
            assert_eq!(text(&scale), "Ctrl+Alt + Drag");
        }
        assert_eq!(text(&crop), "C + Drag");
    }
}
