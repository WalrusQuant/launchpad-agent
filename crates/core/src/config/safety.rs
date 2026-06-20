use lpa_safety::{SandboxMode, SandboxPolicyRecord};
use serde::{Deserialize, Serialize};

/// Selects the model used for safety-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyPolicyModelSelection {
    /// Use the active turn model for compaction summaries.
    UseTurnModel,
    /// Use a separately configured auxiliary model for safety classification.
    UseAxiliaryModel,
}

/// User-facing sandbox configuration parsed from the `[sandbox]` config section.
///
/// Defaults to disabled so existing setups keep their current unrestricted
/// behavior. When `enabled`, the query loop attaches a `Restricted` sandbox
/// that scopes file writes and shell execution to the session workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// When false (default) the agent runs unrestricted.
    pub enabled: bool,
    /// When true, file writes are allowed inside the session workspace (cwd).
    /// When false, all file writes and shell execution are denied.
    pub workspace_write: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            workspace_write: true,
        }
    }
}

impl SandboxConfig {
    /// Converts the config into a runtime sandbox record, or `None` when the
    /// sandbox is disabled (the query loop then builds an unrestricted policy).
    pub fn to_policy_record(&self) -> Option<SandboxPolicyRecord> {
        if !self.enabled {
            return None;
        }
        Some(SandboxPolicyRecord {
            mode: SandboxMode::Restricted,
            workspace_write: self.workspace_write,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn disabled_sandbox_yields_no_policy_record() {
        assert_eq!(SandboxConfig::default().to_policy_record(), None);
    }

    #[test]
    fn enabled_sandbox_maps_to_restricted_record() {
        let config = SandboxConfig {
            enabled: true,
            workspace_write: true,
        };
        assert_eq!(
            config.to_policy_record(),
            Some(SandboxPolicyRecord {
                mode: SandboxMode::Restricted,
                workspace_write: true,
            })
        );
    }
}
