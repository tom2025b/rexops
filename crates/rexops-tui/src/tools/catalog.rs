//! Static launcher catalog and per-tool execution-mode metadata.

/// How a tool runs when launched from the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Hands over the real terminal (interactive tools).
    Foreground,
    /// Streams output into the Jobs screen.
    Background,
}

/// The product area a tool belongs to. Drives Launcher grouping only — it has
/// no effect on resolution, launch, or the palette (which stays a flat list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Scripts,
    System,
    Inventory,
}

impl Category {
    /// Section-header text shown above the group on the Launcher screen.
    pub fn title(self) -> &'static str {
        match self {
            Category::Scripts => "Scripts",
            Category::System => "System",
            Category::Inventory => "Inventory",
        }
    }

    /// Stable render order for the Launcher's grouped list. The launcher walks
    /// this so headers always appear in the same order regardless of catalog
    /// insertion order; an empty category is simply skipped.
    pub const ORDER: [Category; 3] = [Category::Scripts, Category::System, Category::Inventory];
}

pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Product area for Launcher grouping (render-only).
    pub category: Category,
    /// Binary name to look up on PATH when no `binary` is pinned in config.
    /// Usually equal to `id`; it exists for tools whose executable name differs
    /// from their catalog id, so they need no per-user config to be found.
    pub default_binary: &'static str,
    pub run_mode: RunMode,
    pub launch_args: &'static [&'static str],
    /// Whether finishing this tool should trigger a background snapshot refresh.
    /// Only meaningful for `Background` (Jobs) tools. Set true when running the
    /// tool can change what a probe would observe (so the cockpit should re-read
    /// state); false for a tool whose run is self-contained and changes nothing a
    /// refresh would pick up — re-probing every adapter just because a checklist
    /// finished is needless work and a surprising side effect.
    pub refresh_after: bool,
}

/// The launcher catalog: tools the user can actually RUN.
///
/// Only launchable programs belong here. The read-only Workstate *data sections*
/// (scripts/tools/findings) are deliberately absent — they have no executable and
/// are surfaced on the Scripts/Tools screens and under the Workstate adapter, not
/// as runnable rows.
///
/// Both run modes are represented: interactive tools take over the terminal
/// (`Foreground`); tools that emit output and exit stream into the Jobs screen
/// (`Background`). Every `default_binary` here equals its `id` and is on PATH, so
/// no config is required to launch any of these — a pinned `adapters.<id>.binary`
/// is the override, not the norm.
pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "bulwark",
        name: "Bulwark",
        description: "Content/security inspection (live scan)",
        category: Category::Scripts,
        default_binary: "bulwark",
        run_mode: RunMode::Foreground,
        launch_args: &["tui"],
        // Foreground tool: it returns through the launcher path, which decides
        // refresh via LaunchReport::should_refresh — this flag is unused for it.
        refresh_after: false,
    },
    ToolEntry {
        id: "scriptvault",
        name: "ScriptVault",
        description: "Personal search engine for your scripts and tools",
        category: Category::Scripts,
        default_binary: "scriptvault",
        // With no subcommand, ScriptVault opens its interactive TUI, so it takes
        // over the terminal like Bulwark. No launch args: the bare binary is the
        // TUI entry point.
        run_mode: RunMode::Foreground,
        launch_args: &[],
        refresh_after: false,
    },
    ToolEntry {
        id: "proto",
        name: "Proto",
        // Background: Proto's checklist run emits output and exits, so it streams
        // into the Jobs screen rather than seizing the terminal. This is also the
        // catalog's live example of a streamed (RunMode::Background) tool.
        description: "Protocol / checklist runner (streams into Jobs)",
        category: Category::Scripts,
        default_binary: "proto",
        run_mode: RunMode::Background,
        launch_args: &["run"],
        // A checklist run is self-contained: it reports and exits without
        // changing anything a bulwark/system/workstate probe would observe, so
        // finishing it must NOT auto-re-probe every adapter.
        refresh_after: false,
    },
    ToolEntry {
        id: "workstate",
        name: "Workstate",
        description: "Recompile the shared suite state snapshot (streams into Jobs)",
        category: Category::System,
        default_binary: "workstate",
        // Workstate is NOT interactive: it compiles state, writes snapshot.json,
        // and exits — so it streams into Jobs. No launch args: the default output
        // path is the shared RexOps feed.
        run_mode: RunMode::Background,
        launch_args: &[],
        // The one true-refresh case in the catalog: a Workstate run rewrites the
        // very feed RexOps probes, so finishing it SHOULD re-read state. This is
        // the sanctioned use of refresh_after — a background tool that mutates
        // observable state.
        refresh_after: true,
    },
    ToolEntry {
        id: "toolfoundry",
        name: "ToolFoundry",
        description: "Tool lifecycle & inventory catalog",
        category: Category::Inventory,
        default_binary: "toolfoundry",
        // ToolFoundry's interactive surface is the `tui-catalog` subcommand, which
        // renders a terminal catalog dashboard — a Foreground takeover.
        run_mode: RunMode::Foreground,
        launch_args: &["tui-catalog"],
        refresh_after: false,
    },
];

pub fn by_id(id: &str) -> Option<&'static ToolEntry> {
    CATALOG.iter().find(|tool| tool.id == id)
}

/// True when the tool runs as a background job whose output can stream into
/// the Jobs screen (as opposed to taking over the terminal).
pub fn is_streamable(tool_id: &str) -> bool {
    matches!(
        by_id(tool_id).map(|tool| tool.run_mode),
        Some(RunMode::Background)
    )
}

/// Whether finishing this tool should kick off a background snapshot refresh.
/// Unknown ids (not in the catalog) default to `false` — no surprise re-probe.
pub fn refreshes_after(tool_id: &str) -> bool {
    by_id(tool_id).is_some_and(|tool| tool.refresh_after)
}
