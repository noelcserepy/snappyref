//! Writes PureRef 2.x files: build the SQLite database, then wrap it in the Qt header,
//! moving the database bytes the header covers to the end of the file.

use md5::{Digest, Md5};
use rusqlite::{Connection, MAIN_DB, params};

use super::qt_stream::{
    encoded_string_len, polygon_path_variant, put_byte_array, put_string, put_u32, put_u64,
    rect_variant, size_variant, transform_variant, whole_rational_variant,
};
use super::{PureRefBoard, PureRefImage, PureRefTransform};

const FORMAT_VERSION: &str = "2.1";
/// The oldest 2.x app that writes the "2.1" envelope, so every 2.x install opens the file.
const APP_VERSION: &str = "2.0.3";
pub(super) const APPLICATION_ID: i64 = 940_753_918;
pub(super) const USER_VERSION: i64 = 200_101;
const MD5_HEX_LEN: usize = 32;
const ROOT_PARENT: i64 = -1;
const SOURCE_EMBEDDED: i64 = 1;
const SOURCE_LINKED: i64 = 2;
const BILINEAR_FILTERING: i64 = 1;
const COMFORTABLE_NOTE_STYLE: i64 = 0;

/// Every table and column PureRef expects; a missing column fails the load.
const SCHEMA: &str = "
CREATE TABLE images (id INTEGER PRIMARY KEY,source_type INTEGER,origin TEXT,source TEXT,format TEXT,checksum TEXT,data BLOB,width INTEGER,height INTEGER);
CREATE TABLE metadata (id INTEGER PRIMARY KEY,scene_rect TEXT,application_version TEXT,view_transform TEXT,thumbnail BLOB,horizontal_scroll INTEGER,vertical_scroll INTEGER,last_save_path TEXT,last_load_path TEXT,last_load_checksum TEXT,saved INTEGER);
CREATE TABLE items (parent INTEGER,id INTEGER PRIMARY KEY,name TEXT,transform BLOB,sort_order BLOB,z REAL,opacity REAL,locked INTEGER,comment INTEGER);
CREATE TABLE items_images (image INTEGER,playback_speed REAL,id INTEGER PRIMARY KEY,playback_state INTEGER,image_transform BLOB,image_bounds BLOB,playback_frame INTEGER,flags INTEGER);
CREATE TABLE items_drawings (id INTEGER PRIMARY KEY,strokes BLOB);
CREATE TABLE items_notes (text_color TEXT,id INTEGER PRIMARY KEY,fixed_size TEXT,background_color TEXT,text TEXT,style INTEGER);
CREATE TABLE items_groups (id INTEGER PRIMARY KEY,background_color TEXT,lock_mode INTEGER);
";

/// Writes a 2.x file with every item at the root, images below notes.
pub fn write_board(board: &PureRefBoard) -> Result<Vec<u8>, String> {
    if let Some(item) = board
        .image_items
        .iter()
        .find(|item| item.image >= board.images.len())
    {
        return Err(format!("image item refers to missing image {}", item.image));
    }
    envelope(&board_database(board).map_err(|error| format!("PureRef database: {error}"))?)
}

