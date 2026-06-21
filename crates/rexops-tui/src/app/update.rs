//! High-level action handling.

use super::{App, Screen};
use crate::input::Action;
use crate::tools::{self, ForegroundRunner};

impl App {
    pub fn on_action(&mut self, action: Action, launcher: &mut impl ForegroundRunner) -> bool {
        self.clear_toast();

        // The help sheet is a true overlay: it renders over everything, so it
        // must also CAPTURE input. While it's up, any key dismisses it and is
        // otherwise swallowed — nothing reaches (or mutates) the screen behind
        // it. This is the outermost gate, above the pending/palette modals,
        // because the sheet renders on top of those too.
        if self.show_help {
            self.show_help = false;
            return false;
        }

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
            Action::CardKey(c) => {
                // Cockpit-only: a card's letter arms it. On any other screen it's
                // inert (no card grid). Navigation mode only — while filtering the
                // keymap emits InputChar, so letters type into the filter instead.
                if self.current_screen == Screen::Dashboard {
                    self.arm_component_by_marker(c);
                }
                false
            }
            Action::Drill => {
                if self.current_screen == Screen::Dashboard {
                    self.drill_into_selected_component();
                }
                false
            }
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
            // The cockpit moves its own card focus (keyed by component id).
            Screen::Dashboard => self.move_cockpit_selection(down),
            // Adapters keeps the filtered adapter table + its shared selection.
            Screen::Adapters => self.move_adapter_selection(down),
            // On the Jobs screen, Up/Down scroll the output viewport instead of
            // moving a list selection. Up = toward older output.
            Screen::Jobs => self.scroll_jobs_output(!down),
            Screen::Launcher => {
                let len = tools::CATALOG.len();
                if len > 0 {
                    // Clamp first so a stale index (defensive: the field is a raw
                    // usize independent of the catalog) can't make `% len` wrap
                    // from a bogus base; then step with wraparound.
                    let cur = self.selected_tool.min(len - 1);
                    self.selected_tool = if down {
                        (cur + 1) % len
                    } else {
                        (cur + len - 1) % len
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
            // On the cockpit, Enter arms the focused card if it is launchable;
            // otherwise it drills into the card's detail (so Enter is never a
            // silent no-op on a planned/read-only card). `arm_tool` itself gates
            // launchability, so we check `launchable` here only to choose between
            // arm vs. drill.
            Screen::Dashboard => {
                let launchable = self
                    .selected_component
                    .as_deref()
                    .and_then(|id| self.snapshot.components.iter().find(|c| c.id == id))
                    .map(|c| c.launchable)
                    .unwrap_or(false);
                if launchable {
                    self.arm_selected_component();
                } else {
                    self.drill_into_selected_component();
                }
            }
            // On the detail screen, Enter launches the focused component if it is
            // launchable (same arm path as the cockpit). A read-only component's
            // Enter is a no-op here — there's nothing deeper to drill into.
            Screen::CockpitDetail => {
                self.arm_selected_component();
            }
            _ => {}
        }
    }

    /// Handle Esc. It "backs out one level" through the active context — filter
    /// capture, then an applied filter, then the Launcher (→ Dashboard). At the
    /// top level (no context left to back out of) Esc is a deliberate NO-OP, not
    /// a quit. Quit is `q` / Ctrl-C only.
    ///
    /// The old fallback returned quit, so Esc from the Dashboard exited the whole
    /// app — and because quitting kills a running job without a confirm, Esc on
    /// the Jobs screen mid-job meant "kill the job AND drop the app" in one
    /// keystroke. Esc is a back/cancel reflex, not an exit key; making the
    /// top-level case a no-op removes that footgun while leaving every nested
    /// "back out" behaviour intact. Always returns `false` (never quits).
    fn cancel_current_context(&mut self) -> bool {
        // Esc while filtering: abandon the filter — exit the mode AND clear the
        // query, returning the list to its full state.
        if self.filtering {
            self.filtering = false;
            self.filter.clear();
            self.select_first_visible_adapter();
            self.log_event("Filter cleared");
        } else if self.filter_screen() && !self.filter.is_empty() {
            // Not filtering, but a filter was applied (Enter then Esc in nav):
            // Esc clears the applied filter.
            self.filter.clear();
            self.select_first_visible_adapter();
        } else if self.current_screen == Screen::Launcher {
            self.current_screen = Screen::Dashboard;
            self.log_event("Launcher: back to Dashboard");
        } else if self.current_screen == Screen::CockpitDetail {
            self.cockpit_back();
        } else {
            // Top level: nothing to back out of. Esc does NOT quit — that is `q`
            // / Ctrl-C. A no-op here is what keeps Esc from killing a running job
            // and exiting the app in a single keystroke on the Jobs screen.
            self.log_event("Esc: nothing to cancel here (press q to quit)");
        }
        false
    }
}
