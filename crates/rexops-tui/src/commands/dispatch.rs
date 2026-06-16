//! Confirmation-gated actions and their dispatch: the `PendingAction` type
//! plus the App's palette / confirm-gate state transitions.

use rexops_core::AppConfig;

use super::{Command, PaletteCommand};
use crate::app::App;
use crate::tools::{self, ForegroundRunner};

/// A mutating action armed behind the Enter/y or n/Esc confirm gate.
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
        match tools::resolve_launch_command(id, config) {
            Some(command) => format!("Will run:  {}", command.display()),
            None => "No launch command yet (nothing will run)".to_owned(),
        }
    }
}

impl App {
    pub fn palette_commands(&self) -> Vec<PaletteCommand> {
        let mut cmds = super::palette::filter(&self.palette_query);
        // Annotate each `run <tool>` row with its live availability, mirroring
        // the Launcher screen's 3-state tag so the two run surfaces never
        // disagree. The palette command set itself stays pure (no App); the tag
        // is folded in here, where the App's availability is in hand. Tools stay
        // listed even when down — a disabled/unavailable tool reads as such
        // before you pick it, instead of silently no-op'ing on Enter.
        for cmd in &mut cmds {
            if let Command::RunTool { id, .. } = &cmd.command {
                cmd.desc = format!("{} · {}", cmd.desc, self.availability_tag(id));
            }
        }
        cmds
    }

    pub(crate) fn open_palette(&mut self) {
        if self.pending_action.is_some() {
            return;
        }
        // The palette is its own text-input context; end any inline filter
        // capture so closing the palette doesn't silently resume filtering.
        self.filtering = false;
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
        // Count only — movement needs the length, not the annotated rows, so
        // skip rebuilding/availability-tagging the whole Vec here.
        let len = super::palette::filter(&self.palette_query).len();
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
        if tools::resolve_launch_command(&id, self.config()).is_none() {
            self.log_event(format!("{name}: disabled (no launch command)"));
            return;
        }
        // Health-aware gate: even when the command resolves, refuse to open the
        // confirm gate for a tool whose adapter probe reports it Unavailable —
        // matching the launcher's "· unavailable" tag so the UI never invites a
        // launch it just flagged as down. (Unknown/Degraded still arm: see
        // App::is_tool_available.)
        if !self.is_tool_available(&id) {
            self.log_event(format!("{name}: unavailable (adapter reports it is down)"));
            return;
        }
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
        self.log_event(format!("{name}: confirm (Enter/y) or cancel (n/Esc)"));
    }

    pub(crate) fn confirm_pending(&mut self, runner: &mut impl ForegroundRunner) {
        let Some(action) = self.pending_action.take() else {
            return;
        };
        match action {
            PendingAction::LaunchTool { id, name } => {
                let report = tools::launch_tool(&id, &name, self.config(), runner);
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
