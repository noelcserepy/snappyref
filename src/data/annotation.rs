use eframe::egui::{Pos2, Rect};

use super::{RectGeometry, SegmentGeometry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasAnnotationKind {
    Line,
    Arrow,
    /// Shown as a label chip sized to its text.
    Textbox,
    /// A bordered region that calls out part of an image.
    Rectangle,
}

impl CanvasAnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Textbox => "textbox",
            Self::Rectangle => "rectangle",
        }
    }

    /// Rect kinds store [`CanvasAnnotationGeometry::Rect`] and move as boxes.
    pub fn is_rect(self) -> bool {
        matches!(self, Self::Textbox | Self::Rectangle)
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "line" => Some(Self::Line),
            "arrow" => Some(Self::Arrow),
            "textbox" => Some(Self::Textbox),
            "rectangle" => Some(Self::Rectangle),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasTextSize {
    S,
    #[default]
    M,
    L,
    Xl,
    Xxl,
}

impl CanvasTextSize {
    /// In key order: Shift+1 is S, Shift+5 is Xxl.
    pub const ALL: [Self; 5] = [Self::S, Self::M, Self::L, Self::Xl, Self::Xxl];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
            Self::Xl => "xl",
            Self::Xxl => "xxl",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|size| size.as_str() == value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasStrokeWidth {
    S,
    #[default]
    M,
    L,
    Xl,
    Xxl,
}

impl CanvasStrokeWidth {
    /// In key order: Shift+1 is S, Shift+5 is Xxl.
    pub const ALL: [Self; 5] = [Self::S, Self::M, Self::L, Self::Xl, Self::Xxl];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
            Self::Xl => "xl",
            Self::Xxl => "xxl",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|width| width.as_str() == value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasAnnotationColor {
    #[default]
    Default,
    Coral,
    Gold,
    Mint,
    Sky,
}

impl CanvasAnnotationColor {
    /// In key order: 1 is Default, 5 is Sky.
    pub const ALL: [Self; 5] = [
        Self::Default,
        Self::Coral,
        Self::Gold,
        Self::Mint,
        Self::Sky,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Coral => "coral",
            Self::Gold => "gold",
            Self::Mint => "mint",
            Self::Sky => "sky",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "coral" => Some(Self::Coral),
            "gold" => Some(Self::Gold),
            "mint" => Some(Self::Mint),
            "sky" => Some(Self::Sky),
            _ => None,
        }
    }
}

/// What a rectangle shows inside its border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasRectangleFill {
    None,
    #[default]
    Tint,
    Solid,
}

impl CanvasRectangleFill {
    /// In the order F steps through.
    pub const ALL: [Self; 3] = [Self::None, Self::Tint, Self::Solid];

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Tint,
            Self::Tint => Self::Solid,
            Self::Solid => Self::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tint => "tint",
            Self::Solid => "solid",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|fill| fill.as_str() == value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasAnnotation {
    pub id: i64,
    pub kind: CanvasAnnotationKind,
    pub geometry: CanvasAnnotationGeometry,
    pub text: String,
    pub text_size: CanvasTextSize,
    pub stroke_width: CanvasStrokeWidth,
    pub color: CanvasAnnotationColor,
    /// Only a rectangle shows it.
    pub fill: CanvasRectangleFill,
    pub archived: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasAnnotationGeometry {
    Rect(RectGeometry),
    Segment(SegmentGeometry),
}

impl CanvasAnnotationGeometry {
    pub fn from_kind(kind: CanvasAnnotationKind, start: Pos2, end: Pos2) -> Self {
        match kind {
            CanvasAnnotationKind::Textbox | CanvasAnnotationKind::Rectangle => {
                Self::Rect(RectGeometry::from_bounds(Rect::from_two_pos(start, end), 0))
            }
            CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
                Self::Segment(SegmentGeometry::new(start, end, 0))
            }
        }
    }

    pub fn start(self) -> Pos2 {
        match self {
            Self::Rect(geometry) => geometry.unrotated_bounds().min,
            Self::Segment(geometry) => geometry.start,
        }
    }

    pub fn end(self) -> Pos2 {
        match self {
            Self::Rect(geometry) => geometry.unrotated_bounds().max,
            Self::Segment(geometry) => geometry.end,
        }
    }
}

impl CanvasAnnotation {
    pub fn new(
        id: i64,
        kind: CanvasAnnotationKind,
        start: Pos2,
        end: Pos2,
        text: String,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
        color: CanvasAnnotationColor,
        archived: bool,
    ) -> Self {
        Self {
            id,
            kind,
            geometry: CanvasAnnotationGeometry::from_kind(kind, start, end),
            text,
            text_size,
            stroke_width,
            color,
            fill: CanvasRectangleFill::default(),
            archived,
        }
    }

    pub fn start(&self) -> Pos2 {
        self.geometry.start()
    }

    pub fn end(&self) -> Pos2 {
        self.geometry.end()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasBackground {
    Dots,
    Grid,
    #[default]
    None,
}

impl CanvasBackground {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dots => "dots",
            Self::Grid => "grid",
            Self::None => "none",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "dots" => Some(Self::Dots),
            "grid" => Some(Self::Grid),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}
