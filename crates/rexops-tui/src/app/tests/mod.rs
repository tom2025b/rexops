use super::*;
use std::sync::mpsc;

use rexops_core::{AppConfig, OpsSnapshot};

use crate::commands::PendingAction;
use crate::input::Action;
use crate::jobs::{
    spawn, toast_for, JobOutput, JobRecord, LastOutcome, JOB_HISTORY_CAP, JOB_OUTPUT_CAP,
};
use crate::tools::{ChildExit, ForegroundRunner, LaunchCommand, CATALOG};

struct FakeRunner {
    calls: usize,
}

impl ForegroundRunner for FakeRunner {
    fn run_foreground(&mut self, _command: &LaunchCommand) -> std::io::Result<ChildExit> {
        self.calls += 1;
        Ok(ChildExit::Success)
    }
}

/// Build an App already on the Launcher screen for navigation tests.
fn launcher_app() -> App {
    let (tx, _rx) = mpsc::channel();
    let mut app = App::new(tx, AppConfig::default(), None);
    app.current_screen = Screen::Launcher;
    app
}

/// A bare App (no job, fresh state) for status-mapping tests.
fn bare_app() -> App {
    let (tx, _rx) = mpsc::channel();
    App::new(tx, AppConfig::default(), None)
}

/// An App whose snapshot carries the given adapter names, on the Dashboard
/// screen, for the live-filter tests. Names are applied via `apply_snapshot`
/// so `adapter_names` is derived exactly as it is in production.
fn dashboard_app_with_adapters(names: &[&str]) -> App {
    let mut app = bare_app();
    let mut snap = OpsSnapshot::new();
    for name in names {
        snap.adapter_health
            .insert((*name).to_owned(), rexops_core::AdapterHealth::Healthy);
    }
    app.apply_snapshot(snap);
    app.current_screen = Screen::Dashboard;
    app
}

/// Build a snapshot carrying the given adapter names (all Healthy), the way the
/// production refresh path delivers one.
fn snapshot_with_adapters(names: &[&str]) -> OpsSnapshot {
    let mut snap = OpsSnapshot::new();
    for name in names {
        snap.adapter_health
            .insert((*name).to_owned(), rexops_core::AdapterHealth::Healthy);
    }
    snap
}

mod esc;
mod filters;
mod help;
mod jobs;
mod launcher;
mod palette;
mod refresh;
