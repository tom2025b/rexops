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

    pub fn filtered_adapter_names(&self) -> Vec<String> {
        if self.filter.is_empty() {
            self.adapter_names.clone()
        } else {
            let filter = self.filter.to_lowercase();
            self.adapter_names
                .iter()
                .filter(|name| name.to_lowercase().contains(&filter))
                .cloned()
                .collect()
        }
    }

    pub(crate) fn select_first_visible_adapter(&mut self) {
        self.selected_adapter = self.filtered_adapter_names().first().cloned();
    }

    pub(crate) fn keep_selected_adapter_visible(&mut self) {
        let visible = self.filtered_adapter_names();
        if let Some(selected) = &self.selected_adapter {
            if !visible.contains(selected) {
                self.selected_adapter = visible.first().cloned();
            }
        } else {
            self.selected_adapter = visible.first().cloned();
        }
    }

    pub(crate) fn move_adapter_selection(&mut self, down: bool) {
        let visible = self.filtered_adapter_names();
        if visible.is_empty() {
            return;
        }
        if let Some(current) = &self.selected_adapter {
            if let Some(pos) = visible.iter().position(|name| name == current) {
                let next = if down {
                    (pos + 1) % visible.len()
                } else if pos > 0 {
                    pos - 1
                } else {
                    visible.len() - 1
                };
                self.selected_adapter = Some(visible[next].clone());
            }
        }
    }
}
