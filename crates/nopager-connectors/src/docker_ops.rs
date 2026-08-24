use std::{process::Stdio, time::Duration};

use serde::Serialize;
use thiserror::Error;
use tokio::{process::Command, time::timeout};

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COMMAND_OUTPUT_BYTES: usize = 16 * 1024;
const INSPECT_FORMAT: &str = r#"{{.Id}}\t{{.Name}}\t{{.State.Status}}\t{{index .Config.Labels "com.docker.compose.project"}}\t{{index .Config.Labels "com.docker.compose.service"}}\t{{index .Config.Labels "com.nopager.control-plane"}}"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerContainerState {
    pub id: String,
    pub name: String,
    pub status: String,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    pub nopager_control_plane: bool,
}

impl DockerContainerState {
    #[must_use]
    pub fn running(&self) -> bool {
        self.status == "running"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerRestartResult {
    pub before: DockerContainerState,
    pub after: DockerContainerState,
}

/// Executes a deliberately tiny Docker operations surface.
///
/// The model never supplies a command. Trusted NoPager code passes a validated,
/// configured container identity to these fixed Docker CLI operations.
#[derive(Debug, Clone)]
pub struct DockerOperationsClient {
    docker_program: String,
    command_timeout: Duration,
    self_container_hint: Option<String>,
}

impl Default for DockerOperationsClient {
    fn default() -> Self {
        Self::from_environment()
    }
}

impl DockerOperationsClient {
    #[must_use]
    pub fn from_environment() -> Self {
        let explicit = std::env::var("NOPAGER_DOCKER_SELF_CONTAINER")
            .ok()
            .filter(|value| valid_container_target(value));
        let hostname = std::env::var("HOSTNAME")
            .ok()
            .filter(|value| looks_like_container_id(value));
        Self {
            docker_program: "docker".into(),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            self_container_hint: explicit.or(hostname),
        }
    }

    #[cfg(test)]
    fn for_test(self_container_hint: Option<&str>) -> Self {
        Self {
            docker_program: "docker".into(),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            self_container_hint: self_container_hint.map(ToOwned::to_owned),
        }
    }

    /// Read a bounded, non-secret container status snapshot.
    pub async fn inspect_container(
        &self,
        target: &str,
    ) -> Result<DockerContainerState, DockerOperationsError> {
        validate_container_target(target)?;
        let output = self
            .run(&[
                "container",
                "inspect",
                "--format",
                INSPECT_FORMAT,
                "--",
                target,
            ])
            .await?;
        parse_inspect_output(&output.stdout)
    }

    /// Restart exactly one configured container.
    ///
    /// This method checks the target before mutation and refuses to restart a
    /// NoPager control-plane container or any container in the same Compose
    /// project as the current NoPager worker. Independent application health
    /// verification must still happen after this connector returns success.
    pub async fn restart_container(
        &self,
        target: &str,
    ) -> Result<DockerRestartResult, DockerOperationsError> {
        validate_container_target(target)?;
        let before = self.inspect_container(target).await?;
        let self_state = self.inspect_self_if_available().await;
        ensure_safe_mutation_target(&before, self_state.as_ref())?;

        self.run(&["container", "restart", "--time", "10", "--", target])
            .await?;

        let after = self.inspect_container(target).await?;
        if !after.running() {
            return Err(DockerOperationsError::RestartDidNotStart);
        }
        Ok(DockerRestartResult { before, after })
    }

    async fn inspect_self_if_available(&self) -> Option<DockerContainerState> {
        let target = self.self_container_hint.as_deref()?;
        self.inspect_container(target).await.ok()
    }

    async fn run(&self, args: &[&str]) -> Result<CommandOutput, DockerOperationsError> {
        let mut command = Command::new(&self.docker_program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output = timeout(self.command_timeout, command.output())
            .await
            .map_err(|_| DockerOperationsError::Timeout)??;
        let stdout = bounded_utf8(&output.stdout);
        let stderr = bounded_utf8(&output.stderr);
        if !output.status.success() {
            return Err(DockerOperationsError::CommandFailed {
                code: output.status.code(),
                message: stderr,
            });
        }
        Ok(CommandOutput { stdout })
    }
}

#[derive(Debug)]
struct CommandOutput {
    stdout: String,
}

fn ensure_safe_mutation_target(
    target: &DockerContainerState,
    self_state: Option<&DockerContainerState>,
) -> Result<(), DockerOperationsError> {
    if target.nopager_control_plane {
        return Err(DockerOperationsError::ControlPlaneTarget);
    }

    let Some(self_state) = self_state else {
        return Ok(());
    };
    if target.id == self_state.id {
        return Err(DockerOperationsError::ControlPlaneTarget);
    }
    if let (Some(target_project), Some(self_project)) = (
        target.compose_project.as_deref(),
        self_state.compose_project.as_deref(),
    ) && !target_project.is_empty()
        && target_project == self_project
    {
        return Err(DockerOperationsError::ControlPlaneTarget);
    }
    Ok(())
}

fn parse_inspect_output(value: &str) -> Result<DockerContainerState, DockerOperationsError> {
    let line = value.lines().next().unwrap_or_default().trim();
    let parts = line.split('\t').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err(DockerOperationsError::InvalidInspectOutput);
    }
    let id = parts[0].trim();
    let name = parts[1].trim().trim_start_matches('/');
    let status = parts[2].trim();
    if id.is_empty() || name.is_empty() || status.is_empty() {
        return Err(DockerOperationsError::InvalidInspectOutput);
    }

    Ok(DockerContainerState {
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

fn validate_container_target(value: &str) -> Result<(), DockerOperationsError> {
    if valid_container_target(value) {
        Ok(())
    } else {
        Err(DockerOperationsError::InvalidTarget)
    }
}

fn valid_container_target(value: &str) -> bool {
    let value = value.trim();
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

fn looks_like_container_id(value: &str) -> bool {
    (12..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded_utf8(value: &[u8]) -> String {
    let end = value.len().min(MAX_COMMAND_OUTPUT_BYTES);
    String::from_utf8_lossy(&value[..end]).trim().to_owned()
}

#[derive(Debug, Error)]
pub enum DockerOperationsError {
    #[error("Docker operations target is invalid")]
    InvalidTarget,
    #[error("Docker CLI operation timed out")]
    Timeout,
    #[error("Docker CLI could not be started: {0}")]
    Io(#[from] std::io::Error),
    #[error("Docker CLI operation failed with code {code:?}: {message}")]
    CommandFailed { code: Option<i32>, message: String },
    #[error("Docker inspect returned an invalid bounded status response")]
    InvalidInspectOutput,
    #[error("refusing to mutate a NoPager control-plane container")]
    ControlPlaneTarget,
    #[error("Docker restart returned success but the container is not running")]
    RestartDidNotStart,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: &str, project: Option<&str>, control_plane: bool) -> DockerContainerState {
        DockerContainerState {
            id: id.into(),
            name: id.into(),
            status: "running".into(),
            compose_project: project.map(ToOwned::to_owned),
            compose_service: None,
            nopager_control_plane: control_plane,
        }
    }

    #[test]
    fn target_validation_prevents_cli_option_injection() {
        for invalid in [
            "",
            "--host=attacker",
            "web;rm -rf /",
            "web container",
            "../web",
            "/web",
        ] {
            assert_eq!(
                validate_container_target(invalid).unwrap_err().to_string(),
                DockerOperationsError::InvalidTarget.to_string()
            );
        }
        assert!(validate_container_target("checkout-api_1").is_ok());
        assert!(validate_container_target("5f2a06e3f45b").is_ok());
    }

    #[test]
    fn inspect_parser_returns_only_bounded_operational_fields() {
        let value = "abc123\t/checkout-api\trunning\tdemo\tweb\t<no value>\n";
        let parsed = parse_inspect_output(value).unwrap();
        assert_eq!(parsed.name, "checkout-api");
        assert_eq!(parsed.compose_project.as_deref(), Some("demo"));
        assert_eq!(parsed.compose_service.as_deref(), Some("web"));
        assert!(!parsed.nopager_control_plane);
    }

    #[test]
    fn blocks_explicit_control_plane_label() {
        let target = state("target", Some("customer"), true);
        assert!(matches!(
            ensure_safe_mutation_target(&target, None),
            Err(DockerOperationsError::ControlPlaneTarget)
        ));
    }

    #[test]
    fn blocks_same_compose_project_as_worker() {
        let target = state("customer-app", Some("nopager"), false);
        let worker = state("worker", Some("nopager"), true);
        assert!(matches!(
            ensure_safe_mutation_target(&target, Some(&worker)),
            Err(DockerOperationsError::ControlPlaneTarget)
        ));
    }

    #[test]
    fn allows_separate_customer_compose_project() {
        let target = state("customer-app", Some("customer"), false);
        let worker = state("worker", Some("nopager"), true);
        assert!(ensure_safe_mutation_target(&target, Some(&worker)).is_ok());
    }

    #[test]
    fn default_client_has_no_shell_or_model_command_surface() {
        let client = DockerOperationsClient::for_test(Some("5f2a06e3f45b"));
        assert_eq!(client.docker_program, "docker");
        assert_eq!(client.self_container_hint.as_deref(), Some("5f2a06e3f45b"));
    }
}
