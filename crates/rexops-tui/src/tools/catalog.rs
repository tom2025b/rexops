//! Static launcher catalog and per-tool execution-mode metadata.

/// How a tool runs when launched from the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Hands over the real terminal (interactive tools).
    Foreground,
    /// Streams output into the Jobs screen.
    Background,
}

pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
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
/// Only launchable programs belong here. scripts/tools/findings/workstate are
/// deliberately absent — they are read-only Workstate *data sections*, not
/// runnable tools, and have no executable. Listing them here used to render
/// permanently-disabled dead rows in a list titled "pick a tool … Enter
/// launches" (UX-6). Their data is surfaced on the Scripts/Tools screens and
/// under the Workstate adapter, where it belongs.
///
/// Two run modes are in use: Bulwark takes over the terminal (`Foreground`),
/// Proto streams its output into the Jobs screen (`Background`).
pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "bulwark",
        name: "Bulwark",
        description: "Content/security inspection (live scan)",
        run_mode: RunMode::Foreground,
        launch_args: &["tui"],
        // Foreground tool: it returns through the launcher path, which decides
        // refresh via LaunchReport::should_refresh — this flag is unused for it.
        refresh_after: false,
    },
    ToolEntry {
        id: "proto",
        name: "Proto",
        // Background: Proto's checklist run emits output and exits, so it streams
        // into the Jobs screen rather than seizing the terminal. This is also the
        // catalog's live example of a streamed (RunMode::Background) tool.
        description: "Protocol / checklist runner (streams into Jobs)",
        run_mode: RunMode::Background,
        launch_args: &["run"],
        // A checklist run is self-contained: it reports and exits without
        // changing anything a bulwark/system/workstate probe would observe, so
        // finishing it must NOT auto-re-probe every adapter. Set true only for a
        // background tool whose run actually mutates observable state.
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
