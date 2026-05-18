use super::controller::SearchController;

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingResultSelection {
    pub(super) index: usize,
    pub(super) mode: SearchSelectionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchSelectionMode {
    Replace,
    Toggle,
    Range,
}

impl SearchController {
    pub(super) fn update_result_selection(&mut self, index: usize, mode: SearchSelectionMode) {
        match mode {
            SearchSelectionMode::Replace => {
                self.overlay.result_selection.replace(index);
            }
            SearchSelectionMode::Toggle => {
                self.overlay.result_selection.toggle(index);
            }
            SearchSelectionMode::Range => {
                self.overlay.result_selection.range_to(index);
            }
        }
    }

    pub(super) fn select_initial_result(&mut self) {
        self.overlay
            .result_selection
            .select_initial(self.overlay.results.len());
    }
}
