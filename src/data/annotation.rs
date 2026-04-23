#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasAnnotationKind {
    Line,
    Arrow,
    Textbox,
}

impl CanvasAnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Textbox => "textbox",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "line" => Some(Self::Line),
            "arrow" => Some(Self::Arrow),
            "textbox" => Some(Self::Textbox),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CanvasTextSize {
    S,
    M,
    L,
    Xl,
}

impl CanvasTextSize {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
            Self::Xl => "xl",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "s" => Some(Self::S),
            "m" => Some(Self::M),
            "l" => Some(Self::L),
            "xl" => Some(Self::Xl),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
            Self::Xl => "XL",
        }
    }
}

impl Default for CanvasTextSize {
    fn default() -> Self {
        Self::M
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CanvasStrokeWidth {
    S,
    M,
    L,
}

impl CanvasStrokeWidth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "s" => Some(Self::S),
            "m" => Some(Self::M),
            "l" => Some(Self::L),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
        }
    }
}

impl Default for CanvasStrokeWidth {
    fn default() -> Self {
        Self::M
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CanvasAnnotationColor {
    Default,
    Coral,
    Gold,
    Mint,
    Sky,
}

impl CanvasAnnotationColor {
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

impl Default for CanvasAnnotationColor {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasAnnotation {
    pub id: i64,
    pub kind: CanvasAnnotationKind,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub text: String,
    pub text_size: CanvasTextSize,
    pub stroke_width: CanvasStrokeWidth,
    pub color: CanvasAnnotationColor,
    pub background_enabled: bool,
    pub archived: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasBackground {
    Dots,
    Grid,
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

impl Default for CanvasBackground {
    fn default() -> Self {
        Self::None
    }
}
