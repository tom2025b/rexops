//! main.rs — Entrypoint for the `rexops` binary.
//!
//! This is intentionally a *thin shell*:
//! - clap for argument parsing and --help / --version.
//! - With NO subcommand, launch the interactive TUI — the default experience
//!   (`cargo run` / `rexops`). The TUI runs behind rexops-tui's single public
//!   entry point (`rexops_tui::run`), so we drive the exact same code as the
//!   standalone `rexops-tui` binary.
//! - With a subcommand (`status`, `adapters`), call into rexops-app (shared
//!   config + snapshot builder) which in turn uses rexops-core +
//!   rexops-adapters, then format the result. Human output (default) or --json.
//! - All real work (health, risk, registries, the TUI itself) lives in the
//!   libraries; this binary is only dispatch + formatting.
//!
//! Quality: we still follow the project rules at the binary boundary —
//! good error messages, no silent failures, and the four cargo gates apply.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

// CLI is now a pure thin shell. All config loading and snapshot/registry
// building has moved to rexops-app (the shared layer). We only import the
// types we need for clap dispatch and pretty-printing.
use rexops_app::{build_adapter_registry, build_snapshot, load_config};
use rexops_core::{AdapterHealth, AdapterRegistry, OpsSnapshot};

/// rexops — the RexOps ops cockpit.
///
/// Run with no subcommand to open the interactive TUI (the default). The
/// `status` and `adapters` subcommands provide one-shot inspection over the
/// adapter layer and core models. All heavy lifting is delegated; this binary
/// is only glue + formatting.
#[derive(Parser, Debug)]
#[command(name = "rexops", version, about, long_about = None)]
struct Cli {
    /// Emit machine-readable JSON instead of human text.
    #[arg(long, global = true)]
    json: bool,

    /// Optional subcommand. When omitted, the interactive TUI launches.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show overall status: adapter health, risk summary, snapshot timestamp.
    Status,

    /// List known/registered adapters and their health.
    Adapters,
}

/// Top level run. Returns a proper exit code so scripts can react.
fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rexops: error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The actual logic, separated so main() can do the exit-code dance cleanly.
fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    // No subcommand → the interactive TUI is the default experience. It loads
    // its own config and owns the terminal lifecycle, so we hand off directly
    // and return its Result (the `--json` flag has no meaning here and is
    // ignored). Everything below is the one-shot inspection path.
    let Some(command) = cli.command else {
        return rexops_tui::run();
    };

    let config = load_config();
    match command {
        Commands::Status => {
            let snapshot = build_snapshot(&config);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                print_status_human(&snapshot);
            }
        }
        Commands::Adapters => {
            let reg = build_adapter_registry(&config);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&reg)?);
            } else {
                print_adapters_human(&reg);
            }
        }
    }
    Ok(())
}

fn print_status_human(snap: &OpsSnapshot) {
    println!("RexOps status — generated at {} ms", snap.generated_at_ms);
    println!();

    println!("Adapters:");
    if snap.adapter_health.is_empty() {
        println!("  (none probed)");
    } else {
        for (name, h) in &snap.adapter_health {
            let mark = match h {
                AdapterHealth::Healthy => "✓",
                AdapterHealth::Degraded => "!",
                AdapterHealth::Unavailable => "✗",
                AdapterHealth::Unknown => "?",
            };
            println!("  {mark} {name}: {h:?}");
        }
    }
    println!();

    if let Some(sys) = &snap.system {
        println!("System:");
        if let Some(h) = &sys.hostname {
            println!("  hostname: {h}");
        }
        if let Some(k) = &sys.kernel {
            println!("  kernel: {k}");
        }
        if let Some(u) = &sys.uptime {
            println!("  uptime: {u}");
        }
        if !sys.disk.is_empty() {
            println!("  disk:");
            for d in sys.disk.iter().take(3) {
                println!("    {d}");
            }
        }
        println!();
    }

    if let Some(sv) = &snap.scripts {
        println!("Scripts (as of {}):", sv.generated_at);
        println!(
            "  scripts: {} ({} favorites, {} recents)",
            sv.total(),
            sv.favorites_count(),
            sv.recents_count()
        );
        for s in sv.scripts.iter().take(3) {
            let flag = if sv.is_favorite(s) { " ★" } else { "" };
            let desc = s.description.as_deref().unwrap_or("");
            println!("  - {}{} {}", s.label(), flag, desc);
        }
        println!();
    }

    if let Some(tf) = &snap.tools {
        println!("Tools (as of {}):", tf.as_of);
        println!(
            "  tools: {} ({} need attention)",
            tf.tool_count, tf.attention_count
        );
        for t in tf.tools.iter().take(5) {
            let mark = if t.needs_attention() { "!" } else { "✓" };
            let review = if t.review_due_flag {
                match t.review_after.as_deref() {
                    Some(date) => format!("; review due since {date}"),
                    None => "; review due".to_string(),
                }
            } else {
                String::new()
            };
            println!(
                "  {} {} — {} ({}{})",
                mark, t.display_name, t.status, t.lifecycle_state, review
            );
        }
        println!();
    }

    if let Some(bw) = &snap.findings {
        let t = bw.risk_tally();
        println!("Findings (as of {}):", bw.generated_at);
        if t.has_risk_data() {
            println!(
                "  {} items — critical={} high={} medium={} low={} info={}",
                bw.items.len(),
                t.critical,
                t.high,
                t.medium,
                t.low,
                t.info
            );
            for item in bw.high_risk_items().take(5) {
                let sev = item.severity.as_deref().unwrap_or("?");
                println!("  ! {} ({})", item.label(), sev);
            }
        } else {
            println!("  {} items — risk breakdown unavailable", bw.items.len());
        }
        println!();
    }

    if let Some(ws) = &snap.workstate {
        println!("Workstate (v3 snapshot, built {}):", ws.built_at);
        println!(
            "  sections populated: {}/3 (scripts/tools/findings)",
            ws.populated_section_count()
        );
        println!();
    }

    println!(
        "Risk: total_findings={} should_block={}",
        snap.risk.total_findings, snap.risk.should_block
    );
    println!();

    if !snap.notes.is_empty() {
        println!("Notes:");
        for n in &snap.notes {
            println!("  - {n}");
        }
    }

    println!();
    println!("(Tip: try --json for machine output.)");
}

fn print_adapters_human(reg: &AdapterRegistry) {
    println!(
        "Registered adapters ({} total, {} available):",
        reg.len(),
        reg.available_count()
    );
    for e in reg.list() {
        println!(
            "  {} — health: {:?}  label: {}",
            e.id,
            e.health,
            e.label.as_deref().unwrap_or("-")
        );
    }
    if reg.is_empty() {
        println!("  (no adapters registered)");
    }
}
