#[cfg(test)]
use super::boards::active_board_id;
use super::connection::Library;
use crate::data::CanvasAnnotation;
use crate::data::CanvasAnnotationColor;
use crate::data::CanvasAnnotationKind;
use crate::data::CanvasRectangleFill;
#[cfg(test)]
use crate::data::CanvasStackItem;
use crate::data::CanvasStrokeWidth;
use crate::data::CanvasTextSize;
use eframe::egui;
use rusqlite::Connection;
use rusqlite::Transaction;
use rusqlite::params;

impl Library {
    pub(crate) fn new_canvas_annotation(
        &self,
        kind: CanvasAnnotationKind,
        start: egui::Pos2,
        end: egui::Pos2,
        text: String,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
        color: CanvasAnnotationColor,
    ) -> CanvasAnnotation {
        CanvasAnnotation::new(
            self.allocate_document_id(),
            kind,
            start,
            end,
            text,
            text_size,
            stroke_width,
            color,
            false,
        )
    }

    #[cfg(test)]
    pub fn load_canvas_annotations(&self) -> Result<Vec<CanvasAnnotation>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        load_canvas_annotations_for_board_id(&connection, active_board_id)
    }

    #[cfg(test)]
    pub fn fixture_annotation(
        &self,
        kind: CanvasAnnotationKind,
        start: egui::Pos2,
        end: egui::Pos2,
        text: &str,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
        color: CanvasAnnotationColor,
    ) -> Result<CanvasAnnotation, String> {
        let annotation = self.new_canvas_annotation(
            kind,
            start,
            end,
            text.to_owned(),
            text_size,
            stroke_width,
            color,
        );
        let mut stack = self.load_canvas_stack_order()?;
        stack.push(CanvasStackItem::Annotation(annotation.id));
        self.fixture_board_document(
            self.load_user_session()?.active_board_id,
            crate::canvas::CanvasDocumentChanges {
                updated_annotations: vec![annotation.clone()],
                stack_order: Some(stack),
                ..Default::default()
            },
        )?;
        Ok(annotation)
    }
}

/// Writes the annotation row for `board_id`, inserting it when the Board has
/// no row for that id yet. `archived` is explicit: durability writes the
/// annotation's own flag, a document restore forces every row live.
pub(super) fn upsert_board_annotation(
    transaction: &Transaction<'_>,
    board_id: i64,
    annotation: &CanvasAnnotation,
    archived: bool,
) -> Result<(), String> {
    let archived = if archived { 1_i64 } else { 0_i64 };
    let changed = transaction
        .execute(
            "UPDATE canvas_annotations
             SET kind = ?1, start_x = ?2, start_y = ?3, end_x = ?4, end_y = ?5,
                 text = ?6, text_size = ?7, stroke_width = ?8, color = ?9,
                 archived = ?10, fill = ?11
             WHERE id = ?12 AND board_id = ?13",
            params![
                annotation.kind.as_str(),
                annotation.start().x,
                annotation.start().y,
                annotation.end().x,
                annotation.end().y,
                annotation.text,
                annotation.text_size.as_str(),
                annotation.stroke_width.as_str(),
                annotation.color.as_str(),
                archived,
                annotation.fill.as_str(),
                annotation.id,
                board_id,
            ],
        )
        .map_err(|error| {
            format!(
                "failed to update canvas annotation {}: {error}",
                annotation.id
            )
        })?;
    if changed == 0 {
        transaction
            .execute(
                "INSERT INTO canvas_annotations (
                    id, board_id, kind, start_x, start_y, end_x, end_y, text,
                    text_size, stroke_width, color, archived, fill
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    annotation.id,
                    board_id,
                    annotation.kind.as_str(),
                    annotation.start().x,
                    annotation.start().y,
                    annotation.end().x,
                    annotation.end().y,
                    annotation.text,
                    annotation.text_size.as_str(),
                    annotation.stroke_width.as_str(),
                    annotation.color.as_str(),
                    archived,
                    annotation.fill.as_str(),
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert canvas annotation {}: {error}",
                    annotation.id
                )
            })?;
    }
    Ok(())
}

pub(super) fn load_canvas_annotations_for_board_id(
    connection: &Connection,
    board_id: i64,
) -> Result<Vec<CanvasAnnotation>, String> {
    let mut statement = connection
        .prepare(
            "SELECT
                    id,
                    kind,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    text,
                    text_size,
                    stroke_width,
                    color,
                    fill
                 FROM canvas_annotations
                 WHERE archived = 0
                   AND board_id = ?1
                 ORDER BY id ASC",
        )
        .map_err(|error| format!("failed to prepare canvas-annotations query: {error}"))?;

    let rows = statement
        .query_map(params![board_id], |row| {
            let kind_text = row.get::<_, String>(1)?;
            let kind = CanvasAnnotationKind::from_str(&kind_text).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unknown canvas annotation kind: {kind_text}"),
                    )),
                )
            })?;
            let fill =
                CanvasRectangleFill::from_str(&row.get::<_, String>(10)?).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unknown canvas rectangle fill",
                        )),
                    )
                })?;
            let mut annotation = CanvasAnnotation::new(
                row.get(0)?,
                kind,
                egui::pos2(row.get(2)?, row.get(3)?),
                egui::pos2(row.get(4)?, row.get(5)?),
                row.get(6)?,
                CanvasTextSize::from_str(&row.get::<_, String>(7)?).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unknown canvas text size",
                        )),
                    )
                })?,
                CanvasStrokeWidth::from_str(&row.get::<_, String>(8)?).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unknown canvas stroke width",
                        )),
                    )
                })?,
                CanvasAnnotationColor::from_str(&row.get::<_, String>(9)?).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        9,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unknown canvas annotation color",
                        )),
                    )
                })?,
                false,
            );
            annotation.fill = fill;
            Ok(annotation)
        })
        .map_err(|error| format!("failed to read canvas-annotations query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas annotations: {error}"))
}
