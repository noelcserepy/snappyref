use std::collections::HashMap;

use crate::data::{CanvasAnnotation, CanvasStackItem, Placement};
use crate::library::storage::Library;

#[derive(Default)]
pub(crate) struct DocumentHistory {
    boards: HashMap<i64, BoardHistory>,
}

#[derive(Default)]
struct BoardHistory {
    undo_stack: Vec<HistoryAction>,
    redo_stack: Vec<HistoryAction>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryAction {
    pub(crate) label: String,
    pub(crate) placements: Vec<HistoryChange<Placement>>,
    pub(crate) annotations: Vec<HistoryChange<CanvasAnnotation>>,
    pub(crate) stack_order: Option<HistoryChange<Vec<CanvasStackItem>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryChange<T> {
    pub(crate) before: Option<T>,
    pub(crate) after: Option<T>,
}

impl<T> HistoryChange<T> {
    fn replay_target(&self, direction: HistoryDirection) -> Option<(&T, bool)> {
        let (target, fallback) = match direction {
            HistoryDirection::Forward => (self.after.as_ref(), self.before.as_ref()),
            HistoryDirection::Backward => (self.before.as_ref(), self.after.as_ref()),
        };
        let target_missing = target.is_none();
        target.or(fallback).map(|item| (item, target_missing))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryApplied {
    pub(crate) label: String,
    pub(crate) action: HistoryAction,
    pub(crate) direction: HistoryDirection,
}

impl DocumentHistory {
    pub(crate) fn record_applied(&mut self, board_id: i64, action: HistoryAction) {
        let history = self.boards.entry(board_id).or_default();
        history.undo_stack.push(action);
        history.redo_stack.clear();
    }

    #[cfg(test)]
    pub(crate) fn apply(
        &mut self,
        board_id: i64,
        library: &Library,
        action: HistoryAction,
    ) -> Result<(), String> {
        apply_history_action(library, &action, HistoryDirection::Forward)?;
        self.record_applied(board_id, action);
        Ok(())
    }

    pub(crate) fn undo(
        &mut self,
        board_id: i64,
        library: &Library,
    ) -> Result<Option<HistoryApplied>, String> {
        let Some(history) = self.boards.get_mut(&board_id) else {
            return Ok(None);
        };
        let Some(action) = history.undo_stack.pop() else {
            return Ok(None);
        };
        apply_history_action(library, &action, HistoryDirection::Backward)?;
        let label = action.label.clone();
        history.redo_stack.push(action.clone());
        Ok(Some(HistoryApplied {
            label,
            action,
            direction: HistoryDirection::Backward,
        }))
    }

    pub(crate) fn redo(
        &mut self,
        board_id: i64,
        library: &Library,
    ) -> Result<Option<HistoryApplied>, String> {
        let Some(history) = self.boards.get_mut(&board_id) else {
            return Ok(None);
        };
        let Some(action) = history.redo_stack.pop() else {
            return Ok(None);
        };
        apply_history_action(library, &action, HistoryDirection::Forward)?;
        let label = action.label.clone();
        history.undo_stack.push(action.clone());
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
    change: &HistoryChange<Placement>,
    direction: HistoryDirection,
) -> Result<(), String> {
    let Some((item, target_missing)) = change.replay_target(direction) else {
        return Ok(());
    };
    if item.state.archived || target_missing {
        library.archive_placements(&[item.state.id])
    } else {
        library.restore_placements(&[item.state.id])?;
        library.update_placement_document_state(&item.state)
    }
}

fn apply_annotation_change(
    library: &Library,
    change: &HistoryChange<CanvasAnnotation>,
    direction: HistoryDirection,
) -> Result<(), String> {
    let Some((annotation, target_missing)) = change.replay_target(direction) else {
        return Ok(());
    };
    if annotation.archived || target_missing {
        library.archive_canvas_annotations(&[annotation.id])
    } else {
        library.restore_canvas_annotations(&[annotation.id])?;
        library.update_canvas_annotation(annotation)
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::{DocumentHistory, HistoryAction, HistoryChange};
    use crate::data::{
        CanvasAnnotationColor, CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth,
        CanvasTextSize,
    };
    use crate::library::storage::Library;

    fn default_board_id(library: &Library) -> i64 {
        library.list_boards().unwrap().remove(0).id
    }

    fn create_test_annotation(
        library: &Library,
        text: &str,
        offset: f32,
    ) -> crate::data::CanvasAnnotation {
        library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(offset, offset),
                egui::pos2(offset + 100.0, offset + 60.0),
                text,
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                CanvasAnnotationColor::Sky,
                true,
            )
            .unwrap()
    }

    fn record_created_annotation(
        history: &mut DocumentHistory,
        board_id: i64,
        annotation: crate::data::CanvasAnnotation,
    ) {
        history.record_applied(
            board_id,
            HistoryAction {
                label: "Create annotation".to_owned(),
                placements: Vec::new(),
                annotations: vec![HistoryChange {
                    before: None,
                    after: Some(annotation),
                }],
                stack_order: None,
            },
        );
    }

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
                default_board_id(&library),
                &library,
                HistoryAction {
                    label: "Delete annotation".to_owned(),
                    placements: Vec::new(),
                    annotations: vec![HistoryChange {
                        before: Some(annotation.clone()),
                        after: Some(archived),
                    }],
                    stack_order: None,
                },
            )
            .unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        assert_eq!(
            history
                .undo(default_board_id(&library), &library)
                .unwrap()
                .map(|applied| applied.label),
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
            history
                .redo(default_board_id(&library), &library)
                .unwrap()
                .map(|applied| applied.label),
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
                default_board_id(&library),
                &library,
                HistoryAction {
                    label: "Delete image".to_owned(),
                    placements: vec![HistoryChange {
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
            history
                .undo(default_board_id(&library), &library)
                .unwrap()
                .map(|applied| applied.label),
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
            history
                .redo(default_board_id(&library), &library)
                .unwrap()
                .map(|applied| applied.label),
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
                first.require_loaded_asset().id,
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
                default_board_id(&library),
                &library,
                HistoryAction {
                    label: "Move to front".to_owned(),
                    placements: Vec::new(),
                    annotations: Vec::new(),
                    stack_order: Some(HistoryChange {
                        before: Some(before.clone()),
                        after: Some(after.clone()),
                    }),
                },
            )
            .unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), after);

        history.undo(default_board_id(&library), &library).unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), before);

        history.redo(default_board_id(&library), &library).unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), after);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn undo_and_redo_created_history_entries_archives_and_restores_existing_items() {
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

        history.record_applied(
            default_board_id(&library),
            HistoryAction {
                label: "Create annotation".to_owned(),
                placements: Vec::new(),
                annotations: vec![HistoryChange {
                    before: None,
                    after: Some(annotation.clone()),
                }],
                stack_order: None,
            },
        );

        history.undo(default_board_id(&library), &library).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        history.redo(default_board_id(&library), &library).unwrap();
        assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn untouched_board_cannot_undo_another_boards_change() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-board-isolation-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let board_a = library.list_boards().unwrap().remove(0);
        let board_b = library.create_board("Board B").unwrap();
        let annotation = create_test_annotation(&library, "Board A", 10.0);
        let mut history = DocumentHistory::default();
        record_created_annotation(&mut history, board_a.id, annotation.clone());

        library.set_active_board(board_b.id).unwrap();
        assert!(history.undo(board_b.id, &library).unwrap().is_none());
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        library.set_active_board(board_a.id).unwrap();
        assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_board_preserves_its_undo_and_redo_state_across_switches() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-history-board-switches-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = Library::initialize(root.clone()).unwrap();
        let board_a = library.list_boards().unwrap().remove(0);
        let board_b = library.create_board("Board B").unwrap();
        let annotation_a = create_test_annotation(&library, "Board A", 10.0);
        let mut history = DocumentHistory::default();
        record_created_annotation(&mut history, board_a.id, annotation_a.clone());

        library.set_active_board(board_b.id).unwrap();
        let annotation_b = create_test_annotation(&library, "Board B", 20.0);
        record_created_annotation(&mut history, board_b.id, annotation_b.clone());

        history.undo(board_b.id, &library).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        library.set_active_board(board_a.id).unwrap();
        history.undo(board_a.id, &library).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());
        history.redo(board_a.id, &library).unwrap();
        assert_eq!(
            library.load_canvas_annotations().unwrap(),
            vec![annotation_a.clone()]
        );

        library.set_active_board(board_b.id).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());
        history.redo(board_b.id, &library).unwrap();
        assert_eq!(
            library.load_canvas_annotations().unwrap(),
            vec![annotation_b]
        );

        library.set_active_board(board_a.id).unwrap();
        assert_eq!(
            library.load_canvas_annotations().unwrap(),
            vec![annotation_a]
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
