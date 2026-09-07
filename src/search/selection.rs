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

#[derive(Clone, Debug, Default)]
pub(crate) struct SearchSelection {
    selected: Vec<usize>,
    lead: Option<usize>,
    anchor: Option<usize>,
}

impl SearchSelection {
    pub(super) fn update(&mut self, index: usize, mode: SearchSelectionMode) {
        match mode {
            SearchSelectionMode::Replace => self.replace(index),
            SearchSelectionMode::Toggle => self.toggle(index),
            SearchSelectionMode::Range => self.range_to(index),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.lead = None;
        self.anchor = None;
    }

    pub(crate) fn selected(&self) -> &[usize] {
        &self.selected
    }

    pub(crate) fn lead(&self) -> Option<usize> {
        self.lead
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub(crate) fn replace(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
        self.lead = Some(index);
        self.anchor = Some(index);
    }

    pub(crate) fn toggle(&mut self, index: usize) {
        if let Some(position) = self.selected.iter().position(|selected| *selected == index) {
            self.selected.remove(position);
        } else {
            self.selected.push(index);
        }

        self.selected.sort_unstable();
        if self.selected.is_empty() {
            self.selected.push(index);
            self.lead = Some(index);
            self.anchor = Some(index);
        } else if self.selected.contains(&index) {
            self.lead = Some(index);
            self.anchor = Some(index);
        } else {
            let fallback = self.selected[0];
            self.lead = Some(fallback);
            self.anchor = Some(fallback);
        }
    }

    pub(crate) fn range_to(&mut self, index: usize) {
        let anchor = self
            .anchor
            .or_else(|| self.selected.first().copied())
            .unwrap_or(index);
        let start = anchor.min(index);
        let end = anchor.max(index);
        self.selected = (start..=end).collect();
        self.lead = Some(index);
    }

    pub(crate) fn select_initial(&mut self, result_count: usize) {
        if result_count == 0 {
            self.clear();
            return;
        }

        let index = self.lead.unwrap_or(0).min(result_count - 1);
        self.replace(index);
    }
}
