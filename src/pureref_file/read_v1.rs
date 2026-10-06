//! PureRef 1.x (format "1.10"): header, image blobs, item records, save folder, then a
//! ref table that locates each image item's blob.

use std::collections::HashMap;
use std::io::Cursor;

use super::qt_stream::QtReader;
use super::{PureRefBoard, PureRefImage, PureRefTransform, StoredContent, StoredItem, flatten};

const IMAGE_ITEM: &str = "GraphicsImageItem";
const TEXT_ITEM: &str = "GraphicsTextItem";
const LINKED_BLOB: u32 = u32::MAX;

pub(super) fn read(bytes: &[u8]) -> Result<PureRefBoard, String> {
    let mut reader = QtReader::new(bytes);
    let _format = reader.string()?;
    let top_level_count = reader.u16()?;
    let _image_count = reader.u16()?;
    let refs_offset = to_offset(reader.u64()?)?;
    let _app_version = reader.string()?;
    let _checksum = reader.string()?;
    // Next free item id, scene rect, view transform and both scroll offsets.
    reader.take(4 + 32 + 72 + 8)?;

    let blobs = image_blobs(bytes, refs_offset)?;
    let items_start = blobs
        .values()
        .map(|blob| blob.end)
        .max()
        .unwrap_or(reader.position());
    reader.seek(items_start)?;
    let records = read_records(&mut reader, top_level_count)?;

    let mut images = Vec::new();
    let mut image_by_owner = HashMap::new();
    let sources: HashMap<u32, Option<String>> = records
        .iter()
        .filter_map(|record| match &record.kind {
            RecordKind::Image { id, source, .. } => Some((*id, source.clone())),
            _ => None,
        })
        .collect();
    let mut items = Vec::with_capacity(records.len());
    for record in records {
        let content = match record.kind {
            RecordKind::Image {
                id,
                source,
                name,
                image_transform,
                bounds,
            } => {
                let blob = blobs
                    .get(&id)
                    .ok_or_else(|| format!("image item {id} has no ref table entry"))?;
                let image = match blob.duplicate_of(bytes) {
                    Some(LINKED_BLOB) => {
                        images.push(linked_image(source, image_transform));
                        images.len() - 1
                    }
                    owner => {
                        let owner = owner.unwrap_or(id);
                        match image_by_owner.get(&owner) {
                            Some(&image) => image,
                            None => {
                                let owner_blob = blobs.get(&owner).ok_or_else(|| {
                                    format!("image item {id} duplicates missing item {owner}")
                                })?;
                                let source = sources.get(&owner).cloned().flatten();
                                images.push(embedded_image(
                                    &bytes[owner_blob.start..owner_blob.end],
                                    source,
                                    image_transform,
                                ));
                                image_by_owner.insert(owner, images.len() - 1);
                                images.len() - 1
                            }
                        }
                    }
                };
                // PureRef centres the pixels on the item origin. Duplicates in real 1.x files
                // store a drifting translation (-1, then -1.5 on a 1x1 image) that would
                // crop them away, so the image size decides instead.
                let size = &images[image];
                StoredContent::Image {
                    image,
                    name,
                    image_transform: PureRefTransform::translation(
                        -f64::from(size.width) / 2.0,
                        -f64::from(size.height) / 2.0,
                    ),
                    bounds,
                }
            }
            RecordKind::Text { text } => StoredContent::Note { text },
            RecordKind::Unknown => StoredContent::Container,
        };
        items.push(StoredItem {
            parent: record.parent,
            z: record.z,
            sort_order: 0.0,
            transform: record.transform,
            opacity: 1.0,
            content,
        });
    }
    Ok(flatten(images, items))
}

struct BlobRange {
    start: usize,
    end: usize,
}

impl BlobRange {
    /// A 4-byte blob holds the id of the item that owns the pixels (or the link marker)
    /// instead of image data.
    fn duplicate_of(&self, bytes: &[u8]) -> Option<u32> {
        let blob: [u8; 4] = bytes[self.start..self.end].try_into().ok()?;
        Some(u32::from_be_bytes(blob))
    }
}

