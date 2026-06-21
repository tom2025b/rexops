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
    /// The per-component drill-down reached from a focused cockpit card.
    CockpitDetail,
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

    // --- cockpit card focus (keyed by component id) ---

    /// After a snapshot change, keep the focused id if it is still visible; else
    /// fall back to the first visited card (or `None`).
    pub(crate) fn keep_cockpit_selection_visible(&mut self) {
        let visible: Vec<String> = crate::screens::cockpit_visit_order(&self.snapshot.components)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        match &self.selected_component {
            Some(id) if visible.iter().any(|v| v == id) => {}
            _ => self.selected_component = visible.into_iter().next(),
        }
    }

    /// Step cockpit focus along the visit order with wraparound.
    pub(crate) fn move_cockpit_selection(&mut self, down: bool) {
        let order: Vec<String> = crate::screens::cockpit_visit_order(&self.snapshot.components)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        if order.is_empty() {
            return;
        }
        let next = match &self.selected_component {
            Some(cur) => order.iter().position(|id| id == cur).map(|pos| {
                let n = if down {
                    (pos + 1) % order.len()
                } else if pos > 0 {
                    pos - 1
                } else {
                    order.len() - 1
                };
                order[n].clone()
            }),
            None => order.first().cloned(),
        };
        if let Some(id) = next {
            self.selected_component = Some(id);
        }
    }

    /// Arm the focused cockpit card through the shared confirm gate. A `None`
    /// selection is a no-op. Gating (launchable / available) is `arm_tool`'s job.
    pub(crate) fn arm_selected_component(&mut self) {
        let Some(id) = self.selected_component.clone() else {
            return;
        };
        let name = self
            .snapshot
            .components
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone());
        if let Some(name) = name {
            self.arm_tool(id, name);
        }
    }

    /// Resolve a pressed marker letter to a card, focus it, and arm it.
    pub(crate) fn arm_component_by_marker(&mut self, key: char) {
        let id = crate::screens::component_for_marker(&self.snapshot.components, key)
            .map(|s| s.to_owned());
        if let Some(id) = id {
            self.selected_component = Some(id);
            self.arm_selected_component();
        }
    }

    /// Drill into the focused cockpit card's detail. No-op (logged) if nothing
    /// is focused.
    pub(crate) fn drill_into_selected_component(&mut self) {
        if self.selected_component.is_some() {
            self.current_screen = Screen::CockpitDetail;
            self.log_event("Cockpit: opened component detail (Esc to go back)");
        } else {
            self.log_event("Cockpit: no card focused to open");
        }
    }

    /// Back out of the detail screen to the cockpit, keeping the focus.
    pub(crate) fn cockpit_back(&mut self) {
        self.current_screen = Screen::Dashboard;
        self.log_event("Detail: back to cockpit");
    }
}
