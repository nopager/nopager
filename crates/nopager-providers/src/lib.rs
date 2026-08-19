use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[path = "core.rs"]
mod core;
mod setup_validation;

pub(crate) use core::validate_repair_path;
pub use core::{
    CommitContext, ControlledCommand, DiagnosisInput, DiagnosisResult, Evidence,
    OutputValidationError, RepairInput, RepairProposal, RiskLevel, SourceFile,
    VERIFIED_GITHUB_DIFF_SOURCE,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("model provider authentication failed")]
    Authentication,
    #[error("model provider request failed: {0}")]
    Request(String),
    #[error("configured model is not available to this provider account: {0}")]
    ModelUnavailable(String),
    #[error(
        "selected model failed NoPager's structured-output capability probe ({model}): {reason}"
    )]
    CapabilityProbeFailed { model: String, reason: String },
    #[error("verified source context is unavailable; refusing to invent a repair patch")]
    InsufficientSourceContext,
    #[error("model provider returned invalid structured output: {0}")]
    InvalidOutput(#[from] OutputValidationError),
    #[error("model provider response could not be decoded")]
    Decode,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn test_connection(&self) -> Result<(), ProviderError>;
    async fn diagnose(&self, input: &DiagnosisInput) -> Result<DiagnosisResult, ProviderError>;
    async fn propose_patch(&self, input: &RepairInput) -> Result<RepairProposal, ProviderError>;
}

pub async fn discover_available_models(
    provider: &str,
    api_key: SecretString,
) -> Result<Vec<AvailableModel>, ProviderError> {
    setup_validation::discover_available_models(provider, api_key).await
}

macro_rules! provider_wrapper {
    ($name:ident, $id:literal, $kind:ident, $base:literal) => {
        #[derive(Clone)]
        pub struct $name {
            inner: core::$name,
            setup: setup_validation::SetupProvider,
        }

        impl $name {
            pub fn new(
                api_key: SecretString,
                model: impl Into<String>,
            ) -> Result<Self, ProviderError> {
                let model = model.into();
                let base_url = Url::parse($base).expect("constant provider URL");
                let inner = core::$name::new(api_key.clone(), model.clone())?;
                let setup = setup_validation::SetupProvider::new(
                    setup_validation::ProviderKind::$kind,
                    api_key,
                    model,
                    base_url,
                )?;
                Ok(Self { inner, setup })
            }

            pub fn with_base_url(mut self, base_url: Url) -> Self {
                self.inner = self.inner.with_base_url(base_url.clone());
                self.setup = self.setup.with_base_url(base_url);
                self
            }
        }

        #[async_trait]
        impl ModelProvider for $name {
            fn id(&self) -> &'static str {
                $id
            }

            async fn test_connection(&self) -> Result<(), ProviderError> {
                self.setup.test_connection().await
            }

            async fn diagnose(
                &self,
                input: &DiagnosisInput,
            ) -> Result<DiagnosisResult, ProviderError> {
                ModelProvider::diagnose(&self.inner, input).await
            }

            async fn propose_patch(
                &self,
                input: &RepairInput,
            ) -> Result<RepairProposal, ProviderError> {
                ModelProvider::propose_patch(&self.inner, input).await
            }
        }
    };
}

provider_wrapper!(
    OpenAiProvider,
    "openai",
    OpenAi,
    "https://api.openai.com/v1/"
);
provider_wrapper!(
    AnthropicProvider,
    "anthropic",
    Anthropic,
    "https://api.anthropic.com/v1/"
);
provider_wrapper!(
    GeminiProvider,
    "gemini",
    Gemini,
    "https://generativelanguage.googleapis.com/"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_setup_provider_ids_remain_stable() {
        let key = SecretString::from("test-key".to_owned());
        let openai = OpenAiProvider::new(key.clone(), "test-model").unwrap();
        let anthropic = AnthropicProvider::new(key.clone(), "test-model").unwrap();
        let gemini = GeminiProvider::new(key, "test-model").unwrap();
        assert_eq!(openai.id(), "openai");
        assert_eq!(anthropic.id(), "anthropic");
        assert_eq!(gemini.id(), "gemini");
    }
}
