use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::Evidence;

const MAX_INPUT_ACTIONS: usize = 32;
const MAX_INPUT_VERIFICATION_SIGNALS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationsActionKind {
    ObserveOnly,
    DelegateDeploymentRecovery,
    RestartService,
    RestartContainer,
    RestartInstance,
    Escalate,
}

impl OperationsActionKind {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::RestartService | Self::RestartContainer | Self::RestartInstance
        )
    }

    #[must_use]
    pub const fn requires_target(self) -> bool {
        matches!(
            self,
            Self::RestartService | Self::RestartContainer | Self::RestartInstance
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObserveOnly => "observe_only",
            Self::DelegateDeploymentRecovery => "delegate_deployment_recovery",
            Self::RestartService => "restart_service",
            Self::RestartContainer => "restart_container",
            Self::RestartInstance => "restart_instance",
            Self::Escalate => "escalate",
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationsVerificationKind {
    ExternalHttp,
    ExternalTcp,
    ServiceStatus,
    ContainerStatus,
    InstanceHealth,
    ErrorRate,
    ResourcePressure,
    TrafficHealth,
}

impl OperationsVerificationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExternalHttp => "external_http",
            Self::ExternalTcp => "external_tcp",
            Self::ServiceStatus => "service_status",
            Self::ContainerStatus => "container_status",
            Self::InstanceHealth => "instance_health",
            Self::ErrorRate => "error_rate",
            Self::ResourcePressure => "resource_pressure",
            Self::TrafficHealth => "traffic_health",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableOperationsAction {
    pub kind: OperationsActionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub description: String,
}

impl AvailableOperationsAction {
    #[must_use]
    pub fn new(
        kind: OperationsActionKind,
        target_id: Option<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            target_id,
            description: description.into(),
        }
    }

    fn validate(&self) -> Result<(), OperationsValidationError> {
        validate_text(&self.description, "available action description", 2_000)?;
        match (self.kind.requires_target(), self.target_id.as_deref()) {
            (true, Some(target)) => validate_identifier(target, "available targetId"),
            (true, None) => Err(OperationsValidationError::InvalidAvailableAction),
            (false, None) => Ok(()),
            (false, Some(_)) => Err(OperationsValidationError::InvalidAvailableAction),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableVerificationSignal {
    pub kind: OperationsVerificationKind,
    pub source_id: String,
    pub description: String,
}

impl AvailableVerificationSignal {
    #[must_use]
    pub fn new(
        kind: OperationsVerificationKind,
        source_id: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            source_id: source_id.into(),
            description: description.into(),
        }
    }

    fn validate(&self) -> Result<(), OperationsValidationError> {
        validate_identifier(&self.source_id, "verification sourceId")?;
        validate_text(&self.description, "verification description", 2_000)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationsInput {
    pub incident_summary: String,
    pub trigger: Value,
    pub health: Value,
    pub infrastructure: Value,
    pub deployment: Value,
    pub available_actions: Vec<AvailableOperationsAction>,
    pub verification_signals: Vec<AvailableVerificationSignal>,
}

impl OperationsInput {
    pub fn validate(&self) -> Result<(), OperationsValidationError> {
        validate_text(&self.incident_summary, "incident summary", 4_000)?;
        if self.available_actions.is_empty() || self.available_actions.len() > MAX_INPUT_ACTIONS {
            return Err(OperationsValidationError::InvalidAvailableAction);
        }
        for action in &self.available_actions {
            action.validate()?;
        }
        if self.verification_signals.len() > MAX_INPUT_VERIFICATION_SIGNALS {
            return Err(OperationsValidationError::InvalidVerification);
        }
        for signal in &self.verification_signals {
            signal.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedVerificationSignal {
    pub kind: OperationsVerificationKind,
    pub source_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationsDecision {
    pub incident_class: OperationsIncidentClass,
    pub summary: String,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
    pub action: OperationsActionKind,
    pub target_id: String,
    pub escalation_reason: String,
    pub verification_signals: Vec<SelectedVerificationSignal>,
    pub required_consecutive_successes: u8,
    pub timeout_seconds: u32,
    pub rollback_or_fallback: String,
}

impl OperationsDecision {
    pub fn validate_against(
        &self,
        input: &OperationsInput,
    ) -> Result<(), OperationsValidationError> {
        validate_text(&self.summary, "summary", 4_000)?;
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(OperationsValidationError::InvalidConfidence);
        }
        if self.evidence.is_empty() || self.evidence.len() > 32 {
            return Err(OperationsValidationError::MissingEvidence);
        }
        for evidence in &self.evidence {
            validate_text(&evidence.source, "evidence source", 256)?;
            validate_text(&evidence.finding, "evidence finding", 8_000)?;
        }
        validate_text(&self.rollback_or_fallback, "rollback/fallback", 4_000)?;

        let target = self.target_id.trim();
        if self.action.requires_target() {
            validate_identifier(target, "targetId")?;
        } else if !target.is_empty() {
            return Err(OperationsValidationError::UnexpectedTarget);
        }

        let action_offered = input.available_actions.iter().any(|available| {
            available.kind == self.action
                && match available.target_id.as_deref() {
                    Some(allowed) => allowed == target,
                    None => target.is_empty(),
                }
        });
        if !action_offered {
            return Err(OperationsValidationError::ActionNotOffered);
        }

        if self.action == OperationsActionKind::Escalate {
            validate_text(&self.escalation_reason, "escalation reason", 2_000)?;
        } else if !self.escalation_reason.trim().is_empty() {
            return Err(OperationsValidationError::UnexpectedEscalationReason);
        }

        if self.action.is_mutating() {
            if self.verification_signals.is_empty()
                || !(1..=10).contains(&self.required_consecutive_successes)
                || !(1..=3_600).contains(&self.timeout_seconds)
            {
                return Err(OperationsValidationError::InvalidVerification);
            }
        } else if !self.verification_signals.is_empty()
            || self.required_consecutive_successes != 0
            || self.timeout_seconds != 0
        {
            return Err(OperationsValidationError::UnexpectedVerification);
        }

        for selected in &self.verification_signals {
            validate_identifier(&selected.source_id, "selected verification sourceId")?;
            if !input.verification_signals.iter().any(|available| {
                available.kind == selected.kind && available.source_id == selected.source_id
            }) {
                return Err(OperationsValidationError::VerificationNotOffered);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OperationsValidationError {
    #[error("operations input contains an invalid available action")]
    InvalidAvailableAction,
    #[error("operations decision confidence must be between zero and one")]
    InvalidConfidence,
    #[error("operations decision must contain bounded evidence")]
    MissingEvidence,
    #[error("operations output selected an action or target that was not offered")]
    ActionNotOffered,
    #[error("operations output included an unexpected target")]
    UnexpectedTarget,
    #[error("operations output included an unexpected escalation reason")]
    UnexpectedEscalationReason,
    #[error("operations output contains invalid verification")]
    InvalidVerification,
    #[error("operations output selected a verification signal that was not offered")]
    VerificationNotOffered,
    #[error("operations output included verification for a non-mutating action")]
    UnexpectedVerification,
    #[error("invalid identifier in operations data: {0}")]
    InvalidIdentifier(&'static str),
    #[error("invalid or empty text in operations data: {0}")]
    InvalidText(&'static str),
}

pub(crate) fn operations_schema() -> Value {
    let verification_kind = json!({
        "type": "string",
        "enum": [
            "external_http",
            "external_tcp",
            "service_status",
            "container_status",
            "instance_health",
            "error_rate",
            "resource_pressure",
            "traffic_health"
        ]
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "incidentClass": {
                "type": "string",
                "enum": [
                    "service_health",
                    "host_health",
                    "capacity",
                    "traffic_abuse",
                    "edge_security",
                    "dependency",
                    "database",
                    "deployment_regression",
                    "unknown"
                ]
            },
            "summary": { "type": "string" },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "evidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "source": { "type": "string" },
                        "finding": { "type": "string" }
                    },
                    "required": ["source", "finding"]
                }
            },
            "action": {
                "type": "string",
                "enum": [
                    "observe_only",
                    "delegate_deployment_recovery",
                    "restart_service",
                    "restart_container",
                    "restart_instance",
                    "escalate"
                ]
            },
            "targetId": { "type": "string" },
            "escalationReason": { "type": "string" },
            "verificationSignals": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "kind": verification_kind,
                        "sourceId": { "type": "string" }
                    },
                    "required": ["kind", "sourceId"]
                }
            },
            "requiredConsecutiveSuccesses": { "type": "integer", "minimum": 0, "maximum": 10 },
            "timeoutSeconds": { "type": "integer", "minimum": 0, "maximum": 3600 },
            "rollbackOrFallback": { "type": "string" }
        },
        "required": [
            "incidentClass",
            "summary",
            "confidence",
            "evidence",
            "action",
            "targetId",
            "escalationReason",
            "verificationSignals",
            "requiredConsecutiveSuccesses",
            "timeoutSeconds",
            "rollbackOrFallback"
        ]
    })
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), OperationsValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.chars().any(char::is_control)
        || trimmed.contains('\n')
        || trimmed.contains('\r')
    {
        return Err(OperationsValidationError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_text(
    value: &str,
    field: &'static str,
    max_chars: usize,
) -> Result<(), OperationsValidationError> {
    if value.trim().is_empty() || value.chars().count() > max_chars {
        return Err(OperationsValidationError::InvalidText(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> OperationsInput {
        OperationsInput {
            incident_summary: "production health check is down".into(),
            trigger: json!({ "status": 500 }),
            health: json!({ "consecutiveFailures": 3 }),
            infrastructure: json!({ "container": { "running": false } }),
            deployment: json!({ "changedRecently": false }),
            available_actions: vec![
                AvailableOperationsAction::new(
                    OperationsActionKind::RestartContainer,
                    Some("checkout-api".into()),
                    "Restart the configured checkout container",
                ),
                AvailableOperationsAction::new(
                    OperationsActionKind::Escalate,
                    None,
                    "Ask the owner for help without mutating production",
                ),
            ],
            verification_signals: vec![AvailableVerificationSignal::new(
                OperationsVerificationKind::ExternalHttp,
                "primary_https_health",
                "External production health check",
            )],
        }
    }

    fn valid_restart() -> OperationsDecision {
        OperationsDecision {
            incident_class: OperationsIncidentClass::ServiceHealth,
            summary: "configured container is stopped".into(),
            confidence: 0.97,
            evidence: vec![Evidence {
                source: "container_status".into(),
                finding: "container is not running".into(),
            }],
            action: OperationsActionKind::RestartContainer,
            target_id: "checkout-api".into(),
            escalation_reason: String::new(),
            verification_signals: vec![SelectedVerificationSignal {
                kind: OperationsVerificationKind::ExternalHttp,
                source_id: "primary_https_health".into(),
            }],
            required_consecutive_successes: 2,
            timeout_seconds: 60,
            rollback_or_fallback: "escalate after one failed restart".into(),
        }
    }

    #[test]
    fn accepts_only_an_offered_target_and_verification_signal() {
        assert_eq!(valid_restart().validate_against(&input()), Ok(()));
    }

    #[test]
    fn rejects_model_invented_target() {
        let mut decision = valid_restart();
        decision.target_id = "database".into();
        assert_eq!(
            decision.validate_against(&input()),
            Err(OperationsValidationError::ActionNotOffered)
        );
    }

    #[test]
    fn rejects_unoffered_verification_signal() {
        let mut decision = valid_restart();
        decision.verification_signals[0].source_id = "made_up_monitor".into();
        assert_eq!(
            decision.validate_against(&input()),
            Err(OperationsValidationError::VerificationNotOffered)
        );
    }

    #[test]
    fn non_mutating_decision_cannot_smuggle_verification_or_target() {
        let mut decision = valid_restart();
        decision.action = OperationsActionKind::Escalate;
        decision.target_id.clear();
        decision.escalation_reason = "insufficient evidence".into();
        decision.verification_signals.clear();
        decision.required_consecutive_successes = 0;
        decision.timeout_seconds = 0;
        assert_eq!(decision.validate_against(&input()), Ok(()));

        decision.target_id = "checkout-api".into();
        assert_eq!(
            decision.validate_against(&input()),
            Err(OperationsValidationError::UnexpectedTarget)
        );
    }

    #[test]
    fn schema_keeps_actions_and_verification_bounded() {
        let schema = operations_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["action"]["enum"],
            json!([
                "observe_only",
                "delegate_deployment_recovery",
                "restart_service",
                "restart_container",
                "restart_instance",
                "escalate"
            ])
        );
    }

    #[test]
    fn enum_string_values_match_the_strict_schema() {
        assert_eq!(
            OperationsActionKind::RestartContainer.as_str(),
            "restart_container"
        );
        assert_eq!(
            OperationsVerificationKind::ExternalHttp.as_str(),
            "external_http"
        );
    }
}
