use super::boards::active_board_id;
#[cfg(test)]
use super::stack_order::append_canvas_stack_item;
use super::*;

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
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let annotation_id = self.allocate_document_id();
        connection
            .execute(
                "INSERT INTO canvas_annotations (
                    id,
                    board_id,
                    kind,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    text,
                    text_size,
                    stroke_width,
                    color,
                    background_enabled,
                    archived
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    annotation_id,
                    active_board_id,
                    kind.as_str(),
                    start.x,
                    start.y,
                    end.x,
                    end.y,
                    text,
                    text_size.as_str(),
                    stroke_width.as_str(),
                    color.as_str(),
                    if background_enabled { 1_i64 } else { 0_i64 },
                    0_i64
                ],
            )
            .map_err(|error| format!("failed to create canvas annotation: {error}"))?;

        append_canvas_stack_item(
            &connection,
            active_board_id,
            CanvasStackItem::Annotation(annotation_id),
        )?;

        Ok(CanvasAnnotation::new(
            annotation_id,
            kind,
            start,
            end,
            text.to_owned(),
            text_size,
            stroke_width,
            color,
            background_enabled,
            false,
        ))
    }

    #[cfg(test)]
    pub fn update_canvas_annotation(&self, annotation: &CanvasAnnotation) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE canvas_annotations
                 SET start_x = ?1,
                     start_y = ?2,
                     end_x = ?3,
                     end_y = ?4,
                     text = ?5,
                     text_size = ?6,
                     stroke_width = ?7,
                     color = ?8,
                     background_enabled = ?9,
                     archived = ?10
                 WHERE id = ?11",
                params![
                    annotation.start().x,
                    annotation.start().y,
                    annotation.end().x,
                    annotation.end().y,
                    annotation.text,
                    annotation.text_size.as_str(),
                    annotation.stroke_width.as_str(),
                    annotation.color.as_str(),
                    if annotation.background_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    if annotation.archived { 1_i64 } else { 0_i64 },
                    annotation.id,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to update canvas annotation {}: {error}",
                    annotation.id
                )
            })?;

        Ok(())
    }

    #[cfg(test)]
    pub fn archive_canvas_annotations(&self, annotation_ids: &[i64]) -> Result<(), String> {
        if annotation_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start annotation archive transaction: {error}"))?;

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
