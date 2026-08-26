use std::collections::HashMap;

use crate::data::{CanvasAnnotation, CanvasStackItem, Placement};

#[derive(Default)]
pub(crate) struct DocumentHistory {
    histories_by_board: HashMap<i64, BoardHistory>,
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

impl<T: Clone> HistoryChange<T> {
    pub(crate) fn target(&self, direction: HistoryDirection) -> Option<T> {
        match direction {
            HistoryDirection::Forward => self.after.clone(),
            HistoryDirection::Backward => self.before.clone(),
        }
    }
}

impl HistoryChange<Placement> {
    pub(crate) fn placement_target(&self, direction: HistoryDirection) -> Option<Placement> {
        self.target(direction).or_else(|| {
            self.target(direction.opposite()).map(|mut placement| {
                placement.state.archived = true;
                placement
            })
        })
    }
}

impl HistoryChange<CanvasAnnotation> {
    pub(crate) fn annotation_target(
        &self,
        direction: HistoryDirection,
    ) -> Option<CanvasAnnotation> {
        self.target(direction).or_else(|| {
            self.target(direction.opposite()).map(|mut annotation| {
                annotation.archived = true;
                annotation
            })
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryApplied {
    pub(crate) action: HistoryAction,
    pub(crate) direction: HistoryDirection,
}

impl DocumentHistory {
    pub(crate) fn record_applied(&mut self, board_id: i64, action: HistoryAction) {
        let history = self.histories_by_board.entry(board_id).or_default();
        history.undo_stack.push(action);
        history.redo_stack.clear();
    }

    pub(crate) fn undo(&mut self, board_id: i64) -> Option<HistoryApplied> {
        let history = self.histories_by_board.get_mut(&board_id)?;
        let action = history.undo_stack.pop()?;
        history.redo_stack.push(action.clone());
        Some(HistoryApplied {
            action,
            direction: HistoryDirection::Backward,
        })
    }

    pub(crate) fn redo(&mut self, board_id: i64) -> Option<HistoryApplied> {
        let history = self.histories_by_board.get_mut(&board_id)?;
        let action = history.redo_stack.pop()?;
        history.undo_stack.push(action.clone());
        Some(HistoryApplied {
            action,
            direction: HistoryDirection::Forward,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryDirection {
    Forward,
    Backward,
}

impl HistoryDirection {
    fn opposite(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::{DocumentHistory, HistoryAction, HistoryChange, HistoryDirection};
    use crate::data::{
        CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasStrokeWidth,
        CanvasTextSize,
    };

    fn action(label: &str) -> HistoryAction {
        HistoryAction {
            label: label.to_owned(),
            placements: Vec::new(),
            annotations: Vec::new(),
            stack_order: None,
        }
    }

    #[test]
    fn undo_and_redo_return_the_document_action_in_order() {
        let mut history = DocumentHistory::default();
        history.record_applied(1, action("First"));
        history.record_applied(1, action("Second"));

        let undone = history.undo(1).unwrap();
        assert_eq!(undone.action.label, "Second");
        assert_eq!(undone.direction, HistoryDirection::Backward);

        let redone = history.redo(1).unwrap();
        assert_eq!(redone.action.label, "Second");
        assert_eq!(redone.direction, HistoryDirection::Forward);
    }

    #[test]
    fn missing_created_target_becomes_archived_document_state() {
        let annotation = CanvasAnnotation::new(
            7,
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 20.0),
            egui::pos2(110.0, 80.0),
            "note".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            CanvasAnnotationColor::Sky,
            true,
            false,
        );
        let change = HistoryChange {
            before: None,
            after: Some(annotation),
        };

        assert!(
            change
                .annotation_target(HistoryDirection::Backward)
                .unwrap()
                .archived
        );
        assert!(
            !change
                .annotation_target(HistoryDirection::Forward)
                .unwrap()
                .archived
        );
    }

    #[test]
    fn untouched_board_cannot_undo_another_boards_change() {
        let mut history = DocumentHistory::default();
        history.record_applied(1, action("Board A"));

        assert!(history.undo(2).is_none());
        assert_eq!(history.undo(1).unwrap().action.label, "Board A");
    }

    #[test]
    fn each_board_preserves_its_undo_and_redo_state() {
        let mut history = DocumentHistory::default();
        history.record_applied(1, action("Board A"));
        history.record_applied(2, action("Board B"));

        assert_eq!(history.undo(2).unwrap().action.label, "Board B");
        assert_eq!(history.undo(1).unwrap().action.label, "Board A");
        assert_eq!(history.redo(1).unwrap().action.label, "Board A");
        assert_eq!(history.redo(2).unwrap().action.label, "Board B");
    }
}
