//! NoPager's provider-independent domain model.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IncidentState {
    Open,
    CollectingContext,
    Diagnosing,
    Planning,
    Repairing,
    Testing,
    PreviewDeploying,
    VerifyingPreview,
    WaitingApproval,
    ProductionDeploying,
    VerifyingProduction,
    RollingBack,
    RolledBack,
    Resolved,
    Failed,
    Escalated,
    Cancelled,
    Ignored,
    Duplicate,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UiIncidentState {
    Open,
    Diagnosing,
    Repairing,
    WaitingApproval,
    Resolved,
    HumanNeeded,
    Paused,
}

impl IncidentState {
    #[must_use]
    pub const fn ui_projection(self) -> UiIncidentState {
        match self {
            Self::Open | Self::CollectingContext => UiIncidentState::Open,
            Self::Diagnosing | Self::Planning => UiIncidentState::Diagnosing,
            Self::Repairing
            | Self::Testing
            | Self::PreviewDeploying
            | Self::VerifyingPreview
            | Self::ProductionDeploying
            | Self::VerifyingProduction
            | Self::RollingBack => UiIncidentState::Repairing,
            Self::WaitingApproval => UiIncidentState::WaitingApproval,
            Self::Resolved | Self::RolledBack | Self::Cancelled => UiIncidentState::Resolved,
            Self::Failed | Self::Escalated => UiIncidentState::HumanNeeded,
            Self::Paused => UiIncidentState::Paused,
            Self::Ignored | Self::Duplicate => UiIncidentState::Resolved,
        }
    }

    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use IncidentState as S;
        matches!(
            (self, next),
            (S::Open, S::CollectingContext)
                | (S::CollectingContext, S::Diagnosing)
                | (S::Diagnosing, S::Planning)
                | (S::Planning, S::Repairing)
                | (S::Repairing, S::Testing)
                | (S::Testing, S::Repairing)
                | (S::Testing, S::PreviewDeploying)
                | (S::PreviewDeploying, S::VerifyingPreview)
                | (S::VerifyingPreview, S::Repairing)
                | (S::VerifyingPreview, S::WaitingApproval)
                | (S::VerifyingPreview, S::ProductionDeploying)
                | (S::WaitingApproval, S::ProductionDeploying)
                | (S::ProductionDeploying, S::VerifyingProduction)
                | (S::VerifyingProduction, S::Resolved)
                | (S::VerifyingProduction, S::RollingBack)
                | (S::RollingBack, S::RolledBack)
                | (S::RolledBack, S::Resolved)
                | (_, S::Escalated | S::Failed | S::Cancelled | S::Paused)
                | (S::Paused, S::CollectingContext)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub state: IncidentState,
    pub repair_attempts: u8,
}

impl Incident {
    pub fn transition(&mut self, next: IncidentState) -> Result<(), TransitionError> {
        if !self.state.can_transition_to(next) {
            return Err(TransitionError {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid incident transition from {from:?} to {to:?}")]
pub struct TransitionError {
    pub from: IncidentState,
    pub to: IncidentState,
}

/// Broad cause class selected during operations triage.
///
/// This is intentionally independent of any one provider. A deployment
/// regression is one possible cause, not NoPager's product identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationsIncidentClass {
    ServiceHealth,
    HostHealth,
    Capacity,
    TrafficAbuse,
    EdgeSecurity,
    Dependency,
    Database,
    DeploymentRegression,
    Unknown,
}

/// A trusted operation intent. The model may select one of these variants, but
/// connector-specific commands or API request bodies are constructed by trusted
/// NoPager code after policy validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationsAction {
    ObserveOnly,
    RestartService { target_id: String },
    RestartContainer { target_id: String },
    RestartInstance { target_id: String },
    Escalate { reason: String },
}

impl OperationsAction {
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::RestartService { .. }
                | Self::RestartContainer { .. }
                | Self::RestartInstance { .. }
        )
    }

    #[must_use]
    pub fn target_id(&self) -> Option<&str> {
        match self {
            Self::RestartService { target_id }
            | Self::RestartContainer { target_id }
            | Self::RestartInstance { target_id } => Some(target_id),
            Self::ObserveOnly | Self::Escalate { .. } => None,
        }
    }

    pub fn validate(&self) -> Result<(), OperationsPlanError> {
        if let Some(target_id) = self.target_id() {
            validate_bounded_identifier(target_id, "target_id")?;
        }
        if let Self::Escalate { reason } = self {
            validate_nonempty_bounded_text(reason, "escalation reason", 2_000)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSignalKind {
    ExternalHttp,
    ExternalTcp,
    ServiceStatus,
    ContainerStatus,
    InstanceHealth,
    ErrorRate,
    ResourcePressure,
    TrafficHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationSignal {
    pub kind: VerificationSignalKind,
    /// Stable configured monitor/connector identity. This is not an arbitrary
    /// command, URL, or provider request body supplied by the model.
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationsVerificationPlan {
    pub signals: Vec<VerificationSignal>,
    pub required_consecutive_successes: u8,
    pub timeout_seconds: u32,
}

impl OperationsVerificationPlan {
    pub fn validate(&self) -> Result<(), OperationsPlanError> {
        if self.signals.is_empty() || self.signals.len() > 8 {
            return Err(OperationsPlanError::InvalidVerification);
        }
        for signal in &self.signals {
            validate_bounded_identifier(&signal.source_id, "verification source_id")?;
        }
        if !(1..=10).contains(&self.required_consecutive_successes)
            || !(1..=3_600).contains(&self.timeout_seconds)
        {
            return Err(OperationsPlanError::InvalidVerification);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationsEvidence {
    pub source: String,
    pub finding: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationsPlan {
    pub incident_class: OperationsIncidentClass,
    pub summary: String,
    pub confidence: f32,
    pub evidence: Vec<OperationsEvidence>,
    pub action: OperationsAction,
    pub verification: Option<OperationsVerificationPlan>,
    pub rollback_or_fallback: String,
}

impl OperationsPlan {
    pub fn validate(&self) -> Result<(), OperationsPlanError> {
        validate_nonempty_bounded_text(&self.summary, "summary", 4_000)?;
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(OperationsPlanError::InvalidConfidence);
        }
        if self.evidence.is_empty() || self.evidence.len() > 32 {
            return Err(OperationsPlanError::MissingEvidence);
        }
        for evidence in &self.evidence {
            validate_nonempty_bounded_text(&evidence.source, "evidence source", 256)?;
            validate_nonempty_bounded_text(&evidence.finding, "evidence finding", 8_000)?;
        }
        self.action.validate()?;
        validate_nonempty_bounded_text(
            &self.rollback_or_fallback,
            "rollback/fallback plan",
            4_000,
        )?;
        match (&self.action, &self.verification) {
            (action, Some(verification)) if action.is_mutating() => verification.validate()?,
            (action, None) if action.is_mutating() => {
                return Err(OperationsPlanError::MissingVerification);
            }
            (_, Some(verification)) => verification.validate()?,
            (_, None) => {}
        }
        Ok(())
    }
}

fn validate_bounded_identifier(value: &str, field: &'static str) -> Result<(), OperationsPlanError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.chars().any(char::is_control)
        || trimmed.contains('\n')
        || trimmed.contains('\r')
    {
        return Err(OperationsPlanError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_nonempty_bounded_text(
    value: &str,
    field: &'static str,
    max_chars: usize,
) -> Result<(), OperationsPlanError> {
    if value.trim().is_empty() || value.chars().count() > max_chars {
        return Err(OperationsPlanError::InvalidText(field));
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq)]
pub enum OperationsPlanError {
    #[error("operations plan confidence must be between zero and one")]
    InvalidConfidence,
    #[error("operations plan must contain bounded evidence")]
    MissingEvidence,
    #[error("invalid operations identifier: {0}")]
    InvalidIdentifier(&'static str),
    #[error("invalid or empty operations text field: {0}")]
    InvalidText(&'static str),
    #[error("mutating operations actions require an independent verification plan")]
    MissingVerification,
    #[error("operations verification plan is invalid")]
    InvalidVerification,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_projection_hides_internal_noise() {
        assert_eq!(
            IncidentState::VerifyingPreview.ui_projection(),
            UiIncidentState::Repairing
        );
        assert_eq!(
            IncidentState::Escalated.ui_projection(),
            UiIncidentState::HumanNeeded
        );
    }

    #[test]
    fn production_cannot_skip_approval_path_from_safe_preview() {
        assert!(!IncidentState::VerifyingPreview.can_transition_to(IncidentState::Resolved));
        assert!(IncidentState::VerifyingPreview.can_transition_to(IncidentState::WaitingApproval));
    }

    fn external_health_verification() -> OperationsVerificationPlan {
        OperationsVerificationPlan {
            signals: vec![VerificationSignal {
                kind: VerificationSignalKind::ExternalHttp,
                source_id: "primary_https_health".into(),
            }],
            required_consecutive_successes: 2,
            timeout_seconds: 60,
        }
    }

    #[test]
    fn restart_requires_independent_verification() {
        let plan = OperationsPlan {
            incident_class: OperationsIncidentClass::ServiceHealth,
            summary: "service is unhealthy while deployment state is unchanged".into(),
            confidence: 0.91,
            evidence: vec![OperationsEvidence {
                source: "external_health".into(),
                finding: "three consecutive probes failed".into(),
            }],
            action: OperationsAction::RestartService {
                target_id: "checkout-api".into(),
            },
            verification: None,
            rollback_or_fallback: "escalate if restart does not restore health".into(),
        };
        assert_eq!(plan.validate(), Err(OperationsPlanError::MissingVerification));
    }

    #[test]
    fn accepts_bounded_typed_restart_plan() {
        let plan = OperationsPlan {
            incident_class: OperationsIncidentClass::ServiceHealth,
            summary: "configured service is unhealthy".into(),
            confidence: 0.95,
            evidence: vec![OperationsEvidence {
                source: "external_health".into(),
                finding: "health threshold crossed".into(),
            }],
            action: OperationsAction::RestartService {
                target_id: "checkout-api".into(),
            },
            verification: Some(external_health_verification()),
            rollback_or_fallback: "stop after one attempt and escalate if health stays down".into(),
        };
        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn non_mutating_actions_do_not_require_fake_verification() {
        let plan = OperationsPlan {
            incident_class: OperationsIncidentClass::Unknown,
            summary: "evidence is insufficient to mutate production".into(),
            confidence: 0.4,
            evidence: vec![OperationsEvidence {
                source: "health".into(),
                finding: "signal is ambiguous".into(),
            }],
            action: OperationsAction::Escalate {
                reason: "need more evidence".into(),
            },
            verification: None,
            rollback_or_fallback: "leave production unchanged".into(),
        };
        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn typed_actions_cannot_carry_raw_shell_commands() {
        let serialized = serde_json::to_value(OperationsAction::RestartService {
            target_id: "api.service".into(),
        })
        .unwrap();
        assert_eq!(serialized["kind"], "restart_service");
        assert!(serialized.get("command").is_none());
        assert!(serialized.get("shell").is_none());
    }
}
