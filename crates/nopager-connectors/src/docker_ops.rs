use std::{path::PathBuf, time::Duration};

pub use nopager_runtime_protocol::ContainerState as DockerContainerState;
use nopager_runtime_protocol::{
    MutationDisposition, RuntimeErrorCode, RuntimeRequest, RuntimeResponse, RuntimeResult,
    RuntimeSuccess,
};
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_HELPER_SOCKET: &str = "/run/nopager-runtime/runtime-helper.sock";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerRestartResult {
    pub before: DockerContainerState,
    pub after: DockerContainerState,
    pub duplicate: bool,
}

/// Client for the narrow host runtime-helper protocol.
///
/// This type never invokes Docker, a shell, or an arbitrary command. The ordinary
/// worker can request only typed inspect/restart operations for the exact target
/// independently enrolled in the privileged host helper.
#[derive(Clone)]
pub struct RuntimeHelperClient {
    socket_path: PathBuf,
    credential: SecretString,
    timeout: Duration,
}

impl std::fmt::Debug for RuntimeHelperClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeHelperClient")
            .field("socket_path", &self.socket_path)
            .field("credential", &"[REDACTED]")
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl RuntimeHelperClient {
    pub fn from_environment() -> Result<Self, RuntimeHelperError> {
        let socket_path = std::env::var("NOPAGER_RUNTIME_HELPER_SOCKET")
            .unwrap_or_else(|_| DEFAULT_HELPER_SOCKET.into());
        let credential = std::env::var("NOPAGER_RUNTIME_HELPER_TOKEN")
            .ok()
            .filter(|value| value.len() >= 32)
            .map(SecretString::from)
            .ok_or(RuntimeHelperError::CredentialMissing)?;
        let timeout_seconds = std::env::var("NOPAGER_RUNTIME_HELPER_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, 60);
        Self::new(
            PathBuf::from(socket_path),
            credential,
            Duration::from_secs(timeout_seconds),
        )
    }

    pub fn new(
        socket_path: PathBuf,
        credential: SecretString,
        timeout: Duration,
    ) -> Result<Self, RuntimeHelperError> {
        if !socket_path.is_absolute() {
            return Err(RuntimeHelperError::InvalidSocketPath);
        }
        if credential.expose_secret().len() < 32 {
            return Err(RuntimeHelperError::CredentialMissing);
        }
        if timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err(RuntimeHelperError::InvalidTimeout);
        }
        Ok(Self {
            socket_path,
            credential,
            timeout,
        })
    }

    pub async fn inspect_container(
        &self,
        target_id: &str,
    ) -> Result<DockerContainerState, RuntimeHelperError> {
        validate_target_id(target_id)?;
        let request_id = Uuid::now_v7();
        let request = RuntimeRequest::inspect(
            request_id,
            self.credential.expose_secret().to_owned(),
            target_id.to_owned(),
        );
        let response = self.exchange(request, false).await?;
        match response.result {
            RuntimeResult::Success(RuntimeSuccess::Inspect { state }) => Ok(state),
            RuntimeResult::Rejected { code, mutation, .. } => {
                Err(RuntimeHelperError::Rejected { code, mutation })
            }
            RuntimeResult::Success(_) => Err(RuntimeHelperError::UnexpectedResponse {
                mutation: MutationDisposition::NotStarted,
            }),
        }
    }

    pub async fn restart_container(
        &self,
        request_id: Uuid,
        target_id: &str,
    ) -> Result<DockerRestartResult, RuntimeHelperError> {
        validate_target_id(target_id)?;
        let request = RuntimeRequest::restart(
            request_id,
            self.credential.expose_secret().to_owned(),
            target_id.to_owned(),
        );
        let response = self.exchange(request, true).await?;
        let duplicate = response.duplicate;
        match response.result {
            RuntimeResult::Success(RuntimeSuccess::RestartContainer { before, after }) => {
                Ok(DockerRestartResult {
                    before,
                    after,
                    duplicate,
                })
            }
            RuntimeResult::Rejected { code, mutation, .. } => {
                Err(RuntimeHelperError::Rejected { code, mutation })
            }
            RuntimeResult::Success(_) => Err(RuntimeHelperError::UnexpectedResponse {
                mutation: MutationDisposition::Ambiguous,
            }),
        }
    }

    #[cfg(unix)]
    async fn exchange(
        &self,
        request: RuntimeRequest,
        mutating: bool,
    ) -> Result<RuntimeResponse, RuntimeHelperError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;
        use tokio::time::timeout;

        let request_id = request.request_id;
        let mut encoded =
            serde_json::to_vec(&request).map_err(|_| RuntimeHelperError::InvalidResponse {
                mutation: MutationDisposition::NotStarted,
            })?;
        if encoded.len() + 1 > nopager_runtime_protocol::MAX_MESSAGE_BYTES {
            return Err(RuntimeHelperError::RequestTooLarge);
        }
        encoded.push(b'\n');

        let mut stream = timeout(self.timeout, UnixStream::connect(&self.socket_path))
            .await
            .map_err(|_| RuntimeHelperError::HelperUnavailable)?
            .map_err(|_| RuntimeHelperError::HelperUnavailable)?;

        let sent_disposition = if mutating {
            MutationDisposition::Ambiguous
        } else {
            MutationDisposition::NotStarted
        };
        timeout(self.timeout, stream.write_all(&encoded))
            .await
            .map_err(|_| RuntimeHelperError::ResponseLost {
                mutation: sent_disposition,
            })?
            .map_err(|_| RuntimeHelperError::ResponseLost {
                mutation: sent_disposition,
            })?;
        stream
            .shutdown()
            .await
            .map_err(|_| RuntimeHelperError::ResponseLost {
                mutation: sent_disposition,
            })?;

        let mut response_bytes = Vec::new();
        let mut limited = stream.take((nopager_runtime_protocol::MAX_MESSAGE_BYTES + 1) as u64);
        timeout(self.timeout, limited.read_to_end(&mut response_bytes))
            .await
            .map_err(|_| RuntimeHelperError::ResponseTimeout {
                mutation: if mutating {
                    MutationDisposition::Ambiguous
                } else {
                    MutationDisposition::NotStarted
                },
            })?
            .map_err(|_| RuntimeHelperError::ResponseLost {
                mutation: if mutating {
                    MutationDisposition::Ambiguous
                } else {
                    MutationDisposition::NotStarted
                },
            })?;
        if response_bytes.len() > nopager_runtime_protocol::MAX_MESSAGE_BYTES {
            return Err(RuntimeHelperError::InvalidResponse {
                mutation: sent_disposition,
            });
        }
        let response: RuntimeResponse = serde_json::from_slice(&response_bytes).map_err(|_| {
            RuntimeHelperError::InvalidResponse {
                mutation: sent_disposition,
            }
        })?;
        if response.protocol_version != nopager_runtime_protocol::PROTOCOL_VERSION
            || response.request_id != request_id
        {
            return Err(RuntimeHelperError::InvalidResponse {
                mutation: sent_disposition,
            });
        }
        Ok(response)
    }

    #[cfg(not(unix))]
    async fn exchange(
        &self,
        _request: RuntimeRequest,
        _mutating: bool,
    ) -> Result<RuntimeResponse, RuntimeHelperError> {
        Err(RuntimeHelperError::UnsupportedPlatform)
    }
}

