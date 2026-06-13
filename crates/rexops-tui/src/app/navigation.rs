//! Screen selection and list navigation: the `Screen` enum plus the
//! adapter filter / selection-movement helpers that drive it.

use super::App;

/// Top-level screen selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Dashboard,
    Adapters,
    System,
    Scripts,
    Tools,
    Launcher,
    Jobs,
}

impl App {
    pub(crate) fn filter_screen(&self) -> bool {
        matches!(self.current_screen, Screen::Adapters | Screen::Dashboard)
    }

    /// The adapter names the live filter currently keeps, as a borrowed slice.
    ///
    /// This is read on the render hot path (Dashboard + Adapters, ~10×/s), so it
    /// returns a cached `&[String]` rather than allocating a fresh `Vec` each
    /// call. The cache is rebuilt by [`Self::recompute_filtered_names`] whenever
    /// its inputs change — `adapter_names` (in `apply_snapshot`) or `filter`
    /// (every mutation is immediately followed by `select_first_visible_adapter`
    /// or `keep_selected_adapter_visible`, which rebuild it).
    pub fn filtered_adapter_names(&self) -> &[String] {
        &self.filtered_names
    }

    /// Rebuild the `filtered_names` cache from `adapter_names` + `filter`. Cheap
    /// and called only on input changes (snapshot / filter edit), never per
    /// frame.
    pub(crate) fn recompute_filtered_names(&mut self) {
        if self.filter.is_empty() {
            self.filtered_names.clone_from(&self.adapter_names);
        } else {
            let filter = self.filter.to_lowercase();
            self.filtered_names = self
                .adapter_names
                .iter()
                .filter(|name| name.to_lowercase().contains(&filter))
                .cloned()
                .collect();
        }
    }

    pub(crate) fn select_first_visible_adapter(&mut self) {
        self.recompute_filtered_names();
        self.selected_adapter = self.filtered_names.first().cloned();
    }

    pub(crate) fn keep_selected_adapter_visible(&mut self) {
        self.recompute_filtered_names();
        if let Some(selected) = &self.selected_adapter {
            if !self.filtered_names.contains(selected) {
                self.selected_adapter = self.filtered_names.first().cloned();
            }
        } else {
            self.selected_adapter = self.filtered_names.first().cloned();
        }
    }

    pub(crate) fn move_adapter_selection(&mut self, down: bool) {
        let visible = &self.filtered_names;
        if visible.is_empty() {
            return;
        }
        // Resolve the next name before touching `selected_adapter`, so the
        // immutable borrow of the cache ends before the mutable write.
        let next_name = self.selected_adapter.as_ref().and_then(|current| {
            visible.iter().position(|name| name == current).map(|pos| {
                let next = if down {
                    (pos + 1) % visible.len()
                } else if pos > 0 {
                    pos - 1
                } else {
                    visible.len() - 1
                };
                visible[next].clone()
            })
        });
        if let Some(name) = next_name {
            self.selected_adapter = Some(name);
        }
    }
}
