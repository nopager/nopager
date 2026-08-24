use nopager_crypto::SecretCipher;
use nopager_db::Database;
use nopager_providers::{AnthropicProvider, GeminiProvider, ModelProvider, OpenAiProvider};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use uuid::Uuid;

use super::required_string;

pub(super) async fn provider_for(
    database: &Database,
    project_id: Uuid,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    if let Ok(integration) = database
        .integration_secret(project_id, "model_provider")
        .await
    {
        let credentials = decrypt_credentials(&integration.encrypted_credentials)?;
        let api_key = SecretString::from(required_string(&credentials, "apiKey")?.to_owned());
        let kind = integration
            .metadata
            .get("provider")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("model provider metadata is missing provider"))?;
        let model = integration
            .metadata
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("model provider metadata is missing model"))?
            .to_owned();
        return provider(kind, api_key, model);
    }

    let kind = std::env::var("NOPAGER_AI_PROVIDER").unwrap_or_else(|_| "openai".into());
    let model = std::env::var("NOPAGER_AI_MODEL")
        .map_err(|_| anyhow::anyhow!("NOPAGER_AI_MODEL is required"))?;
    let key_name = match kind.as_str() {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => anyhow::bail!("unsupported NOPAGER_AI_PROVIDER: {kind}"),
    };
    provider(&kind, secret_env(key_name)?, model)
}

fn provider(
    kind: &str,
    api_key: SecretString,
    model: String,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    Ok(match kind {
        "openai" => Box::new(OpenAiProvider::new(api_key, model)?) as Box<dyn ModelProvider>,
        "anthropic" => Box::new(AnthropicProvider::new(api_key, model)?),
        "gemini" => Box::new(GeminiProvider::new(api_key, model)?),
        _ => anyhow::bail!("unsupported model provider: {kind}"),
    })
}

fn decrypt_credentials(encrypted: &str) -> anyhow::Result<Value> {
    let key = SecretString::from(std::env::var("NOPAGER_MASTER_KEY")?);
    let plaintext = SecretCipher::from_base64_key(&key)?.decrypt(encrypted)?;
    Ok(serde_json::from_str(plaintext.expose_secret())?)
}

fn secret_env(name: &str) -> anyhow::Result<SecretString> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
        .ok_or_else(|| anyhow::anyhow!("{name} is required for the selected provider"))
}
