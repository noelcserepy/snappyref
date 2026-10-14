//! PureRef 2.x: a SQLite database behind a Qt header that overwrote its first bytes.

use std::collections::HashMap;

use rusqlite::types::{FromSql, ValueRef};
use rusqlite::{Connection, MAIN_DB, Row};

use super::qt_stream::{QtReader, variant_path_points, variant_rational, variant_transform};
use super::{PureRefBoard, PureRefImage, PureRefTransform, StoredContent, StoredItem, flatten};

/// Envelope "2.0" ends at the checksum; later envelopes add a thumbnail.
const ENVELOPE_WITHOUT_THUMBNAIL: &str = "2.0";
const ROOT_PARENT: i64 = -1;

pub(super) fn read(bytes: &[u8], format: &str) -> Result<PureRefBoard, String> {
    let database = embedded_database(bytes, format)?;
    let mut connection = Connection::open_in_memory().map_err(sql)?;
    connection
        .deserialize_read_exact(MAIN_DB, database.as_slice(), database.len(), true)
        .map_err(sql)?;

    let mut images = Vec::new();
    let mut image_index_by_id = HashMap::new();
    query(
        &connection,
        "SELECT id, origin, source, data, width, height FROM images ORDER BY id",
        |row| {
            let id: i64 = cell(row, 0)?;
            let origin: Option<String> = cell(row, 1)?;
            let source: Option<String> = cell(row, 2)?;
            let data: Option<Vec<u8>> = cell(row, 3)?;
            image_index_by_id.insert(id, images.len());
            images.push(PureRefImage {
                bytes: data.filter(|data| !data.is_empty()),
                source: source.or(origin).filter(|source| !source.is_empty()),
                width: cell(row, 4)?,
                height: cell(row, 5)?,
            });
            Ok(())
        },
    )?;

    let mut image_contents = HashMap::new();
    query(
        &connection,
        "SELECT id, image, image_transform, image_bounds FROM items_images",
        |row| {
            let id: i64 = cell(row, 0)?;
            let image_id: i64 = cell(row, 1)?;
            let image = *image_index_by_id
                .get(&image_id)
                .ok_or_else(|| format!("image item {id} refers to missing image {image_id}"))?;
            let image_transform = match variant_cell(row, 2)? {
                Some(bytes) => variant_transform(&bytes)?,
                None => {
                    let size = &images[image];
                    PureRefTransform::translation(
                        -f64::from(size.width) / 2.0,
                        -f64::from(size.height) / 2.0,
                    )
                }
            };
            let bounds = match variant_cell(row, 3)? {
                Some(bytes) => variant_path_points(&bytes)?,
                None => Vec::new(),
            };
            image_contents.insert(id, (image, image_transform, bounds));
            Ok(())
        },
    )?;

    let mut note_texts = HashMap::new();
    query(&connection, "SELECT id, text FROM items_notes", |row| {
        let text: Option<String> = cell(row, 1)?;
        note_texts.insert(
            cell::<i64>(row, 0)?,
            note_plain_text(&text.unwrap_or_default()),
        );
        Ok(())
    })?;

    let mut stored = Vec::new();
    let mut parent_ids = Vec::new();
    let mut index_by_id = HashMap::new();
    query(
        &connection,
        "SELECT id, parent, name, transform, sort_order, z, opacity FROM items",
        |row| {
            let id: i64 = cell(row, 0)?;
            let name: Option<String> = cell(row, 2)?;
            let content = if let Some((image, image_transform, bounds)) = image_contents.remove(&id)
            {
                StoredContent::Image {
                    image,
                    name: name.filter(|name| !name.is_empty()),
                    image_transform,
                    bounds,
                }
            } else if let Some(text) = note_texts.remove(&id) {
                StoredContent::Note { text }
            } else {
                StoredContent::Container
            };
            index_by_id.insert(id, stored.len());
            parent_ids.push(cell::<Option<i64>>(row, 1)?.unwrap_or(ROOT_PARENT));
            stored.push(StoredItem {
                parent: None,
                z: cell::<Option<f64>>(row, 5)?.unwrap_or(0.0),
                sort_order: variant_cell(row, 4)?
                    .map(|bytes| variant_rational(&bytes))
                    .transpose()?
                    .unwrap_or(0.0),
                transform: variant_cell(row, 3)?
                    .map(|bytes| variant_transform(&bytes))
                    .transpose()?
                    .unwrap_or(PureRefTransform::IDENTITY),
                opacity: cell::<Option<f64>>(row, 6)?.unwrap_or(1.0) as f32,
                content,
            });
            Ok(())
        },
    )?;
    for (item, parent_id) in stored.iter_mut().zip(parent_ids) {
        item.parent = if parent_id == ROOT_PARENT {
            None
        } else {
            Some(
                *index_by_id
                    .get(&parent_id)
                    .ok_or_else(|| format!("PureRef item refers to missing parent {parent_id}"))?,
            )
        };
    }
    // Every item must reach a root. A cycle otherwise disappears during flattening.
    let mut visited = vec![false; stored.len()];
    let mut complete = vec![false; stored.len()];
    for start in 0..stored.len() {
        let mut current = Some(start);
        let mut path = Vec::new();
        while let Some(index) = current {
            if complete[index] {
                break;
            }
            if visited[index] {
                return Err("PureRef item parent graph contains a cycle".to_owned());
            }
            visited[index] = true;
            path.push(index);
            current = stored[index].parent;
        }
        for index in path {
            complete[index] = true;
        }
    }
    Ok(flatten(images, stored))
}