fn board_database(board: &PureRefBoard) -> rusqlite::Result<Vec<u8>> {
    let mut connection = Connection::open_in_memory()?;
    // Page layout pragmas only take effect before the first table exists.
    connection.execute_batch(&format!(
        "PRAGMA page_size = 4096;
         PRAGMA auto_vacuum = FULL;
         PRAGMA encoding = 'UTF-8';
         PRAGMA application_id = {APPLICATION_ID};
         PRAGMA user_version = {USER_VERSION};
         {SCHEMA}"
    ))?;
    let transaction = connection.transaction()?;

    for (id, image) in board.images.iter().enumerate() {
        transaction.execute(
            "INSERT INTO images (id, source_type, origin, source, format, checksum, data, width, height)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id as i64,
                if image.bytes.is_some() { SOURCE_EMBEDDED } else { SOURCE_LINKED },
                image.source,
                image_format(image),
                image.bytes.as_deref().map(md5_hex),
                image.bytes,
                image.width,
                image.height,
            ],
        )?;
    }

    let mut next_item_id = 0_i64;
    let mut insert_item = |name: Option<&str>, transform: PureRefTransform, opacity: f32| {
        let id = next_item_id;
        next_item_id += 1;
        let stacking = id + 1;
        transaction
            .execute(
                "INSERT INTO items (parent, id, name, transform, sort_order, z, opacity, locked, comment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL)",
                params![
                    ROOT_PARENT,
                    id,
                    name,
                    variant_text(&transform_variant(transform)),
                    variant_text(&whole_rational_variant(stacking as u32)),
                    stacking as f64,
                    f64::from(opacity),
                ],
            )
            .map(|_| id)
    };

    let mut scene_bounds = SceneBounds::default();
    let mut image_rows = Vec::with_capacity(board.image_items.len());
    for item in &board.image_items {
        let image = &board.images[item.image];
        let (half_width, half_height) =
            (f64::from(image.width) / 2.0, f64::from(image.height) / 2.0);
        // Item-local coordinates have their origin at the image centre.
        let item_transform =
            PureRefTransform::translation(half_width, half_height).then(item.transform);
        let id = insert_item(item.name.as_deref(), item_transform, item.opacity)?;
        let crop = item.crop;
        let (left, top) = (crop.x - half_width, crop.y - half_height);
        let (right, bottom) = (left + crop.width, top + crop.height);
        let bounds = [
            (left, top),
            (right, top),
            (right, bottom),
            (left, bottom),
            (left, top),
        ];
        for (x, y) in &bounds[..4] {
            scene_bounds.include(item_transform.map_point(*x, *y));
        }
        image_rows.push((id, item.image, half_width, half_height, bounds));
    }
    let mut note_rows = Vec::with_capacity(board.notes.len());
    for note in &board.notes {
        scene_bounds.include(note.transform.map_point(0.0, 0.0));
        let id = insert_item(None, note.transform, 1.0)?;
        note_rows.push((id, note_html(&note.text)));
    }

    for (id, image, half_width, half_height, bounds) in image_rows {
        transaction.execute(
            "INSERT INTO items_images (image, playback_speed, id, playback_state, image_transform, image_bounds, playback_frame, flags)
             VALUES (?1, 1.0, ?2, 0, ?3, ?4, 0, ?5)",
            params![
                image as i64,
                id,
                variant_text(&transform_variant(PureRefTransform::translation(
                    -half_width,
                    -half_height
                ))),
                variant_text(&polygon_path_variant(&bounds)),
                BILINEAR_FILTERING,
            ],
        )?;
    }
    for (id, html) in note_rows {
        transaction.execute(
            "INSERT INTO items_notes (text_color, id, fixed_size, background_color, text, style)
             VALUES (NULL, ?1, ?2, '', ?3, ?4)",
            params![
                id,
                // (-1, -1) lets PureRef size the note to its text.
                variant_text(&size_variant(-1.0, -1.0)),
                html,
                COMFORTABLE_NOTE_STYLE,
            ],
        )?;
    }

    let (zoom, horizontal_scroll, vertical_scroll) = scene_bounds.framing_view();
    transaction.execute(
        "INSERT INTO metadata (id, scene_rect, application_version, view_transform, thumbnail,
             horizontal_scroll, vertical_scroll, last_save_path, last_load_path, last_load_checksum, saved)
         VALUES (0, ?1, ?2, ?3, X'', ?4, ?5, NULL, NULL, NULL, 1)",
        params![
            scene_bounds.variant().map(|variant| variant_text(&variant)),
            APP_VERSION,
            variant_text(&transform_variant(PureRefTransform::scale(zoom))),
            horizontal_scroll,
            vertical_scroll,
        ],
    )?;
    transaction.commit()?;
    Ok(connection.serialize(MAIN_DB)?.to_vec())
}

/// PureRef frames the view and sizes scene exports from the stored scene rect.
/// Left NULL, the CLI's `exportScene` clipped items far from the origin.
#[derive(Default)]
struct SceneBounds(Option<(f64, f64, f64, f64)>);