fn validate_target_id(value: &str) -> Result<(), RuntimeHelperError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(RuntimeHelperError::InvalidTarget)
    }
}

#[derive(Debug, Error)]
pub enum RuntimeHelperError {
    #[error("runtime helper target must be an exact 64-character Docker container ID")]
    InvalidTarget,
    #[error("runtime helper socket path must be absolute")]
    InvalidSocketPath,
    #[error("runtime helper credential is missing or too short")]
    CredentialMissing,
    #[error("runtime helper timeout is outside the supported range")]
    InvalidTimeout,
    #[error("runtime helper is unavailable; no mutation request was accepted")]
    HelperUnavailable,
    #[error("runtime helper response timed out")]
    ResponseTimeout { mutation: MutationDisposition },
    #[error("runtime helper response was lost")]
    ResponseLost { mutation: MutationDisposition },
    #[error("runtime helper rejected the request: {code:?}")]
    Rejected {
        code: RuntimeErrorCode,
        mutation: MutationDisposition,
    },
    #[error("runtime helper returned an invalid response")]
    InvalidResponse { mutation: MutationDisposition },
    #[error("runtime helper returned a response for a different operation")]
    UnexpectedResponse { mutation: MutationDisposition },
    #[error("runtime helper request exceeded the protocol size limit")]
    RequestTooLarge,
    #[error("runtime helper IPC is supported only on Unix")]
    UnsupportedPlatform,
}

impl RuntimeHelperError {
    #[must_use]
    pub const fn mutation_disposition(&self) -> MutationDisposition {
        match self {
            Self::ResponseTimeout { mutation }
            | Self::ResponseLost { mutation }
            | Self::InvalidResponse { mutation }
            | Self::UnexpectedResponse { mutation }
            | Self::Rejected { mutation, .. } => *mutation,
            Self::InvalidTarget
            | Self::InvalidSocketPath
            | Self::CredentialMissing
            | Self::InvalidTimeout
            | Self::HelperUnavailable
            | Self::RequestTooLarge
            | Self::UnsupportedPlatform => MutationDisposition::NotStarted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ids_prevent_alias_and_option_substitution() {
        assert!(validate_target_id(&"a".repeat(64)).is_ok());
        for invalid in ["customer-app", "--host=attacker", "abc", "a/b"] {
            assert!(validate_target_id(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn client_has_no_docker_program_or_command_surface() {
        let client = RuntimeHelperClient::new(
            std::env::temp_dir().join("nopager-runtime-helper-test.sock"),
            SecretString::from("a".repeat(32)),
            Duration::from_secs(5),
        )
        .unwrap();
        let debug = format!("{client:?}");
        assert!(!debug.contains("docker_program"));
        assert!(!debug.contains(&"a".repeat(32)));
    }

    #[test]
    fn unavailable_helper_is_definitively_not_started() {
        assert_eq!(
            RuntimeHelperError::HelperUnavailable.mutation_disposition(),
            MutationDisposition::NotStarted
        );
        assert_eq!(
            RuntimeHelperError::ResponseTimeout {
                mutation: MutationDisposition::Ambiguous
            }
            .mutation_disposition(),
            MutationDisposition::Ambiguous
        );
        assert_eq!(
            RuntimeHelperError::InvalidResponse {
                mutation: MutationDisposition::Ambiguous
            }
            .mutation_disposition(),
            MutationDisposition::Ambiguous
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mutating_response_timeout_is_ambiguous() {
        use tokio::io::AsyncReadExt;
        use tokio::net::UnixListener;

        let directory = std::env::temp_dir().join(format!(
            "nopager-runtime-client-timeout-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("helper.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let client = RuntimeHelperClient::new(
            socket,
            SecretString::from("a".repeat(32)),
            Duration::from_millis(50),
        )
        .unwrap();
        let error = client
            .restart_container(Uuid::now_v7(), &"a".repeat(64))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RuntimeHelperError::ResponseTimeout {
                mutation: MutationDisposition::Ambiguous
            }
        ));
        server.await.unwrap();
        let _ = std::fs::remove_dir_all(&directory);
    }
}
