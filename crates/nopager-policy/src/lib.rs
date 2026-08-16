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
    // A kill switch or a high/prohibited action is a hard stop. Human approval
    // must never turn these into a production mutation.
    if context.kill_switch_active || matches!(risk, ActionRisk::High | ActionRisk::Prohibited) {
        return PolicyDecision::Block;
    }

    // Preview verification is a mandatory production safety gate. A failed or
    // missing preview must stop the rollout rather than merely ask for approval.
    if !context.preview_verified {
        return PolicyDecision::Block;
    }

    // A repair without a known rollback target is never allowed to promote
    // automatically. Safe Mode already requires approval for every production
    // mutation; Autopilot falls back to the same requirement here.
    if !context.reversible {
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
    fn unverified_preview_is_a_hard_block_in_every_mode() {
        for mode in [SafetyMode::Safe, SafetyMode::AutopilotExperimental] {
            let context = PolicyContext {
                mode,
                preview_verified: false,
                ..safe_context()
            };
            assert_eq!(decide(ActionRisk::Low, context), PolicyDecision::Block);
        }
    }

    #[test]
    fn irreversible_action_can_never_autopromote() {
        let context = PolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            reversible: false,
            ..safe_context()
        };
        assert_eq!(
            decide(ActionRisk::Low, context),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn verified_reversible_low_risk_action_can_autopromote() {
        let context = PolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            ..safe_context()
        };
        assert_eq!(decide(ActionRisk::Low, context), PolicyDecision::Allow);
    }

    #[test]
    fn medium_risk_action_always_requires_approval() {
        let context = PolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            ..safe_context()
        };
        assert_eq!(
            decide(ActionRisk::Medium, context),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn high_and_prohibited_actions_are_blocked_in_every_mode() {
        for risk in [ActionRisk::High, ActionRisk::Prohibited] {
            assert_eq!(decide(risk, safe_context()), PolicyDecision::Block);
            let autopilot = PolicyContext {
                mode: SafetyMode::AutopilotExperimental,
                ..safe_context()
            };
            assert_eq!(decide(risk, autopilot), PolicyDecision::Block);
        }
    }
}