/// Undoes the displaced prefix: the header overwrote `D[0..H]`, which now sits at the end.
pub(super) fn embedded_database(bytes: &[u8], format: &str) -> Result<Vec<u8>, String> {
    let mut reader = QtReader::new(bytes);
    let _format = reader.string()?;
    let _reserved = reader.u32()?;
    let database_len = usize::try_from(reader.u64()?).map_err(|error| error.to_string())?;
    let _app_version = reader.string()?;
    let _checksum = reader.string()?;
    if format != ENVELOPE_WITHOUT_THUMBNAIL {
        let _thumbnail = reader.byte_array()?;
    }
    let header_len = reader.position();
    if header_len >= database_len || bytes.len() != database_len + header_len {
        return Err(format!(
            "corrupt PureRef container: header {header_len} bytes, database {database_len} bytes, file {} bytes",
            bytes.len()
        ));
    }
    let mut database = Vec::with_capacity(database_len);
    database.extend_from_slice(&bytes[database_len..]);
    database.extend_from_slice(&bytes[header_len..database_len]);
    Ok(database)
}

fn query(
    connection: &Connection,
    sql_text: &str,
    mut on_row: impl FnMut(&Row) -> Result<(), String>,
) -> Result<(), String> {
    let mut statement = connection.prepare(sql_text).map_err(sql)?;
    let mut rows = statement.query([]).map_err(sql)?;
    while let Some(row) = rows.next().map_err(sql)? {
        on_row(row)?;
    }
    Ok(())
}

fn cell<T: FromSql>(row: &Row, column: usize) -> Result<T, String> {
    row.get(column).map_err(sql)
}

/// Variant cells are stored as TEXT with one Latin-1 character per byte.
fn variant_cell(row: &Row, column: usize) -> Result<Option<Vec<u8>>, String> {
    match row.get_ref(column).map_err(sql)? {
        ValueRef::Null => Ok(None),
        ValueRef::Text(text) => std::str::from_utf8(text)
            .map_err(|error| error.to_string())?
            .chars()
            .map(|character| {
                u8::try_from(character)
                    .map_err(|_| "variant cell has a character above U+00FF".to_owned())
            })
            .collect::<Result<_, _>>()
            .map(Some),
        ValueRef::Blob(blob) => Ok(Some(blob.to_vec())),
        _ => Err("variant cell is not text".to_owned()),
    }
}

fn sql(error: rusqlite::Error) -> String {
    format!("PureRef database: {error}")
}

/// Qt rich text to plain text: one line per paragraph, `<br>` as a line break.
fn note_plain_text(html: &str) -> String {
    if !html.contains('<') {
        return html.to_owned();
    }
    let body = match html.find("<body") {
        Some(start) => {
            let content = html[start..]
                .find('>')
                .map_or(html.len(), |end| start + end + 1);
            let end = html[content..]
                .find("</body")
                .map_or(html.len(), |end| content + end);
            &html[content..end]
        }
        None => html,
    };

    let mut paragraphs = Vec::new();
    let mut current: Option<String> = None;
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(tag) = rest.strip_prefix('<') {
            let tag_end = tag.find('>').unwrap_or(tag.len());
            rest = tag.get(tag_end + 1..).unwrap_or("");
            let tag = &tag[..tag_end];
            let closing = tag.starts_with('/');
            let name = tag
                .trim_start_matches('/')
                .split(|character: char| character.is_whitespace() || character == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            match name.as_str() {
                "br" => current.get_or_insert_default().push('\n'),
                "p" | "div" | "li" | "pre" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    paragraphs.extend(current.take());
                    if !closing {
                        current = Some(String::new());
                    }
                }
                _ => {}
            }
        } else {
            let text_end = rest.find('<').unwrap_or(rest.len());
            let text = decode_entities(&rest[..text_end]);
            rest = &rest[text_end..];
            match current.as_mut() {
                Some(paragraph) => paragraph.push_str(&text),
                // Qt puts formatting newlines between block tags.
                None if text.trim().is_empty() => {}
                None => paragraphs.push(text.trim().to_owned()),
            }
        }
    }
    paragraphs.extend(current);
    paragraphs
        .into_iter()
        // Qt writes an empty paragraph as `<p><br /></p>`.
        .map(|paragraph| {
            if paragraph == "\n" {
                String::new()
            } else {
                paragraph
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        let entity = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| Some((entity_char(&rest[1..end])?, end)));
        match entity {
            Some((character, end)) => {
                decoded.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn entity_char(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}
