//! PureRef `.pur` boards: reads 1.x and 2.x files and writes 2.x files.
//! Pure data in and out; byte layouts follow PureRef's Qt serialization.

mod qt_stream;
mod read_v1;
mod read_v2;
#[cfg(test)]
mod tests;
mod write_v2;

pub use write_v2::write_board;

/// Pixel size of written note text, before the note transform scales it.
pub const NOTE_FONT_PX: f32 = 22.0;

/// A PureRef board flattened to scene space. Groups are flattened: child transforms
/// are composed with their parents'.
#[derive(Clone, Debug, PartialEq)]
pub struct PureRefBoard {
    /// Shared image resources. Duplicated items point at the same index.
    pub images: Vec<PureRefImage>,
    /// Back-to-front stacking order.
    pub image_items: Vec<PureRefImageItem>,
    /// Back-to-front stacking order.
    pub notes: Vec<PureRefNote>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PureRefImage {
    /// Original encoded file bytes when embedded; `None` when the board only links the image.
    pub bytes: Option<Vec<u8>>,
    /// Original path or URL, if known.
    pub source: Option<String>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PureRefImageItem {
    /// Index into [`PureRefBoard::images`].
    pub image: usize,
    pub name: Option<String>,
    /// Maps image pixel coordinates (origin at the top-left of the full, uncropped image,
    /// y down) to scene coordinates.
    pub transform: PureRefTransform,
    /// Visible part of the image in pixel coordinates. The full image when uncropped.
    pub crop: PureRefPixelRect,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PureRefNote {
    pub text: String,
    /// Maps note-local coordinates to scene coordinates. PureRef centres the note box on the
    /// local origin.
    pub transform: PureRefTransform,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PureRefPixelRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A 2D affine `QTransform` in Qt's row-vector convention:
/// `x' = m11*x + m21*y + dx`, `y' = m12*x + m22*y + dy`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PureRefTransform {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub dx: f64,
    pub dy: f64,
}

impl PureRefTransform {
    pub const IDENTITY: Self = Self::translation(0.0, 0.0);

    pub const fn translation(dx: f64, dy: f64) -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            dx,
            dy,
        }
    }

    pub const fn scale(factor: f64) -> Self {
        Self {
            m11: factor,
            m22: factor,
            ..Self::IDENTITY
        }
    }

    /// The transform that applies `self` first and `next` second (Qt's `self * next`).
    pub fn then(self, next: Self) -> Self {
        Self {
            m11: self.m11 * next.m11 + self.m12 * next.m21,
            m12: self.m11 * next.m12 + self.m12 * next.m22,
            m21: self.m21 * next.m11 + self.m22 * next.m21,
            m22: self.m21 * next.m12 + self.m22 * next.m22,
            dx: self.dx * next.m11 + self.dy * next.m21 + next.dx,
            dy: self.dx * next.m12 + self.dy * next.m22 + next.dy,
        }
    }

    pub fn map_point(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.m11 * x + self.m21 * y + self.dx,
            self.m12 * x + self.m22 * y + self.dy,
        )
    }

    pub fn inverse(self) -> Option<Self> {
        let determinant = self.m11 * self.m22 - self.m12 * self.m21;
        if determinant == 0.0 || !determinant.is_finite() {
            return None;
        }
        Some(Self {
            m11: self.m22 / determinant,
            m12: -self.m12 / determinant,
            m21: -self.m21 / determinant,
            m22: self.m11 / determinant,
            dx: (self.m21 * self.dy - self.m22 * self.dx) / determinant,
            dy: (self.m12 * self.dx - self.m11 * self.dy) / determinant,
        })
    }
}

/// Reads a 1.x or 2.x board. The first string in the file is the format version.
pub fn read_board(bytes: &[u8]) -> Result<PureRefBoard, String> {
    let format = qt_stream::QtReader::new(bytes)
        .string()?
        .ok_or("PureRef file has no format version")?;
    let major = format
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok());
    match major {
        Some(0 | 1) => read_v1::read(bytes),
        Some(2) => read_v2::read(bytes, &format),
        _ => Err(format!("unsupported PureRef format version {format:?}")),
    }
}

/// One item as stored in the file, before groups are flattened.
struct StoredItem {
    /// Index of the parent in the same list.
    parent: Option<usize>,
    z: f64,
    sort_order: f64,
    /// Maps item-local coordinates to the parent's coordinates.
    transform: PureRefTransform,
    opacity: f32,
    content: StoredContent,
}

enum StoredContent {
    Image {
        image: usize,
        name: Option<String>,
        /// Maps image pixels to item-local coordinates.
        image_transform: PureRefTransform,
        /// Crop path in item-local coordinates. Empty means uncropped.
        bounds: Vec<(f64, f64)>,
    },
    Note {
        text: String,
    },
    /// Groups, drawings and unknown kinds. Only their transform and children matter.
    Container,
}

/// Flattens the item tree in Qt stacking order: siblings back to front by z, then sort
/// order, and every child above its parent.
fn flatten(images: Vec<PureRefImage>, items: Vec<StoredItem>) -> PureRefBoard {
    let mut children = vec![Vec::new(); items.len()];
    let mut roots = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item.parent {
            Some(parent) => children[parent].push(index),
            None => roots.push(index),
        }
    }
    let stacking = |a: &usize, b: &usize| {
        let (first, second) = (&items[*a], &items[*b]);
        first
            .z
            .total_cmp(&second.z)
            .then(first.sort_order.total_cmp(&second.sort_order))
            .then(a.cmp(b))
    };
    roots.sort_by(stacking);
    for siblings in &mut children {
        siblings.sort_by(stacking);
    }

    let mut board = PureRefBoard {
        images,
        image_items: Vec::new(),
        notes: Vec::new(),
    };
    let mut pending: Vec<_> = roots
        .iter()
        .rev()
        .map(|&index| (index, PureRefTransform::IDENTITY, 1.0))
        .collect();
    while let Some((index, parent_transform, parent_opacity)) = pending.pop() {
        let item = &items[index];
        let transform = item.transform.then(parent_transform);
        let opacity = item.opacity * parent_opacity;
        match &item.content {
            StoredContent::Image {
                image,
                name,
                image_transform,
                bounds,
            } => {
                let crop = visible_pixel_rect(&board.images[*image], *image_transform, bounds);
                board.image_items.push(PureRefImageItem {
                    image: *image,
                    name: name.clone(),
                    transform: image_transform.then(transform),
                    crop,
                    opacity,
                });
            }
            StoredContent::Note { text } => board.notes.push(PureRefNote {
                text: text.clone(),
                transform,
            }),
            StoredContent::Container => {}
        }
        pending.extend(
            children[index]
                .iter()
                .rev()
                .map(|&child| (child, transform, opacity)),
        );
    }
    board
}

/// Bounding box of the crop path in pixel coordinates, clamped to the image.
fn visible_pixel_rect(
    image: &PureRefImage,
    image_transform: PureRefTransform,
    bounds: &[(f64, f64)],
) -> PureRefPixelRect {
    let (width, height) = (f64::from(image.width), f64::from(image.height));
    let full = PureRefPixelRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let Some(local_to_pixel) = image_transform.inverse() else {
        return full;
    };
    if bounds.is_empty() {
        return full;
    }
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in bounds {
        let (x, y) = local_to_pixel.map_point(x, y);
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    let (left, right) = (left.clamp(0.0, width), right.clamp(0.0, width));
    let (top, bottom) = (top.clamp(0.0, height), bottom.clamp(0.0, height));
    PureRefPixelRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}
