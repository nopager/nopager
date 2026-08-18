use async_trait::async_trait;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

mod http;
pub use http::{AnthropicProvider, GeminiProvider, OpenAiProvider};

pub const VERIFIED_GITHUB_DIFF_SOURCE: &str = "verified_github_diff";
const REDACTED_PERSONAL_DATA: &str = "[PERSONAL_DATA_REDACTED_BY_NOPAGER]";
const REDACTED_EMAIL: &str = "[EMAIL_REDACTED_BY_NOPAGER]";

#[derive(Debug, Clone)]
pub struct DiagnosisInput {
    pub incident_summary: String,
    pub recent_commits: Vec<CommitContext>,
    pub stack_trace: Option<String>,
    pub deployment: Value,
    pub health_failure: Value,
    pub relevant_files: Vec<SourceFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosisInputRef<'a> {
    incident_summary: &'a str,
    recent_commits: &'a [CommitContext],
    stack_trace: &'a Option<String>,
    deployment: &'a Value,
    health_failure: &'a Value,
    relevant_files: &'a [SourceFile],
}

impl Serialize for DiagnosisInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw = DiagnosisInputRef {
            incident_summary: &self.incident_summary,
            recent_commits: &self.recent_commits,
            stack_trace: &self.stack_trace,
            deployment: &self.deployment,
            health_failure: &self.health_failure,
            relevant_files: &self.relevant_files,
        };
        serialize_model_input(raw, serializer)
    }
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

#[derive(Debug, Clone)]
pub struct RepairInput {
    pub diagnosis: DiagnosisResult,
    pub repository_rules: Vec<String>,
    pub previous_failures: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepairInputRef<'a> {
    diagnosis: &'a DiagnosisResult,
    repository_rules: &'a [String],
    previous_failures: &'a [String],
}

impl Serialize for RepairInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw = RepairInputRef {
            diagnosis: &self.diagnosis,
            repository_rules: &self.repository_rules,
            previous_failures: &self.previous_failures,
        };
        serialize_model_input(raw, serializer)
    }
}

fn serialize_model_input<T, S>(raw: T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    let mut value = serde_json::to_value(raw).map_err(serde::ser::Error::custom)?;
    redact_model_personal_data(&mut value);
    value.serialize(serializer)
}

fn redact_model_personal_data(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if personal_data_json_key(key) {
                    *value = Value::String(REDACTED_PERSONAL_DATA.to_owned());
                } else {
                    redact_model_personal_data(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_model_personal_data(value);
            }
        }
        Value::String(value) => *value = redact_email_addresses(value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn personal_data_json_key(key: &str) -> bool {
    let normalized = normalize_privacy_key(key);
    normalized.ends_with("email")
        || normalized.ends_with("emailaddress")
        || normalized == "phone"
        || normalized.ends_with("phonenumber")
        || matches!(
            normalized.as_str(),
            "ssn"
                | "socialsecuritynumber"
                | "nationalid"
                | "passportnumber"
                | "creditcardnumber"
                | "cardnumber"
        )
}

fn normalize_privacy_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn redact_email_addresses(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut matches = Vec::new();

    for (at, byte) in bytes.iter().enumerate() {
        if *byte != b'@' {
            continue;
        }

        let mut start = at;
        while start > 0 && email_local_byte(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && email_domain_byte(bytes[end]) {
            end += 1;
        }

        if start < at
            && end > at + 1
            && valid_email_domain(&value[at + 1..end])
            && matches
                .last()
                .map_or(true, |(_, previous_end)| start >= *previous_end)
        {
            matches.push((start, end));
        }
    }

    if matches.is_empty() {
        return value.to_owned();
    }

    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    for (start, end) in matches {
        output.push_str(&value[cursor..start]);
        output.push_str(REDACTED_EMAIL);
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn email_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn email_domain_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

fn valid_email_domain(domain: &str) -> bool {
    if domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    let mut labels = domain.split('.');
    let mut last = None;
    for label in labels.by_ref() {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return false;
        }
        last = Some(label);
    }
    last.is_some_and(|label| {
        label.len() >= 2 && label.bytes().all(|byte| byte.is_ascii_alphabetic())
    })
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
    fn model_inputs_redact_high_confidence_customer_pii() {
        let input = DiagnosisInput {
            incident_summary: "checkout failed for alice@example.com while user.email was read"
                .into(),
            recent_commits: vec![CommitContext {
                sha: "abc123".into(),
                message: "preserve user.email field semantics".into(),
                changed_files: vec!["src/checkout.ts".into()],
            }],
            stack_trace: Some("customer bob+prod@example.co.uk failed at checkout.ts:184".into()),
            deployment: serde_json::json!({
                "customerEmail": "alice@example.com",
                "phoneNumber": "+1-415-555-0100",
                "url": "https://prod.example.com"
            }),
            health_failure: Value::Null,
            relevant_files: vec![SourceFile {
                path: "src/checkout.ts".into(),
                content: "const field = user.email; // notify owner@example.com".into(),
            }],
        };
        let rendered = serde_json::to_string(&input).expect("serialize model input");
        assert!(!rendered.contains("alice@example.com"));
        assert!(!rendered.contains("bob+prod@example.co.uk"));
        assert!(!rendered.contains("owner@example.com"));
        assert!(!rendered.contains("+1-415-555-0100"));
        assert!(rendered.contains(REDACTED_EMAIL));
        assert!(rendered.contains(REDACTED_PERSONAL_DATA));
        assert!(rendered.contains("user.email"));
        assert!(rendered.contains("https://prod.example.com"));
    }

    #[test]
    fn repair_inputs_redact_personal_data_before_model_serialization() {
        let mut diagnosis = valid_diagnosis();
        diagnosis.evidence[0].finding = "customer alice@example.com failed checkout".into();
        let input = RepairInput {
            diagnosis,
            repository_rules: vec!["Do not email owner@example.com from tests".into()],
            previous_failures: vec!["fixture user@example.net remained in output".into()],
        };
        let rendered = serde_json::to_string(&input).expect("serialize repair input");
        assert!(!rendered.contains("alice@example.com"));
        assert!(!rendered.contains("owner@example.com"));
        assert!(!rendered.contains("user@example.net"));
        assert!(rendered.contains(REDACTED_EMAIL));
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
