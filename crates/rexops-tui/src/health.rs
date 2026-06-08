//! health.rs — map RexOps' AdapterHealth onto the suite's shared Health.
//!
//! All health *styling* now lives in `suite_ui::Theme::health`. This module is
//! the one piece of glue RexOps still owns: translating its own status enum into
//! the suite's coarse `Health` so the shared, NO_COLOR-safe styling applies.

use rexops_core::AdapterHealth;
use suite_ui::Health;

/// Convert an `AdapterHealth` into the suite's `Health` (a 1:1 mapping).
pub fn to_suite(health: AdapterHealth) -> Health {
    match health {
        AdapterHealth::Healthy => Health::Healthy,
        AdapterHealth::Degraded => Health::Degraded,
        AdapterHealth::Unavailable => Health::Unavailable,
        AdapterHealth::Unknown => Health::Unknown,
    }
}
