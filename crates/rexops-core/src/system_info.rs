//! system_info.rs — Pure data type for collected system information.
//!
//! Moved to core so OpsSnapshot can hold SystemInfo without depending on the
//! adapter execution layer. SystemAdapter (in rexops-adapters) imports this type
//! from here and returns it wrapped in AdapterOutput<SystemInfo>.

use serde::{Deserialize, Serialize};

/// Basic system information collected from standard tools.
/// All fields optional so partial success (some cmds fail) still gives value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SystemInfo {
    /// Hostname (from `hostname` or `uname -n`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    /// Kernel/OS info (from `uname -sr` or similar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,

    /// Uptime string (from `uptime`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,

    /// Simple disk usage summary lines (from `df -h` head).
    #[serde(default)]
    pub disk: Vec<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_empty() {
        let info = SystemInfo::default();
        assert!(info.hostname.is_none());
        assert!(info.kernel.is_none());
        assert!(info.uptime.is_none());
        assert!(info.disk.is_empty());
    }

    #[test]
    fn roundtrips_via_serde() {
        let info = SystemInfo {
            hostname: Some("testhost".into()),
            kernel: Some("Linux 6.1".into()),
            uptime: Some("up 3 days".into()),
            disk: vec!["/dev/sda1  50G  20G  30G  41% /".into()],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: SystemInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn none_fields_are_omitted_but_disk_vec_is_kept() {
        // hostname/kernel/uptime skip when None; disk has no skip and stays as [].
        let json = serde_json::to_string(&SystemInfo::default()).unwrap();
        assert!(!json.contains("hostname"), "None hostname omitted, got: {json}");
        assert!(json.contains("\"disk\":[]"), "empty disk still present, got: {json}");
    }

    #[test]
    fn partial_json_deserializes_with_defaults() {
        // A snapshot carrying only a hostname must parse; the rest defaults.
        let info: SystemInfo = serde_json::from_str(r#"{"hostname":"h"}"#).unwrap();
        assert_eq!(info.hostname.as_deref(), Some("h"));
        assert!(info.kernel.is_none());
        assert!(info.disk.is_empty());
    }
}
