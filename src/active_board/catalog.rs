use crate::data::{BoardDocumentSnapshot, BoardSummary, Placement};
use std::collections::{HashMap, HashSet};

/// Accepted Board metadata and lazily loaded documents. Durable refreshes only
/// discover Boards; accepted changes remain authoritative until restart.
#[derive(Default)]
pub(crate) struct AcceptedBoards {
    summaries: Vec<BoardSummary>,
    documents: std::cell::RefCell<HashMap<i64, BoardDocumentSnapshot>>,
    archived: HashSet<i64>,
}

impl AcceptedBoards {
    pub(crate) fn summaries(&self) -> &[BoardSummary] {
        &self.summaries
    }

    pub(crate) fn refresh_from(
        &mut self,
        library: &crate::library::storage::Library,
    ) -> Result<(), String> {
        for summary in library.list_board_summaries()? {
            if self.archived.contains(&summary.board.id) {
                continue;
            }
            if let Some(existing) = self
                .summaries
                .iter_mut()
                .find(|accepted| accepted.board.id == summary.board.id)
            {
                if !self.documents.get_mut().contains_key(&summary.board.id) {
                    *existing = summary;
                }
            } else {
                self.summaries.push(summary);
            }
        }
        self.summaries.sort_by_key(|summary| summary.board.id);
        Ok(())
    }

    pub(crate) fn document(
        &self,
        library: &crate::library::storage::Library,
        board_id: i64,
    ) -> Result<BoardDocumentSnapshot, String> {
        if self.archived.contains(&board_id) {
            return Err("Board unavailable".to_owned());
        }
        self.ensure_loaded(library, board_id)?;
        Ok(self.documents.borrow()[&board_id].clone())
    }

    pub(crate) fn ensure_loaded(
        &self,
        library: &crate::library::storage::Library,
        board_id: i64,
    ) -> Result<(), String> {
        if !self.documents.borrow().contains_key(&board_id) {
            let document = library.load_board_document(board_id)?;
            self.documents.borrow_mut().insert(board_id, document);
        }
        Ok(())
    }

    pub(crate) fn placement(&self, board_id: i64, placement_id: i64) -> Option<Placement> {
        self.documents
            .borrow()
            .get(&board_id)?
            .placements
            .iter()
            .find(|placement| placement.state.id == placement_id)
            .cloned()
    }

    pub(crate) fn restore_document(&mut self, document: &BoardDocumentSnapshot) {
        self.archived.remove(&document.board.id);
        self.documents
            .get_mut()
            .insert(document.board.id, document.clone());
        self.update_summary(document.board.id);
    }

    pub(crate) fn archive(&mut self, board_id: i64) {
        self.archived.insert(board_id);
        self.documents.get_mut().remove(&board_id);
        self.summaries
            .retain(|summary| summary.board.id != board_id);
    }

    pub(crate) fn rename(&mut self, board_id: i64, name: &str) {
        if let Some(document) = self.documents.get_mut().get_mut(&board_id) {
            document.board.name = name.to_owned();
        }
        if let Some(summary) = self
            .summaries
            .iter_mut()
            .find(|summary| summary.board.id == board_id)
        {
            summary.board.name = name.to_owned();
        }
    }

    pub(crate) fn apply_changes(
        &mut self,
        board_id: i64,
        changes: &crate::canvas::CanvasDocumentChanges,
    ) {
        let document = self
            .documents
            .get_mut()
            .get_mut(&board_id)
            .expect("accepted Board must be loaded before editing");
        document
            .placements
            .retain(|placement| !changes.archived_placement_ids.contains(&placement.id()));
        document
            .annotations
            .retain(|annotation| !changes.archived_annotation_ids.contains(&annotation.id));
        for placement in &changes.updated_placements {
            document
                .placements
                .retain(|existing| existing.id() != placement.id());
            if !placement.state.archived {
                document.placements.push(placement.clone());
            }
        }
        for annotation in &changes.updated_annotations {
            document
                .annotations
                .retain(|existing| existing.id != annotation.id);
            if !annotation.archived {
                document.annotations.push(annotation.clone());
            }
        }
        if let Some(groups) = &changes.groups {
            document.groups = groups.clone();
        }
        if let Some(stack) = &changes.stack_order {
            document.stack_order = stack.clone();
        }
        self.update_summary(board_id);
    }

    pub(crate) fn set_viewport(&mut self, board_id: i64, viewport: crate::data::ViewportState) {
        if let Some(document) = self.documents.get_mut().get_mut(&board_id) {
            document.viewport = viewport;
        }
    }

    pub(crate) fn remove_assets(&mut self, asset_ids: &[i64]) {
        let board_ids = self.documents.get_mut().keys().copied().collect::<Vec<_>>();
        for board_id in board_ids {
            let document = &self.documents.get_mut()[&board_id];
            let archived_placement_ids = document
                .placements
                .iter()
                .filter(|placement| {
                    placement
                        .asset_id()
                        .is_some_and(|id| asset_ids.contains(&id))
                })
                .map(Placement::id)
                .collect::<Vec<_>>();
            let stack_order = document.stack_order.iter().copied().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if archived_placement_ids.contains(id))).collect();
            let mut groups = document.groups.clone();
            for group in &mut groups {
                group
                    .members
                    .retain(|id| !archived_placement_ids.contains(id));
            }
            self.apply_changes(
                board_id,
                &crate::canvas::CanvasDocumentChanges {
                    archived_placement_ids,
                    groups: Some(groups),
                    stack_order: Some(stack_order),
                    ..Default::default()
                },
            );
        }
    }

    pub(crate) fn remap_assets(&mut self, assets: &HashMap<i64, crate::data::Asset>) {
        for document in self.documents.get_mut().values_mut() {
            for placement in &mut document.placements {
                super::editing::remap_placement_asset(placement, assets);
            }
        }
    }

    fn update_summary(&mut self, board_id: i64) {
        let Some(document) = self.documents.get_mut().get(&board_id) else {
            return;
        };
        let summary = BoardSummary {
            board: document.board.clone(),
            placement_count: document.placements.len(),
        };
        self.summaries
            .retain(|existing| existing.board.id != board_id);
        self.summaries.push(summary);
        self.summaries.sort_by_key(|summary| summary.board.id);
    }
}
