//! scripts.rs — Workstate scripts data types (pure data, no execution).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One script entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Script {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl Script {
    pub fn label(&self) -> &str {
        self.id
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("<unnamed>")
    }
}

/// The whole scripts payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScriptsInfo {
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub scripts: Vec<Script>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub recents: Vec<String>,
}

impl ScriptsInfo {
    pub fn total(&self) -> usize {
        self.scripts.len()
    }

    pub fn favorites_count(&self) -> usize {
        self.favorites.len()
    }

    pub fn recents_count(&self) -> usize {
        self.recents.len()
    }

    pub fn is_favorite(&self, script: &Script) -> bool {
        self.favorites.iter().any(|f| {
            Some(f.as_str()) == script.id.as_deref() || Some(f.as_str()) == script.name.as_deref()
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn script(id: Option<&str>, name: Option<&str>) -> Script {
        Script {
            id: id.map(str::to_owned),
            name: name.map(str::to_owned),
            ..Script::default()
        }
    }

    #[test]
    fn label_prefers_id_then_name_then_placeholder() {
        assert_eq!(script(Some("the-id"), Some("the-name")).label(), "the-id");
        assert_eq!(script(None, Some("the-name")).label(), "the-name");
        assert_eq!(script(None, None).label(), "<unnamed>");
    }

    #[test]
    fn counts_reflect_the_vecs() {
        let info = ScriptsInfo {
            scripts: vec![script(Some("a"), None), script(Some("b"), None)],
            favorites: vec!["a".into()],
            recents: vec!["a".into(), "b".into()],
            ..ScriptsInfo::default()
        };
        assert_eq!(info.total(), 2);
        assert_eq!(info.favorites_count(), 1);
        assert_eq!(info.recents_count(), 2);
    }

    #[test]
    fn is_favorite_matches_by_id_or_name() {
        let info = ScriptsInfo {
            favorites: vec!["fav-id".into(), "fav-name".into()],
            ..ScriptsInfo::default()
        };
        assert!(info.is_favorite(&script(Some("fav-id"), None)), "id match");
        assert!(info.is_favorite(&script(None, Some("fav-name"))), "name fallback");
        assert!(!info.is_favorite(&script(Some("other"), Some("nope"))), "no match");
    }

    #[test]
    fn unknown_keys_are_preserved_in_rest() {
        // additionalProperties: a script export carrying extra keys must round-trip
        // them through `rest` rather than dropping or rejecting them.
        let s: Script = serde_json::from_str(r#"{"id":"x","custom":"keepme"}"#).unwrap();
        assert_eq!(s.id.as_deref(), Some("x"));
        assert_eq!(s.rest.get("custom").and_then(|v| v.as_str()), Some("keepme"));
    }
}
