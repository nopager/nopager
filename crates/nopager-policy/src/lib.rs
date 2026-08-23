use nopager_core::OperationsAction;
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

/// Deployment-recovery policy context retained for the v0.1 GitHub/Vercel
/// subsystem. Server/edge operations use [`OperationsPolicyContext`] instead.
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

/// Deterministic safety facts for a non-deployment production operation.
///
/// None of these values are supplied by the model. They come from trusted
/// configuration, connector capability discovery, the Kill Switch and action
/// history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationsPolicyContext {
    pub mode: SafetyMode,
    pub kill_switch_active: bool,
    pub target_configured: bool,
    pub action_enabled: bool,
    pub verification_configured: bool,
    pub cooldown_clear: bool,
}

/// Risk is assigned by NoPager code, not by the model.
#[must_use]
pub const fn operations_action_risk(action: &OperationsAction) -> ActionRisk {
    match action {
        OperationsAction::ObserveOnly | OperationsAction::Escalate { .. } => ActionRisk::Low,
        OperationsAction::RestartService { .. } | OperationsAction::RestartContainer { .. } => {
            ActionRisk::Low
        }
        OperationsAction::RestartInstance { .. } => ActionRisk::Medium,
    }
}

/// Decide whether one typed operations action may run.
///
/// Read-only/no-op choices are always allowed. A mutation is blocked if its
/// configured target, capability, independent verification or cooldown is not
/// proven by trusted code. Safe Mode requires approval for every mutation;
/// experimental Autopilot may execute only the low-risk subset automatically.
#[must_use]
pub const fn decide_operations(
    action: &OperationsAction,
    context: OperationsPolicyContext,
) -> PolicyDecision {
    if !action.is_mutating() {
        return PolicyDecision::Allow;
    }

    if context.kill_switch_active
        || !context.target_configured
        || !context.action_enabled
        || !context.verification_configured
        || !context.cooldown_clear
    {
        return PolicyDecision::Block;
    }

    match (context.mode, operations_action_risk(action)) {
        (_, ActionRisk::High | ActionRisk::Prohibited) => PolicyDecision::Block,
        (SafetyMode::Safe, _) => PolicyDecision::RequireApproval,
        (SafetyMode::AutopilotExperimental, ActionRisk::Low) => PolicyDecision::Allow,
        (SafetyMode::AutopilotExperimental, ActionRisk::Medium) => {
            PolicyDecision::RequireApproval
        }
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

    fn operations_context() -> OperationsPolicyContext {
        OperationsPolicyContext {
            mode: SafetyMode::Safe,
            kill_switch_active: false,
            target_configured: true,
            action_enabled: true,
            verification_configured: true,
            cooldown_clear: true,
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

    #[test]
    fn operations_safe_mode_requires_approval_for_restart() {
        let action = OperationsAction::RestartService {
            target_id: "api".into(),
        };
        assert_eq!(
            decide_operations(&action, operations_context()),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn operations_autopilot_allows_bounded_service_restart() {
        let action = OperationsAction::RestartService {
            target_id: "api".into(),
        };
        let context = OperationsPolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            ..operations_context()
        };
        assert_eq!(decide_operations(&action, context), PolicyDecision::Allow);
    }

    #[test]
    fn instance_restart_still_requires_approval_in_autopilot() {
        let action = OperationsAction::RestartInstance {
            target_id: "vm-primary".into(),
        };
        let context = OperationsPolicyContext {
            mode: SafetyMode::AutopilotExperimental,
            ..operations_context()
        };
        assert_eq!(
            decide_operations(&action, context),
            PolicyDecision::RequireApproval
        );
    }

    #[test]
    fn missing_verification_or_cooldown_blocks_restart() {
        let action = OperationsAction::RestartContainer {
            target_id: "web".into(),
        };
        for context in [
            OperationsPolicyContext {
                verification_configured: false,
                ..operations_context()
            },
            OperationsPolicyContext {
                cooldown_clear: false,
                ..operations_context()
            },
            OperationsPolicyContext {
                kill_switch_active: true,
                ..operations_context()
            },
        ] {
            assert_eq!(decide_operations(&action, context), PolicyDecision::Block);
        }
    }

    #[test]
    fn observe_and_escalate_never_mutate_production() {
        let blocked_context = OperationsPolicyContext {
            kill_switch_active: true,
            target_configured: false,
            action_enabled: false,
            verification_configured: false,
            cooldown_clear: false,
            ..operations_context()
        };
        assert_eq!(
            decide_operations(&OperationsAction::ObserveOnly, blocked_context),
            PolicyDecision::Allow
        );
        assert_eq!(
            decide_operations(
                &OperationsAction::Escalate {
                    reason: "insufficient evidence".into(),
                },
                blocked_context,
            ),
            PolicyDecision::Allow
        );
    }
}
