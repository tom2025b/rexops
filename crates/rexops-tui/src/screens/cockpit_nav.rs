//! cockpit_nav.rs — the cockpit's pure focus/marker model.
//!
//! The one place that decides the order cards are visited (identical to the
//! order the grid draws them) and which letter labels each card. The renderer
//! draws the markers from here; `App::on_action` resolves a pressed letter to a
//! component id from here — so "the `a` you see is the `a` that fires" is true by
//! construction, not by two lists happening to agree. No `Frame`, no `App`: this
//! is domain logic over a borrowed `&[ComponentStatus]`.

// `marker_for` / `component_for_marker` / `MARKER_ALPHABET` are exercised by the
// unit tests below and wired into the renderer (Task 3) and the App key handling
// (Task 4). Until those consumers land they have no non-test caller, so allow
// dead_code at the module level; remove this once Task 4 calls them.
#![allow(dead_code)]

use rexops_core::ComponentStatus;

/// The metaphor groups, in display order, with the `ComponentStatus.group`
/// strings they match. Single source of truth: the renderer (`screens/cockpit`)
/// and the navigator both read THIS, so the on-screen order and the focus order
/// can never drift.
pub const GROUP_ORDER: &[(&str, &[&str])] = &[
    ("BRAIN", &["brain"]),
    ("MONITORS", &["monitor"]),
    ("BLACK BOX", &["black box"]),
    ("FIELD TOOLS", &["field tool"]),
    ("MECHANICS", &["mechanic"]),
    ("FACTORY", &["factory"]),
];

/// Letters used as card markers, in assignment order. Deliberately disjoint from
/// every nav-mode binding (`q r x j k h y n`, the drill key `g`) and all digits
/// (`1`-`7` switch screens) so a marker keypress can never shadow a global key.
pub const MARKER_ALPHABET: &[char] = &[
    'a', 's', 'd', 'f', 'w', 'e', 't', 'z', 'c', 'v', 'b', 'm', 'p', 'l', 'u', 'i', 'o',
];

/// The displayed cards in render order (the order the grid draws them):
/// group-by-group per `GROUP_ORDER`, each group's members in `comps` order.
/// A component whose group isn't listed is omitted — exactly as the renderer
/// skips it.
pub fn cockpit_visit_order(comps: &[ComponentStatus]) -> Vec<&ComponentStatus> {
    let mut out = Vec::new();
    for (_, ids) in GROUP_ORDER {
        for c in comps {
            if ids.contains(&c.group.as_str()) {
                out.push(c);
            }
        }
    }
    out
}

/// The marker letter for the Nth visited card, or `None` past the alphabet.
pub fn marker_for(visit_index: usize) -> Option<char> {
    MARKER_ALPHABET.get(visit_index).copied()
}

/// Map a pressed key to the id of the card it labels, or `None` if it labels no
/// visible card. Case-insensitive on the marker letter.
pub fn component_for_marker(comps: &[ComponentStatus], key: char) -> Option<&str> {
    let key = key.to_ascii_lowercase();
    let pos = MARKER_ALPHABET.iter().position(|&m| m == key)?;
    cockpit_visit_order(comps).get(pos).map(|c| c.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::{AdapterHealth, ComponentStatus};

    fn comp(id: &str, group: &str) -> ComponentStatus {
        ComponentStatus {
            id: id.into(),
            name: id.into(),
            group: group.into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: None,
            launchable: false,
        }
    }

    #[test]
    fn visit_order_follows_group_order_not_input_order() {
        // Input is shuffled across groups; visit order must be BRAIN then
        // FIELD TOOLS (per GROUP_ORDER), regardless of input order.
        let comps = vec![
            comp("bulwark", "field tool"),
            comp("workstate", "brain"),
            comp("proto", "field tool"),
        ];
        let order: Vec<&str> = cockpit_visit_order(&comps)
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(order, vec!["workstate", "bulwark", "proto"]);
    }

    #[test]
    fn marker_alphabet_excludes_every_bound_nav_key() {
        // The whole point: a marker letter must never be a global command.
        for bound in ['q', 'r', 'x', 'j', 'k', 'h', 'y', 'n', 'g'] {
            assert!(
                !MARKER_ALPHABET.contains(&bound),
                "marker alphabet must not contain bound nav key '{bound}'"
            );
        }
        // And it must be long enough for the whole registry (11 components).
        assert!(MARKER_ALPHABET.len() >= 11, "need a marker per component");
    }

    #[test]
    fn pressed_letter_resolves_to_the_card_it_labels() {
        let comps = vec![comp("workstate", "brain"), comp("bulwark", "field tool")];
        // First visited card → first marker ('a'); second → 's'.
        assert_eq!(marker_for(0), Some('a'));
        assert_eq!(marker_for(1), Some('s'));
        assert_eq!(component_for_marker(&comps, 'a'), Some("workstate"));
        assert_eq!(component_for_marker(&comps, 's'), Some("bulwark"));
        // Case-insensitive; an unlabeled letter resolves to nothing.
        assert_eq!(component_for_marker(&comps, 'A'), Some("workstate"));
        assert_eq!(component_for_marker(&comps, 'd'), None);
    }
}

// Learning Notes
// - cockpit_visit_order mirrors render_grid's group-by-group walk EXACTLY; the
//   marker the user presses lands on the card they see because both sides flatten
//   through this one function. Two independent lists would eventually disagree.
// - The marker alphabet is curated, not `'a'..='z'`, precisely so a card letter
//   can never collide with a global nav key. Disjointness is asserted, not hoped.
