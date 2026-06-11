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
}

pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "bulwark",
        name: "Bulwark",
        description: "Content/security inspection (live scan)",
        run_mode: RunMode::Foreground,
    },
    ToolEntry {
        id: "proto",
        name: "Proto",
        description: "Guided protocol / checklist runner (interactive)",
        run_mode: RunMode::Foreground,
    },
    ToolEntry {
        id: "scripts",
        name: "Scripts",
        description: "Script inventory from Workstate",
        run_mode: RunMode::Background,
    },
    ToolEntry {
        id: "tools",
        name: "Tools",
        description: "Tool ownership & lifecycle from Workstate",
        run_mode: RunMode::Background,
    },
    ToolEntry {
        id: "workstate",
        name: "Workstate",
        description: "Snapshot source of truth",
        run_mode: RunMode::Background,
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
