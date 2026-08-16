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
}
