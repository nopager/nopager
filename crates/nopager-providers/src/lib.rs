use async_trait::async_trait;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
use thiserror::Error;

mod http;
pub use http::{AnthropicProvider, GeminiProvider, OpenAiProvider};

pub const VERIFIED_GITHUB_DIFF_SOURCE: &str = "verified_github_diff";

const MAX_RECENT_COMMITS: usize = 8;
const MAX_RELEVANT_FILES: usize = 16;
const MAX_COMMIT_MESSAGE_CHARS: usize = 48_000;
const MAX_STACK_TRACE_CHARS: usize = 16_000;
const MAX_SOURCE_FILE_CHARS: usize = 24_000;
const MAX_CONTEXT_STRING_CHARS: usize = 12_000;
const MAX_CONTEXT_ARRAY_ITEMS: usize = 32;
const MAX_CONTEXT_OBJECT_KEYS: usize = 64;
const MAX_CONTEXT_DEPTH: usize = 8;
const TRUNCATED: &str = "[TRUNCATED_BY_NOPAGER]";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisInput {
    pub incident_summary: String,
    #[serde(serialize_with = "serialize_recent_commits")]
    pub recent_commits: Vec<CommitContext>,
    #[serde(serialize_with = "serialize_optional_stack_trace")]
    pub stack_trace: Option<String>,
    #[serde(serialize_with = "serialize_bounded_context")]
    pub deployment: Value,
    #[serde(serialize_with = "serialize_bounded_context")]
    pub health_failure: Value,
    #[serde(serialize_with = "serialize_relevant_files")]
    pub relevant_files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitContext {
    pub sha: String,
    #[serde(serialize_with = "serialize_commit_message")]
    pub message: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub path: String,
    #[serde(serialize_with = "serialize_source_file_content")]
    pub content: String,
}

fn serialize_recent_commits<S>(value: &[CommitContext], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value.iter().take(MAX_RECENT_COMMITS).collect::<Vec<_>>().serialize(serializer)
}

fn serialize_relevant_files<S>(value: &[SourceFile], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value.iter().take(MAX_RELEVANT_FILES).collect::<Vec<_>>().serialize(serializer)
}

fn serialize_optional_stack_trace<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&truncate_chars(value, MAX_STACK_TRACE_CHARS)),
        None => serializer.serialize_none(),
    }
}

fn serialize_commit_message<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&truncate_chars(value, MAX_COMMIT_MESSAGE_CHARS))
}

fn serialize_source_file_content<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&truncate_chars(value, MAX_SOURCE_FILE_CHARS))
}

fn serialize_bounded_context<S>(value: &Value, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    bounded_context(value, 0).serialize(serializer)
}

fn bounded_context(value: &Value, depth: usize) -> Value {
    if depth >= MAX_CONTEXT_DEPTH {
        return Value::String(TRUNCATED.to_owned());
    }
    match value {
        Value::Object(map) => {
            let mut bounded = Map::new();
            for (key, value) in map.iter().take(MAX_CONTEXT_OBJECT_KEYS) {
                bounded.insert(key.clone(), bounded_context(value, depth + 1));
            }
            if map.len() > MAX_CONTEXT_OBJECT_KEYS {
                bounded.insert("_nopagerContextTruncated".into(), Value::Bool(true));
            }
            Value::Object(bounded)
        }
        Value::Array(values) => {
            let mut bounded = values
                .iter()
                .take(MAX_CONTEXT_ARRAY_ITEMS)
                .map(|value| bounded_context(value, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > MAX_CONTEXT_ARRAY_ITEMS {
                bounded.push(Value::String(TRUNCATED.to_owned()));
            }
            Value::Array(bounded)
        }
        Value::String(value) => Value::String(truncate_chars(value, MAX_CONTEXT_STRING_CHARS)),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut bounded = value.chars().take(max_chars).collect::<String>();
    bounded.push_str("\n");
    bounded.push_str(TRUNCATED);
    bounded
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

    #[test]
    fn bounds_untrusted_external_context_before_provider_serialization() {
        let input = DiagnosisInput {
            incident_summary: "500 after deploy".into(),
            recent_commits: (0..20)
                .map(|index| CommitContext {
                    sha: format!("sha-{index}"),
                    message: "m".repeat(MAX_COMMIT_MESSAGE_CHARS + 100),
                    changed_files: vec!["src/app.rs".into()],
                })
                .collect(),
            stack_trace: Some("s".repeat(MAX_STACK_TRACE_CHARS + 100)),
            deployment: serde_json::json!({
                "events": (0..100).collect::<Vec<_>>(),
                "body": "b".repeat(MAX_CONTEXT_STRING_CHARS + 100)
            }),
            health_failure: Value::Null,
            relevant_files: (0..30)
                .map(|index| SourceFile {
                    path: format!("src/{index}.rs"),
                    content: "c".repeat(MAX_SOURCE_FILE_CHARS + 100),
                })
                .collect(),
        };

        let serialized = serde_json::to_value(&input).expect("serialize bounded model input");
        assert_eq!(
            serialized["recentCommits"].as_array().unwrap().len(),
            MAX_RECENT_COMMITS
        );
        assert_eq!(
            serialized["relevantFiles"].as_array().unwrap().len(),
            MAX_RELEVANT_FILES
        );
        assert!(serialized["stackTrace"].as_str().unwrap().contains(TRUNCATED));
        assert!(
            serialized["deployment"]["body"]
                .as_str()
                .unwrap()
                .contains(TRUNCATED)
        );
        assert_eq!(
            serialized["deployment"]["events"]
                .as_array()
                .unwrap()
                .len(),
            MAX_CONTEXT_ARRAY_ITEMS + 1
        );
        assert!(
            serialized["recentCommits"][0]["message"]
                .as_str()
                .unwrap()
                .contains(TRUNCATED)
        );
        assert!(
            serialized["relevantFiles"][0]["content"]
                .as_str()
                .unwrap()
                .contains(TRUNCATED)
        );
    }

    #[test]
    fn bounds_nested_context_depth_and_object_size() {
        let mut nested = Value::String("leaf".into());
        for _ in 0..(MAX_CONTEXT_DEPTH + 2) {
            nested = serde_json::json!({ "next": nested });
        }
        let large_object = Value::Object(
            (0..100)
                .map(|index| (format!("key-{index:03}"), Value::Number(index.into())))
                .collect(),
        );
        let bounded_nested = bounded_context(&nested, 0);
        let bounded_object = bounded_context(&large_object, 0);
        assert!(bounded_nested.to_string().contains(TRUNCATED));
        assert_eq!(
            bounded_object.as_object().unwrap().len(),
            MAX_CONTEXT_OBJECT_KEYS + 1
        );
        assert_eq!(
            bounded_object["_nopagerContextTruncated"],
            Value::Bool(true)
        );
    }
}
