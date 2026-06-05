//! registry.rs — Simple in-memory registries for tools and adapters.
//!
//! These are *data containers*, not active services. They hold registered
//! items and provide lookup + enumeration. Population (from config, from
//! adapter discovery, from PATH scan) happens in the caller (cli, app layer).
//!
//! Why registries in core?
//! - Central place for "what do we know about?" that both CLI and TUI can query.
//! - Easy to snapshot (serialize the contained items).
//! - Type-safe keys via ToolId / AdapterId.
//! - No I/O, no mutation of external state.
//!
//! In Phase 1 they are intentionally tiny (Vec + HashMap). If hot paths
//! appear later we can swap the backing store without changing the API.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ids::{AdapterId, ToolId};

/// Lightweight descriptor for a registered adapter (name + health + optional metadata).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdapterEntry {
    pub id: AdapterId,
    pub health: crate::AdapterHealth,
    /// Human label or short description (e.g. "Bulwark content scanner").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Lightweight descriptor for a registered/known tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolEntry {
    pub id: ToolId,
    /// Optional current health/version info for the tool itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    /// Where the tool came from (adapter, manual, PATH, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Container for all known adapters.
///
/// This is the data that powers "Adapters / Status" screen and `rexops adapters list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AdapterRegistry {
    /// Primary storage. We keep a Vec for stable ordering (config order or alpha)
    /// and a HashMap for O(1) lookup by id.
    entries: Vec<AdapterEntry>,
    by_id: HashMap<String, usize>, // index into entries
}

impl AdapterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_id: HashMap::new(),
        }
    }

    /// Insert or replace an entry by id.
    ///
    /// Returns the previous entry if one was replaced.
    pub fn insert(&mut self, entry: AdapterEntry) -> Option<AdapterEntry> {
        let key = entry.id.as_str().to_owned();
        if let Some(&idx) = self.by_id.get(&key) {
            let prev = std::mem::replace(&mut self.entries[idx], entry);
            Some(prev)
        } else {
            let idx = self.entries.len();
            self.by_id.insert(key, idx);
            self.entries.push(entry);
            None
        }
    }

    /// Lookup by validated id. Returns None for unknown adapters (normal).
    pub fn get(&self, id: &AdapterId) -> Option<&AdapterEntry> {
        self.by_id.get(id.as_str()).map(|&i| &self.entries[i])
    }

    /// All entries in insertion/definition order.
    pub fn list(&self) -> &[AdapterEntry] {
        &self.entries
    }

    /// Number of adapters currently registered (including Unavailable ones).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no adapters are registered at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Count of adapters whose health allows work.
    pub fn available_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.health.is_available())
            .count()
    }
}

/// Container for known tools (scripts, binaries, logical tools).
///
/// Populated by Workstate sections, adapters, and any manual inventory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolRegistry {
    entries: Vec<ToolEntry>,
    by_id: HashMap<String, usize>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_id: HashMap::new(),
        }
    }

    pub fn insert(&mut self, entry: ToolEntry) -> Option<ToolEntry> {
        let key = entry.id.as_str().to_owned();
        if let Some(&idx) = self.by_id.get(&key) {
            let prev = std::mem::replace(&mut self.entries[idx], entry);
            Some(prev)
        } else {
            let idx = self.entries.len();
            self.by_id.insert(key, idx);
            self.entries.push(entry);
            None
        }
    }

    pub fn get(&self, id: &ToolId) -> Option<&ToolEntry> {
        self.by_id.get(id.as_str()).map(|&i| &self.entries[i])
    }

    pub fn list(&self) -> &[ToolEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// Learning Notes:
// - Dual storage (Vec + index HashMap) is a classic pattern for "stable order +
//   fast lookup" when the set is small (dozens of adapters/tools). It avoids
//   the ordering nondeterminism of HashMap iteration.
// - We store indices rather than references so that the struct remains
//   serializable and Send without lifetime headaches.
// - insert returns Option<old> so callers can detect replacement if they care
//   (e.g. health changed since last snapshot).
// - No remove() in Phase 1; registries are rebuilt from scratch on each
//   refresh cycle. Add removal later only if incremental update is required.
// - The registries do *not* own the "how to refresh" logic. That lives in
//   rexops-app (or cli for simple cases) so that core stays pure data.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::AdapterHealth;

    #[test]
    fn adapter_registry_insert_get_list_and_available_count() {
        let mut reg = AdapterRegistry::new();
        let bul = AdapterId::new("bulwark").unwrap();
        let scripts = AdapterId::new("scripts").unwrap();

        reg.insert(AdapterEntry {
            id: bul.clone(),
            health: AdapterHealth::Healthy,
            label: Some("Bulwark".to_owned()),
        });
        reg.insert(AdapterEntry {
            id: scripts,
            health: AdapterHealth::Unavailable,
            label: None,
        });

        assert_eq!(reg.len(), 2);
        assert_eq!(reg.available_count(), 1);
        assert!(reg.get(&bul).is_some());
        assert_eq!(reg.list()[0].id.as_str(), "bulwark");
    }

    #[test]
    fn tool_registry_replace_returns_old() {
        let mut reg = ToolRegistry::new();
        let t = ToolId::new("scan-secrets").unwrap();

        let first = ToolEntry {
            id: t.clone(),
            health: Some("v1".to_owned()),
            source: None,
        };
        assert!(reg.insert(first).is_none());

        let second = ToolEntry {
            id: t.clone(),
            health: Some("v2".to_owned()),
            source: Some("tools".to_owned()),
        };
        let old = reg.insert(second).unwrap();
        assert_eq!(old.health.as_deref(), Some("v1"));
        assert_eq!(reg.get(&t).unwrap().health.as_deref(), Some("v2"));
    }
}
