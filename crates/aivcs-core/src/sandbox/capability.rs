//! Tool capabilities — the permission axis for sandbox policy evaluation.

use serde::{Deserialize, Serialize};

/// What kind of operation a tool performs.
///
/// Used by the policy engine to decide whether a given role may invoke a tool.
/// `Custom(String)` is an escape hatch for project-specific capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    Shell,
    FileRead,
    FileWrite,
    GitRead,
    GitWrite,
    HttpFetch,
    Custom(String),
}

impl std::fmt::Display for ToolCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCapability::Shell => write!(f, "shell"),
            ToolCapability::FileRead => write!(f, "file_read"),
            ToolCapability::FileWrite => write!(f, "file_write"),
            ToolCapability::GitRead => write!(f, "git_read"),
            ToolCapability::GitWrite => write!(f, "git_write"),
            ToolCapability::HttpFetch => write!(f, "http_fetch"),
            ToolCapability::Custom(s) => write!(f, "custom({s})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_covers_all_variants() {
        assert_eq!(ToolCapability::Shell.to_string(), "shell");
        assert_eq!(ToolCapability::FileRead.to_string(), "file_read");
        assert_eq!(ToolCapability::FileWrite.to_string(), "file_write");
        assert_eq!(ToolCapability::GitRead.to_string(), "git_read");
        assert_eq!(ToolCapability::GitWrite.to_string(), "git_write");
        assert_eq!(ToolCapability::HttpFetch.to_string(), "http_fetch");
        assert_eq!(
            ToolCapability::Custom("deploy".into()).to_string(),
            "custom(deploy)"
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let caps = vec![
            ToolCapability::Shell,
            ToolCapability::FileRead,
            ToolCapability::Custom("my_cap".into()),
        ];
        let json = serde_json::to_string(&caps).unwrap();
        let back: Vec<ToolCapability> = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, back);
    }
}
