//! Launch-availability service: the domain logic for "can this catalog tool be
//! run right now?", split out of the front-end so every caller (TUI launcher,
//! command palette, any future front-end) shares one source of truth.
//!
//! Availability has two halves with different costs:
//!   - *resolvability* — does the tool's command resolve from config + PATH?
//!     This shells out to `which`, so it is computed ONCE into a cache and
//!     rebuilt only when config changes (`refresh`).
//!   - *live health* — what did the adapter probe last report? This is cheap
//!     and changes every refresh, so it is NOT cached here: callers pass the
//!     current [`AdapterHealth`] in at the decision point.
//!
//! Keeping health out of the cache is deliberate: it preserves the cheap
//! once-computed resolvability cache (no `which` per render frame) while letting
//! the launchable/unavailable verdict track live probe results.

use std::collections::HashMap;

use rexops_core::{AdapterHealth, AppConfig};

use super::{catalog::CATALOG, is_streamable, resolve_launch_command};

/// The 3-state launch availability of a catalog tool — the single domain
/// verdict shared by every run surface so they can never disagree about what is
/// runnable. Front-ends map this to their own presentation (glyphs, colours,
/// wording); the domain layer stays UI-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityTag {
    /// Command resolves AND the adapter is not `Unavailable`. The `streamable`
    /// flag distinguishes a background/streaming tool from an interactive one,
    /// so a front-end can word it ("streams" vs "interactive") without
    /// re-deriving the run mode.
    Available { streamable: bool },
    /// Command resolves, but the adapter probe reports it `Unavailable` (binary
    /// gone or administratively disabled).
    Unavailable,
    /// Command does not resolve at all (no launch command configured / on PATH).
    Disabled,
}

/// Owns the per-catalog-tool resolvability cache (config + PATH), and answers
/// the launchable / available / tag questions when given the live adapter
/// health. Construct with [`Availability::new`]; rebuild the cache whenever
/// config changes via [`Availability::refresh`].
#[derive(Debug, Clone, Default)]
pub struct Availability {
    /// Per-catalog-tool "command resolves?" flag, derived from config + PATH.
    /// Rebuilt only by `refresh`; read on the render hot path, so it never
    /// shells out to `which` per frame.
    resolvable: HashMap<&'static str, bool>,
}

impl Availability {
    /// Build the service and populate its resolvability cache from `config`.
    pub fn new(config: &AppConfig) -> Self {
        let mut svc = Self {
            resolvable: HashMap::new(),
        };
        svc.refresh(config);
        svc
    }

    /// Recompute the resolvability cache for every catalog tool from `config`
    /// (and PATH). Call this whenever config changes — it is the only writer of
    /// the cache, which is what keeps the cache from drifting from config.
    pub fn refresh(&mut self, config: &AppConfig) {
        self.resolvable = CATALOG
            .iter()
            .map(|tool| (tool.id, resolve_launch_command(tool.id, config).is_some()))
            .collect();
    }

    /// Whether a catalog tool's command RESOLVES — read from the cached
    /// config+PATH availability. The cheap, snapshot-independent half of
    /// launchability. Unknown ids (not in the catalog) read as not resolvable.
    /// Prefer [`Availability::is_available`] at decision points — it also folds
    /// in live adapter health.
    pub fn is_launchable(&self, tool_id: &str) -> bool {
        self.resolvable.get(tool_id).copied().unwrap_or(false)
    }

    /// Whether a tool should be offered for launch RIGHT NOW: its command must
    /// resolve (cached config+PATH) AND `health` must not be `Unavailable`.
    ///
    /// Health is combined here, at the decision point, rather than baked into
    /// the cache — so the cheap once-computed resolvability cache survives.
    /// `Unknown` and `Degraded` stay launchable on purpose: `Unknown` is the
    /// pre-probe state (blocking it would make every tool unlaunchable for the
    /// first moment after startup), and a `Degraded` tool is often exactly what
    /// you want to launch to inspect or fix it. Only `Unavailable` — binary gone
    /// or administratively disabled — blocks the launch.
    pub fn is_available(&self, tool_id: &str, health: AdapterHealth) -> bool {
        self.is_launchable(tool_id) && health != AdapterHealth::Unavailable
    }

    /// The 3-state availability verdict for a catalog tool, given its live
    /// adapter `health`. The single source of truth shared by every run surface
    /// (the launcher rows and the command palette) so they can never disagree
    /// about what is runnable.
    pub fn tag(&self, tool_id: &str, health: AdapterHealth) -> AvailabilityTag {
        if self.is_available(tool_id, health) {
            AvailabilityTag::Available {
                streamable: is_streamable(tool_id),
            }
        } else if self.is_launchable(tool_id) {
            AvailabilityTag::Unavailable
        } else {
            AvailabilityTag::Disabled
        }
    }

    /// Override a single tool's cached resolvability. Test scaffolding: it lets
    /// render-path tests prove they read the cache rather than resolving live.
    /// No production code calls it — `refresh` is the real writer — so it is not
    /// `#[cfg(test)]`-gated only because front-end tests in another crate need
    /// it (a `#[cfg(test)]` item would not be visible across the crate boundary,
    /// and a whole test-only feature is overkill for one cache insert).
    pub fn set_launchable(&mut self, tool_id: &'static str, launchable: bool) {
        self.resolvable.insert(tool_id, launchable);
    }
}
