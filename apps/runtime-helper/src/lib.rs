use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use nopager_runtime_protocol::{
    ContainerState, MutationDisposition, PROTOCOL_VERSION, RuntimeAction, RuntimeErrorCode,
    RuntimeRequest, RuntimeResponse, RuntimeSuccess,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::{process::Command, time::timeout};
use uuid::Uuid;

const DOCKER_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_COMMAND_OUTPUT_BYTES: usize = 16 * 1024;
const INSPECT_FORMAT: &str = concat!(
    "{{.Id}}",
    "\t",
    "{{.Name}}",
    "\t",
    "{{.State.Status}}",
    "\t",
    r#"{{index .Config.Labels "com.docker.compose.project"}}"#,
    "\t",
    r#"{{index .Config.Labels "com.docker.compose.service"}}"#,
    "\t",
    r#"{{index .Config.Labels "com.nopager.control-plane"}}"#,
);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelperConfig {
    pub protocol_version: u16,
    pub allowed_peer_uid: u32,
    pub control_plane_compose_project: String,
    pub enrolled_target: EnrolledTarget,
    #[serde(default)]
    pub denied_container_ids: Vec<String>,
}

impl HelperConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ConfigError::ProtocolVersion);
        }
        if self.control_plane_compose_project.trim().is_empty()
            || self.control_plane_compose_project.len() > 128
        {
            return Err(ConfigError::ControlPlaneProject);
        }
        self.enrolled_target.validate()?;
        if self
            .denied_container_ids
            .iter()
            .any(|value| !valid_container_id(value))
        {
            return Err(ConfigError::DeniedContainerId);
        }
        if self
            .denied_container_ids
            .iter()
            .any(|value| value == &self.enrolled_target.id)
        {
            return Err(ConfigError::EnrolledTargetDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrolledTarget {
    pub id: String,
    pub name: String,
    pub compose_project: Option<String>,
}

impl EnrolledTarget {
    fn validate(&self) -> Result<(), ConfigError> {
        if !valid_container_id(&self.id) {
            return Err(ConfigError::TargetId);
        }
        if !valid_container_name(&self.name) {
            return Err(ConfigError::TargetName);
        }
        if self.compose_project.as_ref().is_some_and(|project| {
            project.is_empty() || project.len() > 128 || project.contains(['\n', '\r', '\t'])
        }) {
            return Err(ConfigError::TargetProject);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("helper configuration protocol version is unsupported")]
    ProtocolVersion,
    #[error("control-plane Compose project is missing or invalid")]
    ControlPlaneProject,
    #[error("enrolled target must use an exact 64-character Docker ID")]
    TargetId,
    #[error("enrolled target name is invalid")]
    TargetName,
    #[error("enrolled target Compose project is invalid")]
    TargetProject,
    #[error("denied container list contains an invalid ID")]
    DeniedContainerId,
    #[error("enrolled target is also present in the denied container list")]
    EnrolledTargetDenied,
}

#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    async fn daemon_available(&self) -> Result<(), BackendError>;
    async fn inspect(&self, exact_id_or_name: &str) -> Result<ContainerState, BackendError>;
    async fn restart_exact(&self, exact_id: &str) -> Result<(), BackendError>;
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    #[error("Docker daemon is unavailable")]
    DaemonUnavailable,
    #[error("container was not found")]
    NotFound,
    #[error("fixed Docker operation timed out")]
    Timeout,
    #[error("fixed Docker operation failed")]
    CommandFailed,
    #[error("Docker inspect returned invalid data")]
    InvalidInspectOutput,
    #[error("Docker CLI could not be started")]
    Io,
}

#[derive(Debug, Clone)]
pub struct DockerCliBackend {
    docker_program: PathBuf,
    command_timeout: Duration,
}

impl DockerCliBackend {
    pub fn new(docker_program: PathBuf) -> Result<Self, BackendError> {
        if !docker_program.is_absolute() {
            return Err(BackendError::Io);
        }
        Ok(Self {
            docker_program,
            command_timeout: DOCKER_TIMEOUT,
        })
    }

    async fn run(&self, arguments: &[&str]) -> Result<String, BackendError> {
        let mut command = Command::new(&self.docker_program);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = timeout(self.command_timeout, command.output())
            .await
            .map_err(|_| BackendError::Timeout)?
            .map_err(|_| BackendError::Io)?;
        if !output.status.success() {
            return Err(BackendError::CommandFailed);
        }
        Ok(bounded_utf8(&output.stdout))
    }
}

#[async_trait]
impl RuntimeBackend for DockerCliBackend {
    async fn daemon_available(&self) -> Result<(), BackendError> {
        self.run(&["version", "--format", "{{.Server.Version}}"])
            .await
            .map(|_| ())
            .map_err(|error| match error {
                BackendError::Timeout => BackendError::Timeout,
                _ => BackendError::DaemonUnavailable,
            })
    }

    async fn inspect(&self, exact_id_or_name: &str) -> Result<ContainerState, BackendError> {
        let output = self
            .run(&[
                "container",
                "inspect",
                "--format",
                INSPECT_FORMAT,
                "--",
                exact_id_or_name,
            ])
            .await
            .map_err(|error| match error {
                BackendError::CommandFailed => BackendError::NotFound,
                other => other,
            })?;
        parse_inspect_output(&output)
    }

    async fn restart_exact(&self, exact_id: &str) -> Result<(), BackendError> {
        if !valid_container_id(exact_id) {
            return Err(BackendError::CommandFailed);
        }
        self.run(&["container", "restart", "--time", "10", "--", exact_id])
            .await
            .map(|_| ())
    }
}

pub struct RuntimeExecutor {
    config: HelperConfig,
    credential_digest: [u8; 32],
    journal: RequestJournal,
    backend: Arc<dyn RuntimeBackend>,
}

impl RuntimeExecutor {
    pub fn new(
        config: HelperConfig,
        credential: &SecretString,
        state_directory: PathBuf,
        backend: Arc<dyn RuntimeBackend>,
    ) -> Result<Self, ExecutorError> {
        config.validate()?;
        if credential.expose_secret().len() < 32 {
            return Err(ExecutorError::CredentialTooShort);
        }
        let credential_digest: [u8; 32] =
            Sha256::digest(credential.expose_secret().as_bytes()).into();
        Ok(Self {
            config,
            credential_digest,
            journal: RequestJournal::new(state_directory)?,
            backend,
        })
    }

    pub async fn validate_startup(&self) -> Result<ContainerState, ExecutorError> {
        self.inspect_enrolled()
            .await
            .map_err(|rejection| ExecutorError::UnsafeEnrollment(rejection.code))
    }

    pub async fn handle(&self, peer_uid: u32, request: RuntimeRequest) -> RuntimeResponse {
        let request_id = request.request_id;
        if request.protocol_version != PROTOCOL_VERSION {
            return rejection(
                request_id,
                RuntimeErrorCode::ProtocolVersionUnsupported,
                MutationDisposition::NotStarted,
                "unsupported protocol version",
            );
        }
        if peer_uid != self.config.allowed_peer_uid {
            return rejection(
                request_id,
                RuntimeErrorCode::PeerNotAllowed,
                MutationDisposition::NotStarted,
                "Unix peer uid is not enrolled",
            );
        }
        let supplied: [u8; 32] = Sha256::digest(request.credential.as_bytes()).into();
        if !bool::from(supplied.ct_eq(&self.credential_digest)) {
            return rejection(
                request_id,
                RuntimeErrorCode::AuthenticationFailed,
                MutationDisposition::NotStarted,
                "helper credential rejected",
            );
        }
        if request.action.target_id() != self.config.enrolled_target.id {
            return rejection(
                request_id,
                RuntimeErrorCode::TargetNotEnrolled,
                MutationDisposition::NotStarted,
                "target is not explicitly enrolled",
            );
        }

        match request.action {
            RuntimeAction::Inspect { .. } => match self.inspect_enrolled().await {
                Ok(state) => {
                    RuntimeResponse::success(request_id, RuntimeSuccess::Inspect { state })
                }
                Err(rejected) => rejected.response(request_id),
            },
            RuntimeAction::RestartContainer { target_id } => {
                self.restart(request_id, &target_id).await
            }
        }
    }

    async fn restart(&self, request_id: Uuid, target_id: &str) -> RuntimeResponse {
        match self.journal.claim(request_id, target_id) {
            Ok(ClaimResult::Completed(mut response)) => {
                response.duplicate = true;
                return *response;
            }
            Ok(ClaimResult::Started) => {
                return rejection(
                    request_id,
                    RuntimeErrorCode::DuplicateAmbiguous,
                    MutationDisposition::Ambiguous,
                    "request was previously claimed without a durable completion record",
                );
            }
            Ok(ClaimResult::New) => {}
            Err(_) => {
                return rejection(
                    request_id,
                    RuntimeErrorCode::InternalError,
                    MutationDisposition::NotStarted,
                    "helper could not durably claim the mutation",
                );
            }
        }

        let before = match self.inspect_enrolled().await {
            Ok(state) => state,
            Err(rejected) => {
                let response = rejected.response(request_id);
                let _ = self.journal.complete(request_id, target_id, &response);
                return response;
            }
        };

        if self.backend.restart_exact(target_id).await.is_err() {
            return rejection(
                request_id,
                RuntimeErrorCode::RestartAmbiguous,
                MutationDisposition::Ambiguous,
                "Docker restart outcome is ambiguous; this request will never be replayed",
            );
        }

        let after = match self.inspect_enrolled().await {
            Ok(state) => state,
            Err(_) => {
                return rejection(
                    request_id,
                    RuntimeErrorCode::RestartAmbiguous,
                    MutationDisposition::Ambiguous,
                    "target state could not be proven after restart",
                );
            }
        };
        if !after.running() {
            return rejection(
                request_id,
                RuntimeErrorCode::RestartDidNotStart,
                MutationDisposition::Ambiguous,
                "restart returned but the enrolled target is not running",
            );
        }

        let response = RuntimeResponse::success(
            request_id,
            RuntimeSuccess::RestartContainer { before, after },
        );
        if self
            .journal
            .complete(request_id, target_id, &response)
            .is_err()
        {
            return rejection(
                request_id,
                RuntimeErrorCode::InternalError,
                MutationDisposition::Ambiguous,
                "restart completed but helper could not persist the result",
            );
        }
        response
    }

    async fn inspect_enrolled(&self) -> Result<ContainerState, Rejection> {
        self.backend.daemon_available().await.map_err(|_| {
            Rejection::not_started(
                RuntimeErrorCode::DockerDaemonUnavailable,
                "Docker daemon is unavailable",
            )
        })?;

        let target = self
            .backend
            .inspect(&self.config.enrolled_target.id)
            .await
            .map_err(|error| backend_inspect_rejection(error, "enrolled target disappeared"))?;
        let named = self
            .backend
            .inspect(&self.config.enrolled_target.name)
            .await
            .map_err(|error| {
                backend_inspect_rejection(error, "enrolled target name disappeared")
            })?;

        if target.id != self.config.enrolled_target.id
            || named.id != self.config.enrolled_target.id
            || target.name != self.config.enrolled_target.name
        {
            return Err(Rejection::not_started(
                RuntimeErrorCode::TargetReplaced,
                "target name or immutable container identity changed after enrollment",
            ));
        }
        if target.nopager_control_plane
            || self
                .config
                .denied_container_ids
                .iter()
                .any(|id| id == &target.id)
        {
            return Err(Rejection::not_started(
                RuntimeErrorCode::ControlPlaneTarget,
                "NoPager control-plane resources cannot be targeted",
            ));
        }
        if target.compose_project.as_deref()
            == Some(self.config.control_plane_compose_project.as_str())
        {
            return Err(Rejection::not_started(
                RuntimeErrorCode::SameControlPlaneProject,
                "same-control-plane Compose resources cannot be targeted",
            ));
        }
        if target.compose_project != self.config.enrolled_target.compose_project {
            return Err(Rejection::not_started(
                RuntimeErrorCode::TargetReplaced,
                "target Compose identity differs from enrollment",
            ));
        }
        Ok(target)
    }
}

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("helper credential must contain at least 32 characters")]
    CredentialTooShort,
    #[error("helper state directory could not be initialized: {0}")]
    Journal(#[from] std::io::Error),
    #[error("runtime enrollment failed closed: {0:?}")]
    UnsafeEnrollment(RuntimeErrorCode),
}

#[derive(Debug, Clone, Copy)]
struct Rejection {
    code: RuntimeErrorCode,
    message: &'static str,
}

impl Rejection {
    const fn not_started(code: RuntimeErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }

    fn response(self, request_id: Uuid) -> RuntimeResponse {
        rejection(
            request_id,
            self.code,
            MutationDisposition::NotStarted,
            self.message,
        )
    }
}

fn backend_inspect_rejection(error: BackendError, missing_message: &'static str) -> Rejection {
    match error {
        BackendError::NotFound => {
            Rejection::not_started(RuntimeErrorCode::TargetDisappeared, missing_message)
        }
        BackendError::DaemonUnavailable | BackendError::Timeout | BackendError::Io => {
            Rejection::not_started(
                RuntimeErrorCode::DockerDaemonUnavailable,
                "Docker daemon is unavailable",
            )
        }
        BackendError::CommandFailed | BackendError::InvalidInspectOutput => Rejection::not_started(
            RuntimeErrorCode::InternalError,
            "Docker returned an invalid bounded inspection result",
        ),
    }
}

fn rejection(
    request_id: Uuid,
    code: RuntimeErrorCode,
    mutation: MutationDisposition,
    message: impl Into<String>,
) -> RuntimeResponse {
    RuntimeResponse::rejection(request_id, code, mutation, message)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalRecord {
    request_id: Uuid,
    target_id: String,
    status: JournalStatus,
    response: Option<RuntimeResponse>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum JournalStatus {
    Started,
    Completed,
}

struct RequestJournal {
    directory: PathBuf,
}

impl RequestJournal {
    fn new(directory: PathBuf) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&directory)?;
        Ok(Self { directory })
    }

    fn claim(&self, request_id: Uuid, target_id: &str) -> Result<ClaimResult, std::io::Error> {
        let path = self.path(request_id);
        let record = JournalRecord {
            request_id,
            target_id: target_id.to_owned(),
            status: JournalStatus::Started,
            response: None,
        };
        let encoded = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&encoded)?;
                file.sync_all()?;
                sync_directory(&self.directory)?;
                Ok(ClaimResult::New)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing: JournalRecord =
                    serde_json::from_slice(&fs::read(path)?).map_err(std::io::Error::other)?;
                if existing.request_id != request_id || existing.target_id != target_id {
                    return Ok(ClaimResult::Started);
                }
                match (existing.status, existing.response) {
                    (JournalStatus::Completed, Some(response)) => {
                        Ok(ClaimResult::Completed(Box::new(response)))
                    }
                    _ => Ok(ClaimResult::Started),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn complete(
        &self,
        request_id: Uuid,
        target_id: &str,
        response: &RuntimeResponse,
    ) -> Result<(), std::io::Error> {
        let record = JournalRecord {
            request_id,
            target_id: target_id.to_owned(),
            status: JournalStatus::Completed,
            response: Some(response.clone()),
        };
        let encoded = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        let temporary = self.directory.join(format!(".{request_id}.tmp"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        fs::rename(temporary, self.path(request_id))?;
        sync_directory(&self.directory)
    }

    fn path(&self, request_id: Uuid) -> PathBuf {
        self.directory.join(format!("{request_id}.json"))
    }
}

enum ClaimResult {
    New,
    Started,
    Completed(Box<RuntimeResponse>),
}

#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &std::path::Path) -> Result<(), std::io::Error> {
    // The production helper is Linux-only. Windows cannot fsync a directory,
    // but keeping this no-op lets the deterministic executor tests run there.
    Ok(())
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_container_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn parse_inspect_output(value: &str) -> Result<ContainerState, BackendError> {
    let line = value.lines().next().unwrap_or_default().trim();
    let parts = line.split('\t').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err(BackendError::InvalidInspectOutput);
    }
    let id = parts[0].trim();
    let name = parts[1].trim().trim_start_matches('/');
    let status = parts[2].trim();
    if !valid_container_id(id) || !valid_container_name(name) || status.is_empty() {
        return Err(BackendError::InvalidInspectOutput);
    }
    Ok(ContainerState {
        id: id.to_owned(),
        name: name.to_owned(),
        status: status.to_owned(),
        compose_project: optional_template_value(parts[3]),
        compose_service: optional_template_value(parts[4]),
        nopager_control_plane: parts[5].trim().eq_ignore_ascii_case("true"),
    })
}

fn optional_template_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "<no value>").then(|| value.to_owned())
}

fn bounded_utf8(value: &[u8]) -> String {
    let end = value.len().min(MAX_COMMAND_OUTPUT_BYTES);
    String::from_utf8_lossy(&value[..end]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use nopager_runtime_protocol::{RuntimeResult, RuntimeSuccess};

    use super::*;

    const TARGET_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct FakeBackend {
        states: Mutex<HashMap<String, ContainerState>>,
        restart_count: Mutex<usize>,
        daemon_available: bool,
        restart_error: Option<BackendError>,
    }

    impl FakeBackend {
        fn healthy() -> Self {
            let target = state(TARGET_ID, "customer-app", Some("customer"), false);
            let mut states = HashMap::new();
            states.insert(TARGET_ID.into(), target.clone());
            states.insert("customer-app".into(), target);
            Self {
                states: Mutex::new(states),
                restart_count: Mutex::new(0),
                daemon_available: true,
                restart_error: None,
            }
        }
    }

    #[async_trait]
    impl RuntimeBackend for FakeBackend {
        async fn daemon_available(&self) -> Result<(), BackendError> {
            self.daemon_available
                .then_some(())
                .ok_or(BackendError::DaemonUnavailable)
        }

        async fn inspect(&self, target: &str) -> Result<ContainerState, BackendError> {
            self.states
                .lock()
                .unwrap()
                .get(target)
                .cloned()
                .ok_or(BackendError::NotFound)
        }

        async fn restart_exact(&self, _exact_id: &str) -> Result<(), BackendError> {
            *self.restart_count.lock().unwrap() += 1;
            self.restart_error.map_or(Ok(()), Err)
        }
    }

    fn state(id: &str, name: &str, project: Option<&str>, control_plane: bool) -> ContainerState {
        ContainerState {
            id: id.into(),
            name: name.into(),
            status: "running".into(),
            compose_project: project.map(ToOwned::to_owned),
            compose_service: Some("web".into()),
            nopager_control_plane: control_plane,
        }
    }

    fn config() -> HelperConfig {
        HelperConfig {
            protocol_version: PROTOCOL_VERSION,
            allowed_peer_uid: 10001,
            control_plane_compose_project: "nopager".into(),
            enrolled_target: EnrolledTarget {
                id: TARGET_ID.into(),
                name: "customer-app".into(),
                compose_project: Some("customer".into()),
            },
            denied_container_ids: vec![OTHER_ID.into()],
        }
    }

    fn executor(backend: Arc<FakeBackend>, suffix: &str) -> RuntimeExecutor {
        let directory = std::env::temp_dir().join(format!(
            "nopager-runtime-helper-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        RuntimeExecutor::new(
            config(),
            &SecretString::from("correct-credential-with-at-least-32-bytes".to_owned()),
            directory,
            backend,
        )
        .unwrap()
    }

    fn restart_request(id: Uuid) -> RuntimeRequest {
        RuntimeRequest::restart(
            id,
            "correct-credential-with-at-least-32-bytes".into(),
            TARGET_ID.into(),
        )
    }

    fn rejected_code(response: &RuntimeResponse) -> Option<RuntimeErrorCode> {
        match response.result {
            RuntimeResult::Rejected { code, .. } => Some(code),
            RuntimeResult::Success(_) => None,
        }
    }

    #[tokio::test]
    async fn exact_enrollment_and_peer_and_credential_are_independent_gates() {
        let backend = Arc::new(FakeBackend::healthy());
        let executor = executor(backend, "gates");
        let mut request = restart_request(Uuid::now_v7());
        request.action = RuntimeAction::RestartContainer {
            target_id: OTHER_ID.into(),
        };
        assert_eq!(
            rejected_code(&executor.handle(10001, request).await),
            Some(RuntimeErrorCode::TargetNotEnrolled)
        );
        assert_eq!(
            rejected_code(&executor.handle(999, restart_request(Uuid::now_v7())).await),
            Some(RuntimeErrorCode::PeerNotAllowed)
        );
        let mut request = restart_request(Uuid::now_v7());
        request.credential = "wrong-credential-but-still-long-enough".into();
        assert_eq!(
            rejected_code(&executor.handle(10001, request).await),
            Some(RuntimeErrorCode::AuthenticationFailed)
        );
    }

    #[tokio::test]
    async fn duplicate_completed_restart_replays_result_without_second_mutation() {
        let backend = Arc::new(FakeBackend::healthy());
        let executor = executor(backend.clone(), "duplicate");
        let request_id = Uuid::now_v7();
        let first = executor.handle(10001, restart_request(request_id)).await;
        let second = executor.handle(10001, restart_request(request_id)).await;
        assert!(matches!(
            first.result,
            RuntimeResult::Success(RuntimeSuccess::RestartContainer { .. })
        ));
        assert!(second.duplicate);
        assert_eq!(*backend.restart_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn ambiguous_restart_is_never_retried() {
        let mut fake = FakeBackend::healthy();
        fake.restart_error = Some(BackendError::CommandFailed);
        let backend = Arc::new(fake);
        let executor = executor(backend.clone(), "ambiguous");
        let request_id = Uuid::now_v7();
        let first = executor.handle(10001, restart_request(request_id)).await;
        let second = executor.handle(10001, restart_request(request_id)).await;
        assert_eq!(
            rejected_code(&first),
            Some(RuntimeErrorCode::RestartAmbiguous)
        );
        assert_eq!(
            rejected_code(&second),
            Some(RuntimeErrorCode::DuplicateAmbiguous)
        );
        assert_eq!(*backend.restart_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn restart_timeout_is_ambiguous_and_never_replayed() {
        let mut fake = FakeBackend::healthy();
        fake.restart_error = Some(BackendError::Timeout);
        let backend = Arc::new(fake);
        let executor = executor(backend.clone(), "restart-timeout");
        let request_id = Uuid::now_v7();
        let first = executor.handle(10001, restart_request(request_id)).await;
        let replay = executor.handle(10001, restart_request(request_id)).await;
        assert_eq!(
            rejected_code(&first),
            Some(RuntimeErrorCode::RestartAmbiguous)
        );
        assert_eq!(
            rejected_code(&replay),
            Some(RuntimeErrorCode::DuplicateAmbiguous)
        );
        assert_eq!(*backend.restart_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn control_plane_and_same_compose_project_are_blocked() {
        for (label, project, expected) in [
            (true, Some("customer"), RuntimeErrorCode::ControlPlaneTarget),
            (
                false,
                Some("nopager"),
                RuntimeErrorCode::SameControlPlaneProject,
            ),
        ] {
            let backend = Arc::new(FakeBackend::healthy());
            let replacement = state(TARGET_ID, "customer-app", project, label);
            backend
                .states
                .lock()
                .unwrap()
                .insert(TARGET_ID.into(), replacement.clone());
            backend
                .states
                .lock()
                .unwrap()
                .insert("customer-app".into(), replacement);
            let mut cfg = config();
            cfg.enrolled_target.compose_project = project.map(ToOwned::to_owned);
            let directory = std::env::temp_dir().join(format!(
                "nopager-runtime-helper-{}-control-{label}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&directory);
            let executor = RuntimeExecutor::new(
                cfg,
                &SecretString::from("correct-credential-with-at-least-32-bytes".to_owned()),
                directory,
                backend,
            )
            .unwrap();
            assert_eq!(
                rejected_code(
                    &executor
                        .handle(10001, restart_request(Uuid::now_v7()))
                        .await
                ),
                Some(expected)
            );
        }
    }

    #[tokio::test]
    async fn disappeared_replaced_and_daemon_unavailable_fail_closed() {
        let backend = Arc::new(FakeBackend::healthy());
        backend.states.lock().unwrap().remove(TARGET_ID);
        let disappeared_executor = executor(backend, "disappeared");
        assert_eq!(
            rejected_code(
                &disappeared_executor
                    .handle(10001, restart_request(Uuid::now_v7()))
                    .await
            ),
            Some(RuntimeErrorCode::TargetDisappeared)
        );

        let backend = Arc::new(FakeBackend::healthy());
        backend.states.lock().unwrap().insert(
            "customer-app".into(),
            state(OTHER_ID, "customer-app", Some("customer"), false),
        );
        let replaced_executor = executor(backend, "replaced");
        assert_eq!(
            rejected_code(
                &replaced_executor
                    .handle(10001, restart_request(Uuid::now_v7()))
                    .await
            ),
            Some(RuntimeErrorCode::TargetReplaced)
        );

        let mut fake = FakeBackend::healthy();
        fake.daemon_available = false;
        let daemon_executor = executor(Arc::new(fake), "daemon");
        assert_eq!(
            rejected_code(
                &daemon_executor
                    .handle(10001, restart_request(Uuid::now_v7()))
                    .await
            ),
            Some(RuntimeErrorCode::DockerDaemonUnavailable)
        );
    }

    #[test]
    fn inspect_parser_exposes_only_bounded_state_fields() {
        assert_eq!(INSPECT_FORMAT.matches('\t').count(), 5);
        assert!(!INSPECT_FORMAT.contains(r"\t"));
        let output = format!("{TARGET_ID}\t/customer-app\trunning\tcustomer\tweb\t<no value>\n");
        let state = parse_inspect_output(&output).unwrap();
        assert_eq!(state.id, TARGET_ID);
        assert_eq!(state.name, "customer-app");
        assert!(!state.nopager_control_plane);
    }
}
