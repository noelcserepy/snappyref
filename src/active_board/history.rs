use crate::data::{CanvasAnnotation, CanvasStackItem, Placement};
use crate::library::storage::Library;

#[derive(Default)]
pub(crate) struct DocumentHistory {
    undo_stack: Vec<HistoryAction>,
    redo_stack: Vec<HistoryAction>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryAction {
    pub(crate) label: String,
    pub(crate) placements: Vec<RowChange<Placement>>,
    pub(crate) annotations: Vec<RowChange<CanvasAnnotation>>,
    pub(crate) stack_order: Option<RowChange<Vec<CanvasStackItem>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RowChange<T> {
    pub(crate) before: Option<T>,
    pub(crate) after: Option<T>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryApplied {
    pub(crate) label: String,
    pub(crate) action: HistoryAction,
    pub(crate) direction: HistoryDirection,
}

impl DocumentHistory {
    pub(crate) fn record_applied(&mut self, action: HistoryAction) {
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply(&mut self, library: &Library, action: HistoryAction) -> Result<(), String> {
        apply_history_action(library, &action, HistoryDirection::Forward)?;
        self.undo_stack.push(action);
        self.redo_stack.clear();
        Ok(())
    }

    pub(crate) fn undo(&mut self, library: &Library) -> Result<Option<HistoryApplied>, String> {
        let Some(action) = self.undo_stack.pop() else {
            return Ok(None);
        };
        apply_history_action(library, &action, HistoryDirection::Backward)?;
        let label = action.label.clone();
        self.redo_stack.push(action.clone());
        Ok(Some(HistoryApplied {
            label,
            action,
            direction: HistoryDirection::Backward,
        }))
    }

    pub(crate) fn redo(&mut self, library: &Library) -> Result<Option<HistoryApplied>, String> {
        let Some(action) = self.redo_stack.pop() else {
            return Ok(None);
        };
        apply_history_action(library, &action, HistoryDirection::Forward)?;
        let label = action.label.clone();
        self.undo_stack.push(action.clone());
        Ok(Some(HistoryApplied {
            label,
            action,
            direction: HistoryDirection::Forward,
        }))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum HistoryDirection {
    Forward,
    Backward,
}

fn apply_history_action(
    library: &Library,
    action: &HistoryAction,
    direction: HistoryDirection,
) -> Result<(), String> {
    for change in &action.annotations {
        apply_annotation_change(library, change, direction)?;
    }
    for change in &action.placements {
        apply_placement_change(library, change, direction)?;
    }
    if let Some(change) = &action.stack_order {
        let target = match direction {
            HistoryDirection::Forward => change.after.as_ref(),
            HistoryDirection::Backward => change.before.as_ref(),
        };
        if let Some(stack_order) = target {
            library.save_canvas_stack_order(stack_order)?;
        }
    }
    Ok(())
}

fn apply_placement_change(
    library: &Library,
    change: &RowChange<Placement>,
    direction: HistoryDirection,
) -> Result<(), String> {
    let target = match direction {
        HistoryDirection::Forward => change.after.as_ref(),
        HistoryDirection::Backward => change.before.as_ref(),
    };
    let fallback = match direction {
        HistoryDirection::Forward => change.before.as_ref(),
        HistoryDirection::Backward => change.after.as_ref(),
    };
    let Some(item) = target.or(fallback) else {
        return Ok(());
    };
    if item.state.archived {
        library.archive_placements(&[item.state.id])
    } else if target.is_none() {
        library.archive_placements(&[item.state.id])
    } else {
        library.restore_placements(&[item.state.id])?;
        library.update_placement_document_state(&item.state)
    }
}

fn apply_annotation_change(
    library: &Library,
    change: &RowChange<CanvasAnnotation>,
    direction: HistoryDirection,
) -> Result<(), String> {
    let target = match direction {
        HistoryDirection::Forward => change.after.as_ref(),
        HistoryDirection::Backward => change.before.as_ref(),
    };
    let fallback = match direction {
        HistoryDirection::Forward => change.before.as_ref(),
        HistoryDirection::Backward => change.after.as_ref(),
    };
    let Some(annotation) = target.or(fallback) else {
        return Ok(());
    };
    if annotation.archived {
        library.archive_canvas_annotations(&[annotation.id])
    } else if target.is_none() {
        library.archive_canvas_annotations(&[annotation.id])
    } else {
        library.restore_canvas_annotations(&[annotation.id])?;
        library.update_canvas_annotation(annotation)
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::{DocumentHistory, HistoryAction, RowChange};
    use crate::data::{
        CanvasAnnotationColor, CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth,
        CanvasTextSize,
    };
    use crate::library::storage::Library;

    #[test]
    fn undo_and_redo_annotation_archive_restores_document_state() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-annotation-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "hello",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                CanvasAnnotationColor::Sky,
                true,
            )
            .unwrap();
        let mut archived = annotation.clone();
        archived.archived = true;
        let mut history = DocumentHistory::default();

        history
            .apply(
                &library,
                HistoryAction {
                    label: "Delete annotation".to_owned(),
                    placements: Vec::new(),
                    annotations: vec![RowChange {
                        before: Some(annotation.clone()),
                        after: Some(archived),
                    }],
                    stack_order: None,
                },
            )
            .unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        assert_eq!(
            history.undo(&library).unwrap().map(|applied| applied.label),
            Some("Delete annotation".to_owned())
        );
        assert_eq!(
            library.load_canvas_annotations().unwrap(),
            vec![annotation.clone()]
        );
        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            vec![CanvasStackItem::Annotation(annotation.id)]
        );

        assert_eq!(
            history.redo(&library).unwrap().map(|applied| applied.label),
            Some("Delete annotation".to_owned())
        );
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn undo_and_redo_placement_archive_restores_document_state() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-placement-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let mut archived = record.clone();
        archived.state.archived = true;
        let mut history = DocumentHistory::default();

        history
            .apply(
                &library,
                HistoryAction {
                    label: "Delete image".to_owned(),
                    placements: vec![RowChange {
                        before: Some(record.clone()),
                        after: Some(archived),
                    }],
                    annotations: Vec::new(),
                    stack_order: None,
                },
            )
            .unwrap();
        assert!(library.load_placements().unwrap().is_empty());

        assert_eq!(
            history.undo(&library).unwrap().map(|applied| applied.label),
            Some("Delete image".to_owned())
        );
        let restored = library.load_placements().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].state.id, record.state.id);
        assert_eq!(restored[0].state.geometry.center.x, 10.0);
        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            vec![CanvasStackItem::Placement(record.state.id)]
        );

        assert_eq!(
            history.redo(&library).unwrap().map(|applied| applied.label),
            Some("Delete image".to_owned())
        );
        assert!(library.load_placements().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn undo_and_redo_stack_order_restores_order() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-stack-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let first = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let second = library
            .create_placement_from_asset(
                first.asset.as_ref().expect("loaded placement asset").id,
                egui::pos2(30.0, 40.0),
                1.0,
                0.0,
                0,
            )
            .unwrap();
        let before = vec![
            CanvasStackItem::Placement(first.state.id),
            CanvasStackItem::Placement(second.state.id),
        ];
        let after = vec![
            CanvasStackItem::Placement(second.state.id),
            CanvasStackItem::Placement(first.state.id),
        ];
        library.save_canvas_stack_order(&before).unwrap();
        let mut history = DocumentHistory::default();

        history
            .apply(
                &library,
                HistoryAction {
                    label: "Move to front".to_owned(),
                    placements: Vec::new(),
                    annotations: Vec::new(),
                    stack_order: Some(RowChange {
                        before: Some(before.clone()),
                        after: Some(after.clone()),
                    }),
                },
            )
            .unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), after);

        history.undo(&library).unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), before);

        history.redo(&library).unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), after);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn undo_and_redo_created_rows_archives_and_restores_existing_rows() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-created-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "created",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                CanvasAnnotationColor::Sky,
                true,
            )
            .unwrap();
        let mut history = DocumentHistory::default();

        history.record_applied(HistoryAction {
            label: "Create annotation".to_owned(),
            placements: Vec::new(),
            annotations: vec![RowChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            stack_order: None,
        });

        history.undo(&library).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        history.redo(&library).unwrap();
        assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);

        let _ = std::fs::remove_dir_all(root);
    }
}
