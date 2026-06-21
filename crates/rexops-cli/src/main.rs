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

mod launch;

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
    /// Show overall status: adapter health, risk summary, snapshot timestamp.
    Status,

    /// List known/registered adapters and their health.
    Adapters,

    /// List the suite component registry (id, group, maturity, health, vital).
    Components,

    /// Launch a component's tool, with a confirm prompt (mirrors the cockpit).
    Launch {
        /// Component id to launch (e.g. bulwark, proto, scriptvault, pulse).
        tool: String,

        /// Skip the confirmation prompt (for scripts / non-interactive use).
        #[arg(long, short = 'y')]
        yes: bool,

        /// Print the exact command that would run, then exit without running it.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Top level run. Returns a proper exit code so scripts can react.
fn main() -> ExitCode {
    let cli = Cli::parse();

    // `launch` forwards the child's exit code (and uses non-zero to refuse), so
    // it bypasses the Ok/Err inspection flow and returns its own ExitCode.
    if let Some(Commands::Launch { tool, yes, dry_run }) = &cli.command {
        let code = launch::run_launch(tool, *yes, *dry_run, &load_config());
        return ExitCode::from(code);
    }

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
        Commands::Components => {
            let snapshot = build_snapshot(&config);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&snapshot.components)?);
            } else {
                print!("{}", render_components_human(&snapshot));
            }
        }
        // `launch` is intercepted in `main` (it owns its exit code), so it never
        // reaches here. Kept as an explicit arm so the match stays exhaustive.
        Commands::Launch { .. } => unreachable!("launch is handled in main()"),
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

/// Render the human component roster to a String (separated from printing so it
/// can be unit-tested without capturing stdout).
fn render_components_human(snap: &OpsSnapshot) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Components ({} in the suite map):",
        snap.components.len()
    );
    if snap.components.is_empty() {
        let _ = writeln!(out, "  (none — run a refresh)");
        return out;
    }
    for c in &snap.components {
        let mark = match c.health {
            AdapterHealth::Healthy => "✓",
            AdapterHealth::Degraded => "!",
            AdapterHealth::Unavailable => "✗",
            AdapterHealth::Unknown => "·",
        };
        let vital = c.vital.as_deref().unwrap_or("-");
        let _ = writeln!(
            out,
            "  {mark} {:<22} {:<11} {:<10} {}",
            c.name, c.group, c.maturity, vital
        );
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use rexops_core::{AdapterHealth, ComponentStatus, OpsSnapshot};

    use super::render_components_human;

    fn snap_with_one() -> OpsSnapshot {
        let mut s = OpsSnapshot::new();
        s.push_component(ComponentStatus {
            id: "pulse".to_owned(),
            name: "Pulse".to_owned(),
            group: "monitor".to_owned(),
            maturity: "planned".to_owned(),
            health: AdapterHealth::Unknown,
            freshness: None,
            vital: None,
            launchable: false,
        });
        s
    }

    #[test]
    fn components_human_lists_the_row_with_its_maturity() {
        let out = render_components_human(&snap_with_one());
        assert!(out.contains("Pulse"), "names the component:\n{out}");
        assert!(out.contains("planned"), "shows maturity:\n{out}");
        assert!(out.contains("monitor"), "shows the group:\n{out}");
    }
}
