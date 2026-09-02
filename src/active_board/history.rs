use std::collections::HashMap;

use crate::data::{CanvasAnnotation, CanvasStackItem, Placement};

#[derive(Clone, Default)]
pub(crate) struct DocumentHistory {
    histories_by_board: HashMap<i64, BoardHistory>,
}

#[derive(Clone, Default)]
struct BoardHistory {
    undo_stack: Vec<HistoryAction>,
    redo_stack: Vec<HistoryAction>,
}

#[derive(Clone, Debug)]
pub(crate) enum HistoryAction {
    Document(DocumentHistoryAction),
    Board(BoardLifecycleHistoryAction),
}

#[derive(Clone, Debug)]
pub(crate) enum BoardLifecycleHistoryAction {
    Created {
        document: crate::data::BoardDocumentSnapshot,
        active_before: i64,
    },
    Renamed {
        board_id: i64,
        before_name: String,
        after_name: String,
    },
    Deleted {
        document: crate::data::BoardDocumentSnapshot,
        active_before: i64,
        active_after: i64,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentHistoryAction {
    pub(crate) label: String,
    pub(crate) placements: Vec<HistoryChange<Placement>>,
    pub(crate) annotations: Vec<HistoryChange<CanvasAnnotation>>,
    pub(crate) stack_order: Option<HistoryChange<Vec<CanvasStackItem>>>,
}

impl HistoryAction {
    pub(crate) fn document(
        label: String,
        placements: Vec<HistoryChange<Placement>>,
        annotations: Vec<HistoryChange<CanvasAnnotation>>,
        stack_order: Option<HistoryChange<Vec<CanvasStackItem>>>,
    ) -> Self {
        Self::Document(DocumentHistoryAction {
            label,
            placements,
            annotations,
            stack_order,
        })
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Document(action) => &action.label,
            Self::Board(BoardLifecycleHistoryAction::Created { .. }) => "Create board",
            Self::Board(BoardLifecycleHistoryAction::Renamed { .. }) => "Rename board",
            Self::Board(BoardLifecycleHistoryAction::Deleted { .. }) => "Delete board",
        }
    }

    pub(crate) fn owner_after_replay(
        &self,
        current_owner: i64,
        direction: HistoryDirection,
    ) -> i64 {
        match (self, direction) {
            (
                Self::Board(BoardLifecycleHistoryAction::Created { active_before, .. }),
                HistoryDirection::Backward,
            ) => *active_before,
            (
                Self::Board(BoardLifecycleHistoryAction::Created { document, .. }),
                HistoryDirection::Forward,
            ) => document.board.id,
            (
                Self::Board(BoardLifecycleHistoryAction::Deleted { active_before, .. }),
                HistoryDirection::Backward,
            ) => *active_before,
            (
                Self::Board(BoardLifecycleHistoryAction::Deleted { active_after, .. }),
                HistoryDirection::Forward,
            ) => *active_after,
            _ => current_owner,
        }
    }
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
    pub(crate) fn encode(&self) -> Result<String, String> {
        serde_json::to_string(&StoredDocumentHistory::from(self))
            .map_err(|error| format!("failed to encode Board history: {error}"))
    }

    pub(crate) fn decode(
        payload: &str,
        library: &crate::library::storage::Library,
    ) -> Result<Self, String> {
        let stored: StoredDocumentHistory = serde_json::from_str(payload)
            .map_err(|error| format!("failed to decode Board history: {error}"))?;
        stored.into_history(library)
    }

    pub(crate) fn accepted_placement(&self, board_id: i64, placement_id: i64) -> Option<Placement> {
        let history = self.histories_by_board.get(&board_id)?;
        if let Some(change) = history
            .undo_stack
            .iter()
            .rev()
            .find_map(|action| placement_change(action, placement_id))
        {
            return change.after.clone();
        }
        history
            .redo_stack
            .iter()
            .rev()
            .find_map(|action| placement_change(action, placement_id))
            .and_then(|change| change.placement_target(HistoryDirection::Backward))
    }

    pub(crate) fn update_accepted_placement(
        &mut self,
        board_id: i64,
        placement: Placement,
    ) -> bool {
        let Some(history) = self.histories_by_board.get_mut(&board_id) else {
            return false;
        };
        for action in history.undo_stack.iter_mut().rev() {
            let HistoryAction::Document(action) = action else {
                continue;
            };
            if let Some(change) = action.placements.iter_mut().find(|change| {
                change
                    .after
                    .as_ref()
                    .or(change.before.as_ref())
                    .is_some_and(|candidate| candidate.state.id == placement.state.id)
            }) {
                change.after = Some(placement);
                return true;
            }
        }
        false
    }

    pub(crate) fn fail_accepted_placement(
        &mut self,
        board_id: i64,
        placement_id: i64,
        message: String,
    ) -> Option<Placement> {
        let mut placement = self.accepted_placement(board_id, placement_id)?;
        let source = match &placement.content {
            crate::data::PlacementContent::Loading { source, .. } => Some(source.clone()),
            crate::data::PlacementContent::Failed { source, .. } => source.clone(),
            crate::data::PlacementContent::Loaded { .. } => None,
        };
        placement.content = crate::data::PlacementContent::Failed {
            source,
            message,
            recoverable: true,
        };
        placement.asset = None;
        placement.state.asset_id = None;
        self.update_accepted_placement(board_id, placement.clone())
            .then_some(placement)
    }

    pub(crate) fn accepted_annotation(
        &self,
        board_id: i64,
        annotation_id: i64,
    ) -> Option<CanvasAnnotation> {
        self.histories_by_board
            .get(&board_id)?
            .undo_stack
            .iter()
            .rev()
            .filter_map(|action| match action {
                HistoryAction::Document(action) => Some(&action.annotations),
                HistoryAction::Board(_) => None,
            })
            .flatten()
            .find(|change| {
                change
                    .after
                    .as_ref()
                    .or(change.before.as_ref())
                    .is_some_and(|annotation| annotation.id == annotation_id)
            })
            .and_then(|change| change.after.clone())
    }

    pub(crate) fn accepted_stack_order(&self, board_id: i64) -> Option<Vec<CanvasStackItem>> {
        self.histories_by_board
            .get(&board_id)?
            .undo_stack
            .iter()
            .rev()
            .find_map(|action| match action {
                HistoryAction::Document(action) => action
                    .stack_order
                    .as_ref()
                    .and_then(|change| change.after.clone()),
                HistoryAction::Board(_) => None,
            })
    }

    pub(crate) fn apply_accepted_document_state(
        &self,
        board_id: i64,
        document: &mut crate::data::BoardDocumentSnapshot,
    ) {
        let Some(history) = self.histories_by_board.get(&board_id) else {
            return;
        };
        for action in &history.undo_stack {
            apply_document_action(document, action, HistoryDirection::Forward);
        }
        for action in &history.redo_stack {
            apply_document_action(document, action, HistoryDirection::Backward);
        }
    }

    pub(crate) fn accepted_document_for_created_board(
        &self,
        board_id: i64,
    ) -> Option<crate::data::BoardDocumentSnapshot> {
        let history = self.histories_by_board.get(&board_id)?;
        let mut document = history.undo_stack.iter().find_map(|action| match action {
            HistoryAction::Board(BoardLifecycleHistoryAction::Created { document, .. })
                if document.board.id == board_id =>
            {
                Some(document.clone())
            }
            _ => None,
        })?;
        self.apply_accepted_document_state(board_id, &mut document);
        Some(document)
    }

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

    pub(crate) fn reassign_applied(
        &mut self,
        from_board_id: i64,
        to_board_id: i64,
        direction: HistoryDirection,
    ) {
        if from_board_id == to_board_id {
            return;
        }
        let action = {
            let source = self
                .histories_by_board
                .get_mut(&from_board_id)
                .expect("replayed history owner should exist");
            match direction {
                HistoryDirection::Backward => source.redo_stack.pop(),
                HistoryDirection::Forward => source.undo_stack.pop(),
            }
            .expect("replayed history action should be committed")
        };
        let target = self.histories_by_board.entry(to_board_id).or_default();
        match direction {
            HistoryDirection::Backward => target.redo_stack.push(action),
            HistoryDirection::Forward => target.undo_stack.push(action),
        }
    }
}

fn placement_change(
    action: &HistoryAction,
    placement_id: i64,
) -> Option<&HistoryChange<Placement>> {
    let HistoryAction::Document(action) = action else {
        return None;
    };
    action.placements.iter().find(|change| {
        change
            .after
            .as_ref()
            .or(change.before.as_ref())
            .is_some_and(|placement| placement.state.id == placement_id)
    })
}

fn apply_document_action(
    document: &mut crate::data::BoardDocumentSnapshot,
    action: &HistoryAction,
    direction: HistoryDirection,
) {
    let HistoryAction::Document(action) = action else {
        return;
    };
    for change in &action.placements {
        let Some(target) = change.placement_target(direction) else {
            continue;
        };
        document
            .placements
            .retain(|placement| placement.state.id != target.state.id);
        if !target.state.archived {
            document.placements.push(target);
        }
    }
    for change in &action.annotations {
        let Some(target) = change.annotation_target(direction) else {
            continue;
        };
        document
            .annotations
            .retain(|annotation| annotation.id != target.id);
        if !target.archived {
            document.annotations.push(target);
        }
    }
    if let Some(stack_order) = action
        .stack_order
        .as_ref()
        .and_then(|change| change.target(direction))
    {
        document.stack_order = stack_order;
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredDocumentHistory {
    histories_by_board: HashMap<i64, StoredBoardHistory>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredBoardHistory {
    undo_stack: Vec<StoredHistoryAction>,
    redo_stack: Vec<StoredHistoryAction>,
}

#[derive(serde::Deserialize, serde::Serialize)]
enum StoredHistoryAction {
    Document {
        label: String,
        placements: Vec<StoredHistoryChange<StoredPlacement>>,
        annotations: Vec<StoredHistoryChange<StoredAnnotation>>,
        stack_order: Option<StoredHistoryChange<Vec<(String, i64)>>>,
    },
    Created {
        document: StoredBoardDocument,
        active_before: i64,
    },
    Renamed {
        board_id: i64,
        before_name: String,
        after_name: String,
    },
    Deleted {
        document: StoredBoardDocument,
        active_before: i64,
        active_after: i64,
    },
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredHistoryChange<T> {
    before: Option<T>,
    after: Option<T>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredPlacement {
    id: i64,
    asset_id: Option<i64>,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    scale: f32,
    rotation: f32,
    z_index: i32,
    archived: bool,
    content: crate::data::PlacementContent,
    lifecycle: crate::data::PlacementLifecycle,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredAnnotation {
    id: i64,
    kind: String,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    text: String,
    text_size: String,
    stroke_width: String,
    color: String,
    background_enabled: bool,
    archived: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StoredBoardDocument {
    board_id: i64,
    board_name: String,
    board_archived: bool,
    placements: Vec<StoredPlacement>,
    annotations: Vec<StoredAnnotation>,
    stack_order: Vec<(String, i64)>,
    camera_pan_x: f32,
    camera_pan_y: f32,
    camera_zoom: f32,
    canvas_background: String,
}

impl From<&DocumentHistory> for StoredDocumentHistory {
    fn from(history: &DocumentHistory) -> Self {
        Self {
            histories_by_board: history
                .histories_by_board
                .iter()
                .map(|(board_id, history)| {
                    (
                        *board_id,
                        StoredBoardHistory {
                            undo_stack: history
                                .undo_stack
                                .iter()
                                .map(StoredHistoryAction::from)
                                .collect(),
                            redo_stack: history
                                .redo_stack
                                .iter()
                                .map(StoredHistoryAction::from)
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl StoredDocumentHistory {
    fn into_history(
        self,
        library: &crate::library::storage::Library,
    ) -> Result<DocumentHistory, String> {
        Ok(DocumentHistory {
            histories_by_board: self
                .histories_by_board
                .into_iter()
                .map(|(board_id, history)| {
                    Ok((
                        board_id,
                        BoardHistory {
                            undo_stack: history
                                .undo_stack
                                .into_iter()
                                .map(|action| action.into_action(library))
                                .collect::<Result<_, _>>()?,
                            redo_stack: history
                                .redo_stack
                                .into_iter()
                                .map(|action| action.into_action(library))
                                .collect::<Result<_, _>>()?,
                        },
                    ))
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

impl From<&HistoryAction> for StoredHistoryAction {
    fn from(action: &HistoryAction) -> Self {
        match action {
            HistoryAction::Document(action) => Self::Document {
                label: action.label.clone(),
                placements: action.placements.iter().map(stored_change).collect(),
                annotations: action.annotations.iter().map(stored_change).collect(),
                stack_order: action
                    .stack_order
                    .as_ref()
                    .map(|change| StoredHistoryChange {
                        before: change
                            .before
                            .as_ref()
                            .map(|items| items.iter().map(stored_stack_item).collect()),
                        after: change
                            .after
                            .as_ref()
                            .map(|items| items.iter().map(stored_stack_item).collect()),
                    }),
            },
            HistoryAction::Board(BoardLifecycleHistoryAction::Created {
                document,
                active_before,
            }) => Self::Created {
                document: StoredBoardDocument::from(document),
                active_before: *active_before,
            },
            HistoryAction::Board(BoardLifecycleHistoryAction::Renamed {
                board_id,
                before_name,
                after_name,
            }) => Self::Renamed {
                board_id: *board_id,
                before_name: before_name.clone(),
                after_name: after_name.clone(),
            },
            HistoryAction::Board(BoardLifecycleHistoryAction::Deleted {
                document,
                active_before,
                active_after,
            }) => Self::Deleted {
                document: StoredBoardDocument::from(document),
                active_before: *active_before,
                active_after: *active_after,
            },
        }
    }
}

fn stored_change<T, U>(change: &HistoryChange<T>) -> StoredHistoryChange<U>
where
    for<'a> U: From<&'a T>,
{
    StoredHistoryChange {
        before: change.before.as_ref().map(U::from),
        after: change.after.as_ref().map(U::from),
    }
}

fn stored_stack_item(item: &CanvasStackItem) -> (String, i64) {
    (item.kind().to_owned(), item.item_id())
}

fn load_stack_item((kind, id): (String, i64)) -> Result<CanvasStackItem, String> {
    CanvasStackItem::from_parts(&kind, id)
        .ok_or_else(|| format!("unknown stored stack item kind: {kind}"))
}

impl StoredHistoryAction {
    fn into_action(
        self,
        library: &crate::library::storage::Library,
    ) -> Result<HistoryAction, String> {
        Ok(match self {
            Self::Document {
                label,
                placements,
                annotations,
                stack_order,
            } => HistoryAction::document(
                label,
                placements
                    .into_iter()
                    .map(|change| change.into_placement_change(library))
                    .collect::<Result<_, _>>()?,
                annotations
                    .into_iter()
                    .map(StoredHistoryChange::into_annotation_change)
                    .collect::<Result<_, _>>()?,
                stack_order
                    .map(|change| change.into_stack_change())
                    .transpose()?,
            ),
            Self::Created {
                document,
                active_before,
            } => HistoryAction::Board(BoardLifecycleHistoryAction::Created {
                document: document.into_document(library)?,
                active_before,
            }),
            Self::Renamed {
                board_id,
                before_name,
                after_name,
            } => HistoryAction::Board(BoardLifecycleHistoryAction::Renamed {
                board_id,
                before_name,
                after_name,
            }),
            Self::Deleted {
                document,
                active_before,
                active_after,
            } => HistoryAction::Board(BoardLifecycleHistoryAction::Deleted {
                document: document.into_document(library)?,
                active_before,
                active_after,
            }),
        })
    }
}

impl StoredHistoryChange<StoredPlacement> {
    fn into_placement_change(
        self,
        library: &crate::library::storage::Library,
    ) -> Result<HistoryChange<Placement>, String> {
        Ok(HistoryChange {
            before: self
                .before
                .map(|placement| placement.into_placement(library))
                .transpose()?,
            after: self
                .after
                .map(|placement| placement.into_placement(library))
                .transpose()?,
        })
    }
}

impl StoredHistoryChange<StoredAnnotation> {
    fn into_annotation_change(self) -> Result<HistoryChange<CanvasAnnotation>, String> {
        Ok(HistoryChange {
            before: self
                .before
                .map(StoredAnnotation::into_annotation)
                .transpose()?,
            after: self
                .after
                .map(StoredAnnotation::into_annotation)
                .transpose()?,
        })
    }
}

impl StoredHistoryChange<Vec<(String, i64)>> {
    fn into_stack_change(self) -> Result<HistoryChange<Vec<CanvasStackItem>>, String> {
        Ok(HistoryChange {
            before: self
                .before
                .map(|items| items.into_iter().map(load_stack_item).collect())
                .transpose()?,
            after: self
                .after
                .map(|items| items.into_iter().map(load_stack_item).collect())
                .transpose()?,
        })
    }
}

impl From<&Placement> for StoredPlacement {
    fn from(placement: &Placement) -> Self {
        Self {
            id: placement.state.id,
            asset_id: placement.state.asset_id,
            center_x: placement.state.geometry.center.x,
            center_y: placement.state.geometry.center.y,
            width: placement.state.geometry.size.x,
            height: placement.state.geometry.size.y,
            scale: placement.state.geometry.scale,
            rotation: placement.state.geometry.rotation,
            z_index: placement.state.geometry.z_index,
            archived: placement.state.archived,
            content: placement.content.clone(),
            lifecycle: placement.lifecycle,
        }
    }
}

impl StoredPlacement {
    fn into_placement(
        self,
        library: &crate::library::storage::Library,
    ) -> Result<Placement, String> {
        let state = crate::data::PlacementState::with_display_size(
            self.id,
            self.asset_id,
            eframe::egui::pos2(self.center_x, self.center_y),
            eframe::egui::vec2(self.width, self.height),
            self.scale,
            self.rotation,
            self.z_index,
            self.archived,
        );
        let asset = self
            .asset_id
            .map(|asset_id| library.load_asset(asset_id))
            .transpose()?;
        Ok(Placement {
            state,
            asset,
            content: self.content,
            lifecycle: self.lifecycle,
        })
    }
}

impl From<&CanvasAnnotation> for StoredAnnotation {
    fn from(annotation: &CanvasAnnotation) -> Self {
        Self {
            id: annotation.id,
            kind: annotation.kind.as_str().to_owned(),
            start_x: annotation.start().x,
            start_y: annotation.start().y,
            end_x: annotation.end().x,
            end_y: annotation.end().y,
            text: annotation.text.clone(),
            text_size: annotation.text_size.as_str().to_owned(),
            stroke_width: annotation.stroke_width.as_str().to_owned(),
            color: annotation.color.as_str().to_owned(),
            background_enabled: annotation.background_enabled,
            archived: annotation.archived,
        }
    }
}

impl StoredAnnotation {
    fn into_annotation(self) -> Result<CanvasAnnotation, String> {
        Ok(CanvasAnnotation::new(
            self.id,
            crate::data::CanvasAnnotationKind::from_str(&self.kind)
                .ok_or_else(|| format!("unknown stored annotation kind: {}", self.kind))?,
            eframe::egui::pos2(self.start_x, self.start_y),
            eframe::egui::pos2(self.end_x, self.end_y),
            self.text,
            crate::data::CanvasTextSize::from_str(&self.text_size)
                .ok_or_else(|| format!("unknown stored text size: {}", self.text_size))?,
            crate::data::CanvasStrokeWidth::from_str(&self.stroke_width)
                .ok_or_else(|| format!("unknown stored stroke width: {}", self.stroke_width))?,
            crate::data::CanvasAnnotationColor::from_str(&self.color)
                .ok_or_else(|| format!("unknown stored annotation color: {}", self.color))?,
            self.background_enabled,
            self.archived,
        ))
    }
}

impl From<&crate::data::BoardDocumentSnapshot> for StoredBoardDocument {
    fn from(document: &crate::data::BoardDocumentSnapshot) -> Self {
        Self {
            board_id: document.board.id,
            board_name: document.board.name.clone(),
            board_archived: document.board.archived,
            placements: document
                .placements
                .iter()
                .map(StoredPlacement::from)
                .collect(),
            annotations: document
                .annotations
                .iter()
                .map(StoredAnnotation::from)
                .collect(),
            stack_order: document.stack_order.iter().map(stored_stack_item).collect(),
            camera_pan_x: document.viewport.camera_pan_x,
            camera_pan_y: document.viewport.camera_pan_y,
            camera_zoom: document.viewport.camera_zoom,
            canvas_background: document.viewport.canvas_background.as_str().to_owned(),
        }
    }
}

impl StoredBoardDocument {
    fn into_document(
        self,
        library: &crate::library::storage::Library,
    ) -> Result<crate::data::BoardDocumentSnapshot, String> {
        Ok(crate::data::BoardDocumentSnapshot {
            board: crate::data::Board {
                id: self.board_id,
                name: self.board_name,
                archived: self.board_archived,
            },
            placements: self
                .placements
                .into_iter()
                .map(|placement| placement.into_placement(library))
                .collect::<Result<_, _>>()?,
            annotations: self
                .annotations
                .into_iter()
                .map(StoredAnnotation::into_annotation)
                .collect::<Result<_, _>>()?,
            stack_order: self
                .stack_order
                .into_iter()
                .map(load_stack_item)
                .collect::<Result<_, _>>()?,
            viewport: crate::data::ViewportState {
                camera_pan_x: self.camera_pan_x,
                camera_pan_y: self.camera_pan_y,
                camera_zoom: self.camera_zoom,
                canvas_background: crate::data::CanvasBackground::from_str(&self.canvas_background)
                    .ok_or_else(|| {
                        format!(
                            "unknown stored canvas background: {}",
                            self.canvas_background
                        )
                    })?,
            },
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
        HistoryAction::document(label.to_owned(), Vec::new(), Vec::new(), None)
    }

    #[test]
    fn undo_and_redo_return_the_document_action_in_order() {
        let mut history = DocumentHistory::default();
        history.record_applied(1, action("First"));
        history.record_applied(1, action("Second"));

        let undone = history.undo(1).unwrap();
        assert_eq!(undone.action.label(), "Second");
        assert_eq!(undone.direction, HistoryDirection::Backward);

        let redone = history.redo(1).unwrap();
        assert_eq!(redone.action.label(), "Second");
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
        assert_eq!(history.undo(1).unwrap().action.label(), "Board A");
    }

    #[test]
    fn each_board_preserves_its_undo_and_redo_state() {
        let mut history = DocumentHistory::default();
        history.record_applied(1, action("Board A"));
        history.record_applied(2, action("Board B"));

        assert_eq!(history.undo(2).unwrap().action.label(), "Board B");
        assert_eq!(history.undo(1).unwrap().action.label(), "Board A");
        assert_eq!(history.redo(1).unwrap().action.label(), "Board A");
        assert_eq!(history.redo(2).unwrap().action.label(), "Board B");
    }

    #[test]
    fn replay_can_hand_an_entry_to_the_new_active_board() {
        let mut history = DocumentHistory::default();
        history.record_applied(2, action("Create board"));

        history.undo(2).unwrap();
        history.reassign_applied(2, 1, HistoryDirection::Backward);

        assert!(history.redo(2).is_none());
        assert_eq!(history.redo(1).unwrap().action.label(), "Create board");
        history.reassign_applied(1, 2, HistoryDirection::Forward);
        assert_eq!(history.undo(2).unwrap().action.label(), "Create board");
    }
}
