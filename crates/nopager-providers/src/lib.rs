use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

mod http;
pub use http::{AnthropicProvider, GeminiProvider, OpenAiProvider};

pub const VERIFIED_GITHUB_DIFF_SOURCE: &str = "verified_github_diff";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisInput {
    pub incident_summary: String,
    pub recent_commits: Vec<CommitContext>,
    pub stack_trace: Option<String>,
    pub deployment: Value,
    pub health_failure: Value,
    pub relevant_files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitContext {
    pub sha: String,
    pub message: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosisResult {
    pub suspected_root_cause: String,
    pub evidence: Vec<Evidence>,
    pub confidence: f32,
    pub affected_files: Vec<String>,
    pub proposed_actions: Vec<String>,
    pub risk_level: RiskLevel,
    pub validation_plan: Vec<String>,
    pub rollback_plan: String,
}

impl DiagnosisResult {
    pub fn validate(&self) -> Result<(), OutputValidationError> {
        if self.suspected_root_cause.trim().is_empty() {
            return Err(OutputValidationError::MissingRootCause);
        }
        if self.evidence.is_empty() {
            return Err(OutputValidationError::MissingEvidence);
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(OutputValidationError::InvalidConfidence);
        }
        for path in &self.affected_files {
            validate_relative_path(path)?;
        }
        if self.validation_plan.is_empty() || self.rollback_plan.trim().is_empty() {
            return Err(OutputValidationError::MissingSafetyPlan);
        }
        Ok(())
    }

    #[must_use]
    pub fn has_verified_source_context(&self) -> bool {
        self.evidence.iter().any(|evidence| {
            evidence.source == VERIFIED_GITHUB_DIFF_SOURCE && !evidence.finding.trim().is_empty()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub source: String,
    pub finding: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairInput {
    pub diagnosis: DiagnosisResult,
    pub repository_rules: Vec<String>,
    pub previous_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairProposal {
    pub unified_diff: String,
    pub changed_files: Vec<String>,
    pub explanation: String,
    pub validation_commands: Vec<ControlledCommand>,
}

impl RepairProposal {
    pub fn validate(&self) -> Result<(), OutputValidationError> {
        if self.unified_diff.trim().is_empty() || self.unified_diff.len() > 1_000_000 {
            return Err(OutputValidationError::InvalidPatch);
        }
        if self.changed_files.is_empty() {
            return Err(OutputValidationError::InvalidPatch);
        }
        for path in &self.changed_files {
            validate_repair_path(path)?;
        }
        if self.validation_commands.is_empty() {
            return Err(OutputValidationError::MissingSafetyPlan);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlledCommand {
    pub program: String,
    pub arguments: Vec<String>,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn test_connection(&self) -> Result<(), ProviderError>;
    async fn diagnose(&self, input: &DiagnosisInput) -> Result<DiagnosisResult, ProviderError>;
    async fn propose_patch(&self, input: &RepairInput) -> Result<RepairProposal, ProviderError>;
}

fn validate_relative_path(path: &str) -> Result<(), OutputValidationError> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains("../")
        || normalized.contains(':')
    {
        return Err(OutputValidationError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn validate_repair_path(path: &str) -> Result<(), OutputValidationError> {
    validate_relative_path(path)?;
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let sensitive_component = normalized.split('/').any(|component| {
        component == ".env"
            || component.starts_with(".env.")
            || component == "migrations"
            || component == "terraform"
            || component == "k8s"
            || component == "kubernetes"
            || component == "helm"
            || component == "charts"
            || component == "infra"
    });
    let sensitive_file = matches!(
        file_name,
        "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "cargo.toml"
            | "cargo.lock"
            | "vercel.json"
            | "docker-compose.yml"
            | "docker-compose.yaml"
            | "compose.yml"
            | "compose.yaml"
    ) || file_name == "dockerfile"
        || file_name.starts_with("dockerfile.")
        || file_name.ends_with(".tf")
        || file_name.ends_with(".tfvars")
        || normalized.starts_with(".github/workflows/");
    if sensitive_component || sensitive_file {
        return Err(OutputValidationError::SensitiveRepairPath(path.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OutputValidationError {
    #[error("model output omitted the root cause")]
    MissingRootCause,
    #[error("model output omitted evidence")]
    MissingEvidence,
    #[error("confidence must be between zero and one")]
    InvalidConfidence,
    #[error("model output contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("model attempted to repair a sensitive infrastructure or dependency path: {0}")]
    SensitiveRepairPath(String),
    #[error("model output omitted validation or rollback planning")]
    MissingSafetyPlan,
    #[error("model patch is empty, too large, or has no changed files")]
    InvalidPatch,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("model provider authentication failed")]
    Authentication,
    #[error("model provider request failed: {0}")]
    Request(String),
    #[error("verified source context is unavailable; refusing to invent a repair patch")]
    InsufficientSourceContext,
    #[error("model provider returned invalid structured output: {0}")]
    InvalidOutput(#[from] OutputValidationError),
    #[error("model provider response could not be decoded")]
    Decode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_diagnosis() -> DiagnosisResult {
        DiagnosisResult {
            suspected_root_cause: "Recent null handling regression".to_owned(),
            evidence: vec![Evidence {
                source: "stack trace".to_owned(),
                finding: "src/checkout.rs:48".to_owned(),
            }],
            confidence: 0.92,
            affected_files: vec!["src/checkout.rs".to_owned()],
            proposed_actions: vec!["add a null guard".to_owned()],
            risk_level: RiskLevel::Low,
            validation_plan: vec!["run checkout tests".to_owned()],
            rollback_plan: "restore the known-good deployment".to_owned(),
        }
    }

    fn valid_repair(path: &str) -> RepairProposal {
        RepairProposal {
            unified_diff: format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new"),
            changed_files: vec![path.to_owned()],
            explanation: "repair regression".to_owned(),
            validation_commands: vec![ControlledCommand {
                program: "npm".to_owned(),
                arguments: vec!["test".to_owned()],
            }],
        }
    }

    #[test]
    fn accepts_complete_structured_diagnosis() {
        assert_eq!(valid_diagnosis().validate(), Ok(()));
    }

    #[test]
    fn rejects_repo_escape_paths() {
        let mut diagnosis = valid_diagnosis();
        diagnosis.affected_files = vec!["../../host-secret".to_owned()];
        assert!(matches!(
            diagnosis.validate(),
            Err(OutputValidationError::UnsafePath(_))
        ));
    }

    #[test]
    fn rejects_non_finite_confidence() {
        let mut diagnosis = valid_diagnosis();
        diagnosis.confidence = f32::NAN;
        assert_eq!(
            diagnosis.validate(),
            Err(OutputValidationError::InvalidConfidence)
        );
    }

    #[test]
    fn recognizes_only_explicit_verified_source_evidence() {
        let mut diagnosis = valid_diagnosis();
        assert!(!diagnosis.has_verified_source_context());
        diagnosis.evidence.push(Evidence {
            source: VERIFIED_GITHUB_DIFF_SOURCE.to_owned(),
            finding: "FILE: src/checkout.rs\n@@ -1 +1 @@".to_owned(),
        });
        assert!(diagnosis.has_verified_source_context());
    }

    #[test]
    fn permits_normal_application_code_repairs() {
        assert_eq!(valid_repair("src/login.ts").validate(), Ok(()));
    }

    #[test]
    fn blocks_sensitive_automatic_repair_paths() {
        for path in [
            ".github/workflows/deploy.yml",
            ".env.production",
            "prisma/migrations/001.sql",
            "infra/main.tf",
            "vercel.json",
            "Dockerfile",
            "package.json",
        ] {
            assert!(matches!(
                valid_repair(path).validate(),
                Err(OutputValidationError::SensitiveRepairPath(_))
            ));
        }
    }
}
