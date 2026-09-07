#[derive(Default)]
pub(crate) struct BoardSummariesCache {
    summaries: Vec<crate::data::BoardSummary>,
}

impl BoardSummariesCache {
    pub(crate) fn summaries(&self) -> Vec<crate::data::BoardSummary> {
        self.summaries.clone()
    }

    pub(crate) fn clear(&mut self) {
        self.summaries.clear();
    }

    pub(crate) fn refresh_from(
        &mut self,
        library: &crate::library::storage::Library,
    ) -> Result<(), String> {
        let durable = library.list_board_summaries()?;
        let accepted = std::mem::replace(&mut self.summaries, durable);
        for summary in accepted {
            if !self
                .summaries
                .iter()
                .any(|durable| durable.board.id == summary.board.id)
            {
                self.summaries.push(summary);
            }
        }
        self.summaries.sort_by_key(|summary| summary.board.id);
        Ok(())
    }

    pub(crate) fn restore_document(&mut self, document: &crate::data::BoardDocumentSnapshot) {
        self.summaries
            .retain(|summary| summary.board.id != document.board.id);
        self.summaries.push(crate::data::BoardSummary {
            board: document.board.clone(),
            placement_count: document.placements.len(),
        });
        self.summaries.sort_by_key(|summary| summary.board.id);
    }

    pub(crate) fn archive(&mut self, board_id: i64) {
        self.summaries
            .retain(|summary| summary.board.id != board_id);
    }

    pub(crate) fn rename(&mut self, board_id: i64, name: &str) {
        if let Some(summary) = self
            .summaries
            .iter_mut()
            .find(|summary| summary.board.id == board_id)
        {
            summary.board.name = name.to_owned();
        }
    }

    pub(crate) fn set_placement_count(&mut self, board_id: i64, placement_count: usize) {
        if let Some(summary) = self
            .summaries
            .iter_mut()
            .find(|summary| summary.board.id == board_id)
        {
            summary.placement_count = placement_count;
        }
    }
}
