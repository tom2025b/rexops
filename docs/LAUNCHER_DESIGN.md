# Launcher Design Decisions

This file records the Launcher decisions that are still open before
implementation. It intentionally does not choose answers yet.

## Open Questions

- **Keybinding(s):** Which key or keys should request a launch from the TUI?
  Should launch be global, screen-specific, or both?
- **Launch scope:** Should the first launcher open a whole specialist tool from
  an adapter, or launch a specific surfaced item such as a ScriptVault script,
  ToolFoundry tool, or Bulwark scan?
- **Initial surfaces:** Which screens should expose launch first: Adapters,
  Scripts, Tools, Dashboard, or another screen?
- **Target resolution:** Should RexOps resolve specialist binaries from `PATH`,
  from config-declared paths, or from a priority order that uses both?
- **First target:** What exact ScriptVault command should RexOps launch first:
  its TUI, a specific CLI subcommand, or another entry point?
- **Unavailable tools:** What message and hint should RexOps show when a target
  binary is missing or cannot be executed?
- **Terminal handoff:** What should the user see while RexOps suspends its TUI,
  gives the real terminal to the launched tool, and restores afterward?
- **Return behavior:** After the launched tool exits, should RexOps auto-refresh
  its snapshot, show the child exit status, append a log event, or do nothing?
- **Failure semantics:** How should RexOps report a non-zero child exit status,
  especially when a specialist may use non-zero exits for attention/status?
- **Boundary data:** What context may RexOps pass to a specialist without
  absorbing specialist behavior: adapter ID, selected item ID, config path, or
  something else?