impl SceneBounds {
    fn include(&mut self, (x, y): (f64, f64)) {
        let (min_x, min_y, max_x, max_y) = self.0.get_or_insert((x, y, x, y));
        *min_x = min_x.min(x);
        *min_y = min_y.min(y);
        *max_x = max_x.max(x);
        *max_y = max_y.max(y);
    }

    /// PureRef reopens a file at its stored view instead of fitting the content, so frame the
    /// content for a fresh 600x400 PureRef window. Scroll is the view's top-left in zoomed scene units.
    fn framing_view(&self) -> (f64, i64, i64) {
        const WINDOW: (f64, f64) = (600.0, 400.0);
        const MARGIN: f64 = 40.0;
        let Some((min_x, min_y, max_x, max_y)) = self.0 else {
            return (1.0, -(WINDOW.0 / 2.0) as i64, -(WINDOW.1 / 2.0) as i64);
        };
        let zoom = ((WINDOW.0 - MARGIN) / (max_x - min_x))
            .min((WINDOW.1 - MARGIN) / (max_y - min_y))
            .min(1.0);
        let scroll = |min: f64, max: f64, window: f64| {
            ((min + max) / 2.0 * zoom - window / 2.0).round() as i64
        };
        (
            zoom,
            scroll(min_x, max_x, WINDOW.0),
            scroll(min_y, max_y, WINDOW.1),
        )
    }

    fn variant(&self) -> Option<Vec<u8>> {
        self.0.map(|(min_x, min_y, max_x, max_y)| {
            rect_variant(min_x, min_y, max_x - min_x, max_y - min_y)
        })
    }
}

/// The header overwrites the first `H` database bytes, which move to the end of the file.
/// The checksum covers everything after the checksum string.
fn envelope(database: &[u8]) -> Result<Vec<u8>, String> {
    let thumbnail: &[u8] = &[];
    let header_len = encoded_string_len(FORMAT_VERSION)
        + 4
        + 8
        + encoded_string_len(APP_VERSION)
        + 4
        + 2 * MD5_HEX_LEN
        + 4
        + thumbnail.len();
    if header_len >= database.len() {
        return Err("PureRef database is smaller than its header".to_owned());
    }
    let mut tail = Vec::with_capacity(database.len() + 4);
    put_byte_array(&mut tail, thumbnail);
    tail.extend_from_slice(&database[header_len..]);
    tail.extend_from_slice(&database[..header_len]);

    let mut file = Vec::with_capacity(header_len + database.len());
    put_string(&mut file, FORMAT_VERSION);
    put_u32(&mut file, 0);
    put_u64(&mut file, database.len() as u64);
    put_string(&mut file, APP_VERSION);
    put_string(&mut file, &md5_hex(&tail));
    file.extend_from_slice(&tail);
    Ok(file)
}

/// PureRef stores QVariant bytes as TEXT, one Latin-1 character per byte.
fn variant_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

fn md5_hex(bytes: &[u8]) -> String {
    hex::encode(Md5::digest(bytes))
}

/// PureRef writes "PNG"/"JPG" for detected formats and the lowercase extension otherwise.
fn image_format(image: &PureRefImage) -> Option<String> {
    let detected = image
        .bytes
        .as_deref()
        .and_then(|bytes| image::guess_format(bytes).ok());
    match detected {
        Some(image::ImageFormat::Png) => Some("PNG".to_owned()),
        Some(image::ImageFormat::Jpeg) => Some("JPG".to_owned()),
        Some(format) => format.extensions_str().first().map(|ext| (*ext).to_owned()),
        None => image
            .source
            .as_deref()
            .and_then(|source| source.rsplit_once('.'))
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .filter(|extension| !extension.is_empty() && !extension.contains(['/', '\\'])),
    }
}

/// Minimal Qt rich text, one paragraph per line.
fn note_html(text: &str) -> String {
    let paragraphs: String = text
        .split('\n')
        .map(|line| {
            let line = escape_html(line.trim_end_matches('\r'));
            let content = if line.is_empty() { "<br />" } else { &line };
            format!("<p style=\"white-space:pre-wrap;margin:0\">{content}</p>")
        })
        .collect();
    format!(
        "<html><body style=\"font-family:Open Sans;font-size:{}px;\">{paragraphs}</body></html>",
        super::NOTE_FONT_PX
    )
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