/// The ref table runs from `refs_offset` to the end of the file, one
/// `u32 item id, u64 start, u64 end` entry per image item.
fn image_blobs(bytes: &[u8], refs_offset: usize) -> Result<HashMap<u32, BlobRange>, String> {
    let mut reader = QtReader::new(bytes);
    reader.seek(refs_offset)?;
    let mut blobs = HashMap::new();
    while reader.position() < bytes.len() {
        let id = reader.u32()?;
        let start = to_offset(reader.u64()?)?;
        let end = to_offset(reader.u64()?)?;
        if start > end || end > bytes.len() {
            return Err(format!("image item {id} has an invalid blob range"));
        }
        blobs.insert(id, BlobRange { start, end });
    }
    Ok(blobs)
}

struct Record {
    parent: Option<usize>,
    transform: PureRefTransform,
    z: f64,
    kind: RecordKind,
}

enum RecordKind {
    Image {
        id: u32,
        source: Option<String>,
        name: Option<String>,
        image_transform: PureRefTransform,
        bounds: Vec<(f64, f64)>,
    },
    Text {
        text: String,
    },
    Unknown,
}

/// Records are stored depth-first: each record is followed by its children.
fn read_records(reader: &mut QtReader, top_level_count: u16) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut open_parents = vec![(None, u32::from(top_level_count))];
    while let Some((parent, remaining)) = open_parents.last_mut() {
        if *remaining == 0 {
            open_parents.pop();
            continue;
        }
        *remaining -= 1;
        let parent = *parent;
        let (record, child_count) = read_record(reader, parent)?;
        records.push(record);
        if child_count > 0 {
            open_parents.push((Some(records.len() - 1), child_count));
        }
    }
    Ok(records)
}

fn read_record(reader: &mut QtReader, parent: Option<usize>) -> Result<(Record, u32), String> {
    let end = to_offset(reader.u64()?)?;
    let kind = reader.string()?.unwrap_or_default();
    let mut record = Record {
        parent,
        transform: PureRefTransform::IDENTITY,
        z: 0.0,
        kind: RecordKind::Unknown,
    };
    match kind.as_str() {
        IMAGE_ITEM => {
            let source = reader.string()?;
            let name = reader.string()?;
            let _always_one = reader.f64()?;
            record.transform = reader.transform()?;
            let id = reader.u32()?;
            record.z = reader.f64()?;
            let image_transform = reader.transform()?;
            let bounds = reader.path_points()?;
            record.kind = RecordKind::Image {
                id,
                source,
                name: name.filter(|name| !name.is_empty()),
                image_transform,
                bounds,
            };
        }
        TEXT_ITEM => {
            let text = reader.string()?.unwrap_or_default();
            record.transform = reader.transform()?;
            let _id = reader.u32()?;
            record.z = reader.f64()?;
            record.kind = RecordKind::Text { text };
        }
        _ => {}
    }
    // The child count is the last field before `end`; skip any unknown tail bytes.
    let child_count_at = end
        .checked_sub(4)
        .filter(|offset| *offset >= reader.position())
        .ok_or_else(|| format!("{kind} record overruns its end offset {end}"))?;
    reader.seek(child_count_at)?;
    let child_count = reader.u32()?;
    Ok((record, child_count))
}

/// Unreadable pixels keep the stored size so the rest of the board still loads.
fn embedded_image(
    bytes: &[u8],
    source: Option<String>,
    image_transform: PureRefTransform,
) -> PureRefImage {
    let stored = linked_image(source, image_transform);
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .unwrap_or((stored.width, stored.height));
    PureRefImage {
        bytes: Some(bytes.to_vec()),
        width,
        height,
        ..stored
    }
}

/// Without pixels the size only survives in the image transform, `translate(-w/2, -h/2)`.
fn linked_image(source: Option<String>, image_transform: PureRefTransform) -> PureRefImage {
    PureRefImage {
        bytes: None,
        source,
        width: (-2.0 * image_transform.dx).round().max(0.0) as u32,
        height: (-2.0 * image_transform.dy).round().max(0.0) as u32,
    }
}

fn to_offset(value: u64) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| format!("offset {value} does not fit in memory"))
}
