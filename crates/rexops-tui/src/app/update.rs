//! High-level action handling.

use super::{App, Modal, Screen};
use crate::input::Action;
use crate::tools::{self, ForegroundRunner};

impl App {
    pub fn on_action(&mut self, action: Action, launcher: &mut impl ForegroundRunner) -> bool {
        self.clear_toast();

        // Modal input is gated by the SAME precedence the render path layers by
        // (App::active_modal) — the single source of truth — so the overlay
        // drawn on top is always the one capturing keys. A modal swallows input;
        // only `Modal::None` falls through to the screen bindings below.
        match self.active_modal() {
            Modal::Help => {
                // The help sheet is a true overlay: any key dismisses it and is
                // otherwise swallowed — nothing reaches the screen behind it.
                self.show_help = false;
                return false;
            }
            Modal::Confirm => {
                match action {
                    Action::Activate => self.confirm_pending(launcher),
                    Action::Cancel => self.cancel_pending(),
                    Action::InputChar('y' | 'Y') => self.confirm_pending(launcher),
                    Action::InputChar('n' | 'N') => self.cancel_pending(),
                    _ => {}
                }
                return false;
            }
            Modal::Palette => {
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
            Modal::None => {}
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
                // While filtering, Enter confirms the filter and returns to nav
                // (the narrowed list stays applied) rather than activating a row.
                if self.filtering {
                    self.filtering = false;
                    self.log_event("Filter applied");
                } else {
                    self.activate_selection();
                }
                false
            }
            Action::Cancel => self.cancel_current_context(),
            // `/` on a filter screen enters filter mode. It is intercepted here
            // and NOT appended to the query — the slash is the trigger, not text.
            // (While already filtering we run in Text mode, so `/` arrives as a
            // normal InputChar below and types literally, as you'd expect.)
            Action::InputChar('/') if self.filter_screen() && !self.filtering => {
                self.filtering = true;
                self.log_event("Filter: type to narrow, Enter to keep, Esc to clear");
                false
            }
            Action::InputChar(c) => {
                // Only capture into the filter while actively filtering. In Text
                // mode every printable char reaches here (no command stole it),
                // so bound letters like q/r/digits now type into the filter too.
                if self.filtering && self.filter_screen() && c.is_ascii_graphic() {
                    self.filter.push(c);
                    self.select_first_visible_adapter();
                }
                false
            }
            Action::Backspace => {
                if self.filtering && self.filter_screen() && !self.filter.is_empty() {
                    self.filter.pop();
                    self.keep_selected_adapter_visible();
                }
                false
            }
        }
    }

    fn switch_to(&mut self, screen: Screen, label: &str) -> bool {
        // Leaving a screen ends any active filter capture — `filtering` is only
        // valid on the screen it was started on, and a stale flag would keep the
        // keymap in Text mode where it doesn't belong.
        self.filtering = false;
        self.current_screen = screen;
        self.log_event(format!("Switched to {label} screen"));
        false
    }

    fn move_selection(&mut self, down: bool) {
        match self.current_screen {
            // Dashboard and Adapters show the same filtered adapter table and
            // share its selection, so j/k move it identically on both.
            Screen::Dashboard | Screen::Adapters => self.move_adapter_selection(down),
            // On the Jobs screen, Up/Down scroll the output viewport instead of
            // moving a list selection. Up = toward older output.
            Screen::Jobs => self.scroll_jobs_output(!down),
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
        // Esc while filtering: abandon the filter — exit the mode AND clear the
        // query, returning the list to its full state.
        if self.filtering {
            self.filtering = false;
            self.filter.clear();
            self.select_first_visible_adapter();
            self.log_event("Filter cleared");
            false
        } else if self.filter_screen() && !self.filter.is_empty() {
            // Not filtering, but a filter was applied (Enter then Esc in nav):
            // Esc clears the applied filter.
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
