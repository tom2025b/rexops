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
use rexops_core::{format_unix_millis_utc, AdapterHealth, AdapterRegistry, OpsSnapshot};

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
    /// Open the RexOps cockpit TUI directly (the launcher/jobs interface).
    ///
    /// A bare `rexops` now opens Pulse — the calm suite status screen — by
    /// default. Use this subcommand to jump straight to the full cockpit
    /// instead; it is also where `rexops` falls back when Pulse isn't installed.
    Tui,

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
    // No subcommand → Pulse is the default experience: the calm, read-only suite
    // status screen. RexOps is no longer "just a launcher" — it greets the user
    // with the all-clear / needs-attention verdict first, and the full cockpit is
    // a step away (`rexops tui`, or from inside Pulse). The `--json` flag has no
    // meaning here and is ignored. Everything below is the one-shot inspection
    // path.
    //
    // Pulse is a separate suite binary that may not be installed; if it can't be
    // resolved we fall back to the cockpit so `rexops` is never unusable.
    let Some(command) = cli.command else {
        return match rexops_tui::run_pulse_default()? {
            rexops_tui::PulseOutcome::Ran { .. } => Ok(()),
            rexops_tui::PulseOutcome::NotFound => {
                eprintln!(
                    "rexops: pulse (the default status screen) was not found on PATH — opening the cockpit instead. Install pulse, or run `rexops tui` to skip this notice."
                );
                rexops_tui::run()
            }
        };
    };

    // The cockpit, reachable explicitly. Same entry point the default used to
    // call; it owns its own terminal lifecycle and config loading.
    if let Commands::Tui = command {
        return rexops_tui::run();
    }

    let config = load_config();
    match command {
        Commands::Tui => unreachable!("handled above before config load"),
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
    println!(
        "RexOps status — generated {}",
        format_unix_millis_utc(snap.generated_at_ms)
    );
    println!();

    println!("Adapters:");
    if snap.adapter_health.is_empty() {
        println!("  (none probed)");
    } else {
        // Sort by id so the list is stable run-to-run (adapter_health is a
        // HashMap, whose iteration order is otherwise nondeterministic).
        let mut adapters: Vec<_> = snap.adapter_health.iter().collect();
        adapters.sort_by_key(|(name, _)| name.as_str());
        for (name, h) in adapters {
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
            const DISK_SHOWN: usize = 3;
            for d in sys.disk.iter().take(DISK_SHOWN) {
                println!("    {d}");
            }
            if let Some(extra) = sys.disk.len().checked_sub(DISK_SHOWN).filter(|n| *n > 0) {
                println!("    … (+{extra} more)");
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
        const SCRIPTS_SHOWN: usize = 3;
        for s in sv.scripts.iter().take(SCRIPTS_SHOWN) {
            let flag = if sv.is_favorite(s) { " ★" } else { "" };
            let desc = s.description.as_deref().unwrap_or("");
            println!("  - {}{} {}", s.label(), flag, desc);
        }
        if let Some(extra) = sv
            .scripts
            .len()
            .checked_sub(SCRIPTS_SHOWN)
            .filter(|n| *n > 0)
        {
            println!("  - … (+{extra} more)");
        }
        println!();
    }

    if let Some(tf) = &snap.tools {
        println!("Tools (as of {}):", tf.as_of);
        println!(
            "  tools: {} ({} need attention)",
            tf.tool_count, tf.attention_count
        );
        const TOOLS_SHOWN: usize = 5;
        for t in tf.tools.iter().take(TOOLS_SHOWN) {
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
        if let Some(extra) = tf.tools.len().checked_sub(TOOLS_SHOWN).filter(|n| *n > 0) {
            println!("  … (+{extra} more)");
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
            const HIGH_RISK_SHOWN: usize = 5;
            let high_risk_total = bw.high_risk_items().count();
            for item in bw.high_risk_items().take(HIGH_RISK_SHOWN) {
                let sev = item.severity.as_deref().unwrap_or("?");
                println!("  ! {} ({})", item.label(), sev);
            }
            if let Some(extra) = high_risk_total
                .checked_sub(HIGH_RISK_SHOWN)
                .filter(|n| *n > 0)
            {
                println!("  ! … (+{extra} more)");
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

    // Notes used to re-print everything the structured panes above already
    // show (system facts, every tool, every finding) — a duplicated wall of
    // debug output. Show only notes that add something NOT already rendered:
    // adapter provenance/version, section freshness, config, and anything
    // unanticipated (e.g. a panic-recovery note). The duplicates are dropped by
    // prefix so a genuinely new note still surfaces by default.
    let extra: Vec<&String> = snap
        .notes
        .iter()
        .filter(|n| !is_duplicate_note(n))
        .collect();
    if !extra.is_empty() {
        println!("Notes:");
        for n in extra {
            println!("  - {n}");
        }
        println!();
    }

    println!("(Tip: try --json for the full machine-readable snapshot.)");
}

/// Whether a snapshot note merely repeats data the structured panes already
/// render (system facts, per-tool/-finding detail). Such notes are hidden from
/// the human `status` view to avoid printing everything twice; everything else
/// (provenance, freshness, config, unanticipated notes) is kept. Conservative by
/// design: only well-known duplicate prefixes are dropped, so a new kind of note
/// is surfaced rather than silently swallowed.
fn is_duplicate_note(note: &str) -> bool {
    let n = note.trim_start();
    const DUP_PREFIXES: &[&str] = &[
        "system hostname:",
        "system kernel:",
        "system uptime:",
        "system disk:",
        "tools:",     // counts + per-tool attention (shown in the Tools pane)
        "attention:", // indented per-tool attention lines
        "scripts:",   // counts (shown in the Scripts pane)
        "findings:",  // counts (shown in the Findings pane)
        "high-risk:", // indented per-finding lines (shown in the Findings pane)
    ];
    DUP_PREFIXES.iter().any(|p| n.starts_with(p))
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
