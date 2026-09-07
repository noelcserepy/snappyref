use super::boards::active_board_id;
use super::connection::Library;
#[cfg(test)]
use super::connection::write_transaction;
#[cfg(test)]
use super::stack_order::append_canvas_stack_item;
use crate::data::CanvasAnnotation;
use crate::data::CanvasAnnotationColor;
use crate::data::CanvasAnnotationKind;
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
        background_enabled: bool,
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
            background_enabled,
            false,
        )
    }

    pub fn load_canvas_annotations(&self) -> Result<Vec<CanvasAnnotation>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        load_canvas_annotations_for_board_id(&connection, active_board_id)
    }

    #[cfg(test)]
    pub fn create_canvas_annotation(
        &self,
        kind: CanvasAnnotationKind,
        start: egui::Pos2,
        end: egui::Pos2,
        text: &str,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
        color: CanvasAnnotationColor,
        background_enabled: bool,
    ) -> Result<CanvasAnnotation, String> {
        let annotation = CanvasAnnotation::new(
            self.allocate_document_id(),
            kind,
            start,
            end,
            text.to_owned(),
            text_size,
            stroke_width,
            color,
            background_enabled,
            false,
        );
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = write_transaction(&mut connection, "annotation create")?;
        upsert_board_annotation(&transaction, active_board_id, &annotation, false)?;
        append_canvas_stack_item(
            &transaction,
            active_board_id,
            CanvasStackItem::Annotation(annotation.id),
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation create: {error}"))?;
        Ok(annotation)
    }

    #[cfg(test)]
    pub fn update_canvas_annotation(&self, annotation: &CanvasAnnotation) -> Result<(), String> {
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = write_transaction(&mut connection, "annotation update")?;
        upsert_board_annotation(
            &transaction,
            active_board_id,
            annotation,
            annotation.archived,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation update: {error}"))
    }

    #[cfg(test)]
    pub fn archive_canvas_annotations(&self, annotation_ids: &[i64]) -> Result<(), String> {
        if annotation_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "annotation archive")?;

        for annotation_id in annotation_ids {
            transaction
                .execute(
                    "UPDATE canvas_annotations SET archived = 1 WHERE id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!("failed to archive canvas annotation {annotation_id}: {error}")
                })?;
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'annotation'
                       AND item_id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!(
                        "failed to remove canvas annotation {annotation_id} from stack: {error}"
                    )
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation archive transaction: {error}"))?;

        Ok(())
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
    let background_enabled = if annotation.background_enabled {
        1_i64
    } else {
        0_i64
    };
    let changed = transaction
        .execute(
            "UPDATE canvas_annotations
             SET kind = ?1, start_x = ?2, start_y = ?3, end_x = ?4, end_y = ?5,
                 text = ?6, text_size = ?7, stroke_width = ?8, color = ?9,
                 background_enabled = ?10, archived = ?11
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
                background_enabled,
                archived,
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
                    text_size, stroke_width, color, background_enabled, archived
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
                    background_enabled,
                    archived,
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
                    background_enabled
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
            Ok(CanvasAnnotation::new(
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
                row.get::<_, i64>(10).unwrap_or(0) != 0,
                false,
            ))
        })
        .map_err(|error| format!("failed to read canvas-annotations query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas annotations: {error}"))
}
