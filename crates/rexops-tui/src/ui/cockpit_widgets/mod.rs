//! cockpit_widgets — the domain-free widgets the cockpit screen is built from.
//!
//! These take a `Theme`, borrowed data, and a `Rect`, and draw into a `Frame`.
//! They own no application state and import no rexops domain types beyond the
//! single mapping helper below — so a later phase can lift this whole module
//! into the shared `thomas-tui` toolkit (and re-export via `suite-ui`) almost
//! verbatim. The screen maps `ComponentStatus` into these inputs; the widgets
//! never see a `ComponentStatus`.

pub mod card_grid;
pub mod identity_banner;
pub mod status_card;
pub mod status_light;

use rexops_core::AdapterHealth;

/// The domain-free state of a health lamp. Deliberately NOT `AdapterHealth`, so
/// the widgets stay liftable into a toolkit that cannot depend on rexops-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    /// Everything good (green).
    Healthy,
    /// Works but reduced / needs attention (amber).
    Degraded,
    /// No verdict yet, or designed-but-not-wired (dim).
    Neutral,
    /// A real fault — binary gone / probe failed / disabled (red).
    Down,
}

/// Map a probed `AdapterHealth` to a lamp state. The one place the domain type
/// touches the widget vocabulary; everything else here is domain-free.
///
/// `Unknown → Neutral` on purpose: an unprobed or planned component is not a
/// fault, so it must read dim, never red. Only `Unavailable` is `Down`.
pub fn light_state_from_health(h: AdapterHealth) -> LightState {
    match h {
        AdapterHealth::Healthy => LightState::Healthy,
        AdapterHealth::Degraded => LightState::Degraded,
        AdapterHealth::Unavailable => LightState::Down,
        AdapterHealth::Unknown => LightState::Neutral,
    }
}

/// The borrowed input a `StatusCard` renders. The screen builds one of these per
/// component from a `ComponentStatus`; the widget never sees the domain type.
#[derive(Debug, Clone, Copy)]
pub struct CardInput<'a> {
    pub name: &'a str,
    pub role: &'a str,
    pub light: LightState,
    pub vital: Option<&'a str>,
    /// Render muted (a planned / inactive component).
    pub dim: bool,
    /// The single-letter hotkey label for this card, drawn dim as `[a]` before
    /// the name. `None` draws no marker (a card with no actuation letter).
    pub marker: Option<char>,
    /// Whether this card currently has cockpit focus — drawn with the accent
    /// selection rail + name, the same focus look the Launcher rows use.
    pub focused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_maps_to_the_right_light_state() {
        assert_eq!(
            light_state_from_health(AdapterHealth::Healthy),
            LightState::Healthy
        );
        assert_eq!(
            light_state_from_health(AdapterHealth::Degraded),
            LightState::Degraded
        );
        // Unavailable is a real fault → Down (red). Unknown is the pre-probe /
        // planned neutral → Neutral (dim), never a red fault.
        assert_eq!(
            light_state_from_health(AdapterHealth::Unavailable),
            LightState::Down
        );
        assert_eq!(
            light_state_from_health(AdapterHealth::Unknown),
            LightState::Neutral
        );
    }
}

// Learning Notes
// - LightState exists so the widgets never name `AdapterHealth`. That single
//   indirection is what keeps this module liftable into thomas-tui later (a
//   toolkit crate can't depend on rexops-core). The mapping lives here, at the
//   seam, not scattered through the widgets.
// - CardInput borrows (`&'a str`) rather than owning, matching suite-ui's widget
//   contract ("borrowed values only") — no per-frame allocation.
