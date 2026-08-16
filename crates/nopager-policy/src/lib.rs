use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyMode {
    #[default]
    Safe,
    AutopilotExperimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionRisk {
    Low,
    Medium,
    High,
    Prohibited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    RequireApproval,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyContext {
    pub mode: SafetyMode,
    pub kill_switch_active: bool,
    pub preview_verified: bool,
    pub reversible: bool,
}

#[must_use]
pub const fn decide(risk: ActionRisk, context: PolicyContext) -> PolicyDecision {
    if context.kill_switch_active || matches!(risk, ActionRisk::High | ActionRisk::Prohibited) {
        return PolicyDecision::Block;
    }
    if !context.preview_verified || !context.reversible {
        return PolicyDecision::RequireApproval;
    }
    match (context.mode, risk) {
        (SafetyMode::Safe, _) | (_, ActionRisk::Medium) => PolicyDecision::RequireApproval,
        (SafetyMode::AutopilotExperimental, ActionRisk::Low) => PolicyDecision::Allow,
        (_, ActionRisk::High | ActionRisk::Prohibited) => PolicyDecision::Block,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_context() -> PolicyContext {
        PolicyContext {
            mode: SafetyMode::Safe,
            kill_switch_active: false,
            preview_verified: true,
            reversible: true,
        }
    }

    #[test]
    fn safe_mode_always_requires_production_approval() {
        assert_eq!(
            decide(ActionRisk::Low, safe_context()),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn kill_switch_blocks_mutation_even_in_autopilot() {
        let context = PolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            kill_switch_active: true,
            ..safe_context()
        };
        assert_eq!(decide(ActionRisk::Low, context), PolicyDecision::Block);
    }

    #[test]
    fn unverified_or_irreversible_actions_are_never_automatic() {
        let context = PolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            preview_verified: false,
            reversible: false,
            ..safe_context()
        };
        assert_eq!(
            decide(ActionRisk::Low, context),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn high_risk_actions_are_blocked_in_every_mode() {
        assert_eq!(
            decide(ActionRisk::High, safe_context()),
            PolicyDecision::Block
        );
    }
}
