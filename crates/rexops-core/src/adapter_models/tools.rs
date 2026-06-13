//! tools.rs — Workstate tools data types (pure data, no execution).

use serde::{Deserialize, Serialize};

pub(super) fn bool_or_null<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(false))
}

/// One tool as reported by Workstate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Tool {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub lifecycle_state: String,
    #[serde(default, deserialize_with = "bool_or_null")]
    pub review_due: bool,
    #[serde(default)]
    pub review_after: Option<String>,
    #[serde(default)]
    pub review_due_flag: bool,
    #[serde(default)]
    pub health_passed: u32,
    #[serde(default)]
    pub health_total: u32,
    #[serde(default)]
    pub drifted: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub manifest_path: String,
}

impl Tool {
    pub fn needs_attention(&self) -> bool {
        self.status == "attention"
    }
}

/// The whole tools payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolsInfo {
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub as_of: String,
    #[serde(default)]
    pub tool_count: usize,
    #[serde(default)]
    pub attention_count: usize,
    #[serde(default)]
    pub tools: Vec<Tool>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn needs_attention_only_for_attention_status() {
        let mut t = Tool {
            status: "attention".into(),
            ..Tool::default()
        };
        assert!(t.needs_attention());
        t.status = "ok".into();
        assert!(!t.needs_attention());
    }

    #[test]
    fn review_due_null_deserializes_as_false() {
        // The crux of bool_or_null: Workstate sends review_due as JSON null for
        // compat. Plain serde rejects null for a non-Option bool; our custom
        // deserializer must turn it into false rather than failing the parse.
        let t: Tool =
            serde_json::from_str(r#"{"id":"x","display_name":"X","review_due":null}"#).unwrap();
        assert!(!t.review_due, "explicit null → false");
    }

    #[test]
    fn review_due_absent_and_true_both_work() {
        let absent: Tool = serde_json::from_str(r#"{"id":"x","display_name":"X"}"#).unwrap();
        assert!(!absent.review_due, "omitted → false");

        let truthy: Tool =
            serde_json::from_str(r#"{"id":"x","display_name":"X","review_due":true}"#).unwrap();
        assert!(truthy.review_due, "explicit true → true");
    }

    #[test]
    fn required_fields_parse_and_optionals_default() {
        let t: Tool =
            serde_json::from_str(r#"{"id":"svc","display_name":"Service","status":"ok"}"#).unwrap();
        assert_eq!(t.id, "svc");
        assert_eq!(t.display_name, "Service");
        assert_eq!(t.owner, "", "absent owner defaults empty");
        assert_eq!(t.health_total, 0);
        assert!(t.review_after.is_none());
    }

    #[test]
    fn tools_info_roundtrips_via_serde() {
        let info = ToolsInfo {
            schema_version: 3,
            as_of: "2026-06-13".into(),
            tool_count: 1,
            attention_count: 0,
            tools: vec![Tool {
                id: "a".into(),
                display_name: "A".into(),
                ..Tool::default()
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ToolsInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }
}
