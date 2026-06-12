//! High-level action handling.

use super::{App, Screen};
use crate::input::Action;
use crate::tools::{self, ForegroundRunner};

impl App {
    pub fn on_action(&mut self, action: Action, launcher: &mut impl ForegroundRunner) -> bool {
        self.clear_toast();

        if self.pending_action.is_some() {
            match action {
                Action::Activate => self.confirm_pending(launcher),
                Action::Cancel => self.cancel_pending(),
                Action::InputChar('y' | 'Y') => self.confirm_pending(launcher),
                Action::InputChar('n' | 'N') => self.cancel_pending(),
                _ => {}
            }
            return false;
        }

        if self.palette_open {
            match action {
                Action::Cancel => self.close_palette(),
                Action::Activate => return self.palette_activate(launcher),
                Action::Up => self.palette_move(false),
                Action::Down => self.palette_move(true),
                Action::Backspace => {
                    self.palette_query.pop();
                    self.palette_selected = 0;
                }
                Action::InputChar(c) if c.is_ascii_graphic() || c == ' ' => {
                    self.palette_query.push(c);
                    self.palette_selected = 0;
                }
                _ => {}
            }
            return false;
        }

        match action {
            Action::Quit => true,
            Action::Refresh => {
                self.request_refresh();
                false
            }
            Action::ToggleHelp => {
                self.toggle_help();
                false
            }
            Action::SwitchToDashboard => self.switch_to(Screen::Dashboard, "Dashboard"),
            Action::SwitchToAdapters => self.switch_to(Screen::Adapters, "Adapters"),
            Action::SwitchToSystem => self.switch_to(Screen::System, "System"),
            Action::SwitchToScripts => self.switch_to(Screen::Scripts, "Scripts"),
            Action::SwitchToTools => self.switch_to(Screen::Tools, "Tools"),
            Action::SwitchToLauncher => self.switch_to(Screen::Launcher, "Launcher"),
            Action::SwitchToJobs => self.switch_to(Screen::Jobs, "Jobs"),
            Action::OpenPalette => {
                self.open_palette();
                false
            }
            Action::CancelJob => {
                self.cancel_job();
                false
            }
            Action::Up => {
                self.move_selection(false);
                false
            }
            Action::Down => {
                self.move_selection(true);
                false
            }
            Action::Activate => {
                self.activate_selection();
                false
            }
            Action::Cancel => self.cancel_current_context(),
            Action::InputChar(c) => {
                if self.filter_screen() && c.is_ascii_graphic() {
                    self.filter.push(c);
                    self.select_first_visible_adapter();
                }
                false
            }
            Action::Backspace => {
                if self.filter_screen() && !self.filter.is_empty() {
                    self.filter.pop();
                    self.keep_selected_adapter_visible();
                }
                false
            }
        }
    }

    fn switch_to(&mut self, screen: Screen, label: &str) -> bool {
        self.current_screen = screen;
        self.log_event(format!("Switched to {label} screen"));
        false
    }

    fn move_selection(&mut self, down: bool) {
        match self.current_screen {
            Screen::Adapters => self.move_adapter_selection(down),
            Screen::Launcher => {
                let len = tools::CATALOG.len();
                if len > 0 {
                    self.selected_tool = if down {
                        (self.selected_tool + 1) % len
                    } else {
                        (self.selected_tool + len - 1) % len
                    };
                }
            }
            _ => {}
        }
    }

    fn activate_selection(&mut self) {
        match self.current_screen {
            Screen::Adapters => {
                if let Some(name) = &self.selected_adapter {
                    self.snapshot.add_note(format!(
                        "selected adapter detail: {name} (press r to refresh for live)"
                    ));
                }
            }
            Screen::Launcher => {
                if let Some(tool) = tools::CATALOG.get(self.selected_tool) {
                    self.arm_tool(tool.id.to_owned(), tool.name.to_owned());
                }
            }
            _ => {}
        }
    }

    fn cancel_current_context(&mut self) -> bool {
        if self.filter_screen() && !self.filter.is_empty() {
            self.filter.clear();
            self.select_first_visible_adapter();
            false
        } else if self.current_screen == Screen::Launcher {
            self.current_screen = Screen::Dashboard;
            self.log_event("Launcher: back to Dashboard");
            false
        } else {
            true
        }
    }
}
