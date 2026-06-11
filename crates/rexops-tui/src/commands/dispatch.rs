//! Confirmation-gated actions and their dispatch: the `PendingAction` type
//! plus the App's palette / confirm-gate state transitions.

use rexops_core::AppConfig;

use super::{Command, PaletteCommand};
use crate::app::App;
use crate::tools::{self, ForegroundRunner};

/// A mutating action armed behind the Enter/Esc confirm gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    LaunchTool { id: String, name: String },
    RunJob { id: String, name: String },
}

impl PendingAction {
    pub fn prompt(&self) -> String {
        match self {
            PendingAction::LaunchTool { name, .. } => format!("Launch {name}?"),
            PendingAction::RunJob { name, .. } => format!("Run {name} as a background job?"),
        }
    }

    pub fn preview(&self, config: &AppConfig) -> String {
        let id = match self {
            PendingAction::LaunchTool { id, .. } | PendingAction::RunJob { id, .. } => id,
        };
        match tools::resolve_command(id, config) {
            Some(command) => format!("Will run:  {command}"),
            None => "No launch command yet (nothing will run)".to_owned(),
        }
    }
}

impl App {
    pub fn palette_commands(&self) -> Vec<PaletteCommand> {
        super::palette::filter(&self.palette_query)
    }

    pub(crate) fn open_palette(&mut self) {
        if self.pending_action.is_some() {
            return;
        }
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    pub(crate) fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    pub(crate) fn palette_move(&mut self, down: bool) {
        let len = self.palette_commands().len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }
        self.palette_selected = if down {
            (self.palette_selected + 1) % len
        } else {
            (self.palette_selected + len - 1) % len
        };
    }

    pub(crate) fn palette_activate(&mut self, runner: &mut impl ForegroundRunner) -> bool {
        let filtered = self.palette_commands();
        let Some(chosen) = filtered.get(self.palette_selected).cloned() else {
            self.close_palette();
            return false;
        };
        self.close_palette();
        match chosen.command {
            Command::Action(action) => self.on_action(action, runner),
            Command::RunTool { id, name } => {
                self.arm_tool(id, name);
                false
            }
        }
    }

    pub(crate) fn arm_tool(&mut self, id: String, name: String) {
        self.pending_action = Some(if tools::is_streamable(&id) {
            PendingAction::RunJob {
                id,
                name: name.clone(),
            }
        } else {
            PendingAction::LaunchTool {
                id,
                name: name.clone(),
            }
        });
        self.log_event(format!("{name}: confirm (Enter) or cancel (Esc)"));
    }

    pub(crate) fn confirm_pending(&mut self, runner: &mut impl ForegroundRunner) {
        let Some(action) = self.pending_action.take() else {
            return;
        };
        match action {
            PendingAction::LaunchTool { id, name } => {
                let report = tools::launch_tool(&id, &name, &self.config, runner);
                self.log_event(report.message());
                if report.should_refresh() {
                    self.request_refresh();
                }
            }
            PendingAction::RunJob { id, name } => {
                self.start_job(&id, &name);
            }
        }
    }

    pub(crate) fn cancel_pending(&mut self) {
        if let Some(action) = self.pending_action.take() {
            let name = match action {
                PendingAction::LaunchTool { name, .. } | PendingAction::RunJob { name, .. } => name,
            };
            self.log_event(format!("{name}: cancelled (nothing ran)"));
        }
    }
}
