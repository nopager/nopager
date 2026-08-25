use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRequest {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub credential: String,
    pub action: RuntimeAction,
}

impl RuntimeRequest {
    #[must_use]
    pub fn inspect(request_id: Uuid, credential: String, target_id: String) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            credential,
            action: RuntimeAction::Inspect { target_id },
        }
    }

    #[must_use]
    pub fn restart(request_id: Uuid, credential: String, target_id: String) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            credential,
            action: RuntimeAction::RestartContainer { target_id },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeAction {
    Inspect { target_id: String },
    RestartContainer { target_id: String },
}

impl RuntimeAction {
    #[must_use]
    pub const fn mutating(&self) -> bool {
        matches!(self, Self::RestartContainer { .. })
    }

    #[must_use]
    pub fn target_id(&self) -> &str {
        match self {
            Self::Inspect { target_id } | Self::RestartContainer { target_id } => target_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeResponse {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub duplicate: bool,
    pub result: RuntimeResult,
}

impl RuntimeResponse {
    #[must_use]
    pub fn success(request_id: Uuid, result: RuntimeSuccess) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            duplicate: false,
            result: RuntimeResult::Success(result),
        }
    }

    #[must_use]
    pub fn rejection(
        request_id: Uuid,
        code: RuntimeErrorCode,
        mutation: MutationDisposition,
        message: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            duplicate: false,
            result: RuntimeResult::Rejected {
                code,
                mutation,
                message: message.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
// Messages are capped at 16 KiB; keeping the bounded result inline makes the
// audited wire representation simpler than adding heap indirection here.
#[allow(clippy::large_enum_variant)]
pub enum RuntimeResult {
    Success(RuntimeSuccess),
    Rejected {
        code: RuntimeErrorCode,
        mutation: MutationDisposition,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSuccess {
    Inspect {
        state: ContainerState,
    },
    RestartContainer {
        before: ContainerState,
        after: ContainerState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContainerState {
    pub id: String,
    pub name: String,
    pub status: String,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    pub nopager_control_plane: bool,
}

impl ContainerState {
    #[must_use]
    pub fn running(&self) -> bool {
        self.status == "running"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationDisposition {
    NotStarted,
    Completed,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCode {
    AuthenticationFailed,
    PeerNotAllowed,
    ProtocolVersionUnsupported,
    InvalidRequest,
    TargetNotEnrolled,
    TargetDisappeared,
    TargetReplaced,
    ControlPlaneTarget,
    SameControlPlaneProject,
    DockerDaemonUnavailable,
    RestartAmbiguous,
    RestartDidNotStart,
    DuplicateAmbiguous,
    InternalError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_has_no_generic_command_or_docker_request_variant() {
        let forbidden = [
            r#"{"protocolVersion":1,"requestId":"018f0000-0000-7000-8000-000000000000","credential":"x","action":{"type":"exec","command":"id"}}"#,
            r#"{"protocolVersion":1,"requestId":"018f0000-0000-7000-8000-000000000000","credential":"x","action":{"type":"create","image":"alpine"}}"#,
            r#"{"protocolVersion":1,"requestId":"018f0000-0000-7000-8000-000000000000","credential":"x","action":{"type":"docker_request","path":"/containers/json"}}"#,
            r#"{"protocolVersion":1,"requestId":"018f0000-0000-7000-8000-000000000000","credential":"x","action":{"type":"restart_container","target_id":"a","shell":"sh"}}"#,
        ];
        for value in forbidden {
            assert!(
                serde_json::from_str::<RuntimeRequest>(value).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn only_typed_inspect_and_restart_round_trip() {
        let request = RuntimeRequest::restart(Uuid::nil(), "credential".into(), "a".repeat(64));
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeRequest>(&encoded).unwrap(),
            request
        );
    }
}
