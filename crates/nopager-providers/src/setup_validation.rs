use std::collections::BTreeSet;

use reqwest::{Client, Method, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use url::Url;

use crate::{AvailableModel, ProviderError};

const MAX_DISCOVERED_MODELS: usize = 200;
const CAPABILITY_PROBE_MAX_OUTPUT_TOKENS: u64 = 1024;
const CAPABILITY_PROBE_SYSTEM_PROMPT: &str = "This is a NoPager setup capability probe. Return only the requested structured JSON and set ok to true.";

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Clone)]
pub(crate) struct SetupProvider {
    kind: ProviderKind,
    http: Client,
    api_key: SecretString,
    model: String,
    base_url: Url,
}

#[derive(serde::Deserialize)]
struct CapabilityProbe {
    ok: bool,
}

impl ProviderKind {
    fn from_id(value: &str) -> Result<Self, ProviderError> {
        match value {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            _ => Err(ProviderError::Request("unsupported provider".to_owned())),
        }
    }

    fn base_url(self) -> Url {
        let value = match self {
            Self::OpenAi => "https://api.openai.com/v1/",
            Self::Anthropic => "https://api.anthropic.com/v1/",
            Self::Gemini => "https://generativelanguage.googleapis.com/",
        };
        Url::parse(value).expect("constant provider URL")
    }
}

impl SetupProvider {
    pub(crate) fn new(
        kind: ProviderKind,
        api_key: SecretString,
        model: String,
        base_url: Url,
    ) -> Result<Self, ProviderError> {
        if api_key.expose_secret().trim().is_empty() || model.trim().is_empty() {
            return Err(ProviderError::Authentication);
        }
        Self::build(kind, api_key, model, base_url)
    }

    fn for_discovery(
        kind: ProviderKind,
        api_key: SecretString,
        base_url: Url,
    ) -> Result<Self, ProviderError> {
        if api_key.expose_secret().trim().is_empty() {
            return Err(ProviderError::Authentication);
        }
        Self::build(kind, api_key, String::new(), base_url)
    }

    fn build(
        kind: ProviderKind,
        api_key: SecretString,
        model: String,
        base_url: Url,
    ) -> Result<Self, ProviderError> {
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        Ok(Self {
            kind,
            http,
            api_key,
            model,
            base_url,
        })
    }

    pub(crate) fn with_base_url(mut self, base_url: Url) -> Self {
        self.base_url = base_url;
        self
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, ProviderError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        let request = self.http.request(method, url);
        Ok(match self.kind {
            ProviderKind::OpenAi => request.bearer_auth(self.api_key.expose_secret()),
            ProviderKind::Anthropic => request
                .header("x-api-key", self.api_key.expose_secret())
                .header("anthropic-version", "2023-06-01"),
            ProviderKind::Gemini => request.header("x-goog-api-key", self.api_key.expose_secret()),
        })
    }

    pub(crate) async fn test_connection(&self) -> Result<(), ProviderError> {
        let models = self.list_models().await?;
        if !models.iter().any(|model| model.id == self.model) {
            return Err(ProviderError::ModelUnavailable(self.model.clone()));
        }
        self.capability_probe().await
    }

    async fn list_models(&self) -> Result<Vec<AvailableModel>, ProviderError> {
        let path = match self.kind {
            ProviderKind::OpenAi => "models",
            ProviderKind::Anthropic => "models?limit=100",
            ProviderKind::Gemini => "v1beta/models?pageSize=1000",
        };
        let response = decode_json(
            self.request(Method::GET, path)?
                .send()
                .await
                .map_err(request_error)?,
        )
        .await?;
        parse_available_models(self.kind, &response)
    }

    async fn capability_probe(&self) -> Result<(), ProviderError> {
        let result = self
            .run_capability_probe()
            .await
            .map_err(|error| match error {
                ProviderError::Authentication => ProviderError::Authentication,
                other => ProviderError::CapabilityProbeFailed {
                    model: self.model.clone(),
                    reason: other.to_string(),
                },
            })?;
        if result.ok {
            Ok(())
        } else {
            Err(ProviderError::CapabilityProbeFailed {
                model: self.model.clone(),
                reason: "provider returned ok=false".to_owned(),
            })
        }
    }

    async fn run_capability_probe(&self) -> Result<CapabilityProbe, ProviderError> {
        let schema = capability_probe_schema();
        let prompt =
            "Return the requested capability_probe for this input:\n{\"probe\":\"nopager_setup\"}";
        let body = self.structured_request_body(
            "capability_probe",
            schema,
            prompt,
            CAPABILITY_PROBE_SYSTEM_PROMPT,
            CAPABILITY_PROBE_MAX_OUTPUT_TOKENS,
        );
        let response = match self.kind {
            ProviderKind::OpenAi => {
                decode_json(
                    self.request(Method::POST, "responses")?
                        .json(&body)
                        .send()
                        .await
                        .map_err(request_error)?,
                )
                .await?
            }
            ProviderKind::Anthropic => {
                decode_json(
                    self.request(Method::POST, "messages")?
                        .json(&body)
                        .send()
                        .await
                        .map_err(request_error)?,
                )
                .await?
            }
            ProviderKind::Gemini => {
                let path = format!("v1beta/models/{}:generateContent", self.model);
                decode_json(
                    self.request(Method::POST, &path)?
                        .json(&body)
                        .send()
                        .await
                        .map_err(request_error)?,
                )
                .await?
            }
        };
        let text = extract_text(self.kind, &response)?;
        serde_json::from_str(text).map_err(|_| ProviderError::Decode)
    }

    fn structured_request_body(
        &self,
        name: &str,
        schema: Value,
        prompt: &str,
        system_prompt: &str,
        max_output_tokens: u64,
    ) -> Value {
        match self.kind {
            ProviderKind::OpenAi => json!({
                "model": self.model,
                "instructions": system_prompt,
                "input": prompt,
                "max_output_tokens": max_output_tokens,
                "text": { "format": { "type": "json_schema", "name": name, "strict": true, "schema": schema } }
            }),
            ProviderKind::Anthropic => json!({
                "model": self.model,
                "max_tokens": max_output_tokens,
                "system": system_prompt,
                "messages": [{ "role": "user", "content": prompt }],
                "output_config": { "format": { "type": "json_schema", "schema": schema } }
            }),
            ProviderKind::Gemini => json!({
                "systemInstruction": { "parts": [{ "text": system_prompt }] },
                "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
                "generationConfig": {
                    "responseMimeType": "application/json",
                    "responseJsonSchema": schema,
                    "maxOutputTokens": max_output_tokens
                }
            }),
        }
    }
}

pub(crate) async fn discover_available_models(
    provider: &str,
    api_key: SecretString,
) -> Result<Vec<AvailableModel>, ProviderError> {
    let kind = ProviderKind::from_id(provider)?;
    SetupProvider::for_discovery(kind, api_key, kind.base_url())?
        .list_models()
        .await
}

fn parse_available_models(
    kind: ProviderKind,
    response: &Value,
) -> Result<Vec<AvailableModel>, ProviderError> {
    let items = match kind {
        ProviderKind::OpenAi | ProviderKind::Anthropic => response.get("data"),
        ProviderKind::Gemini => response.get("models"),
    }
    .and_then(Value::as_array)
    .ok_or(ProviderError::Decode)?;

    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for item in items {
        if models.len() >= MAX_DISCOVERED_MODELS {
            break;
        }
        if matches!(kind, ProviderKind::Gemini) {
            let supports_generate = item
                .get("supportedGenerationMethods")
                .or_else(|| item.get("supportedActions"))
                .and_then(Value::as_array)
                .is_some_and(|methods| {
                    methods
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|method| method == "generateContent")
                });
            if !supports_generate {
                continue;
            }
        }
        let raw_id = match kind {
            ProviderKind::OpenAi | ProviderKind::Anthropic => item.get("id"),
            ProviderKind::Gemini => item.get("name"),
        }
        .and_then(Value::as_str)
        .unwrap_or_default();
        let id = raw_id.strip_prefix("models/").unwrap_or(raw_id).trim();
        if id.is_empty() || id.len() > 256 || !seen.insert(id.to_owned()) {
            continue;
        }
        let display_name = item
            .get("display_name")
            .or_else(|| item.get("displayName"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(id)
            .chars()
            .take(256)
            .collect();
        models.push(AvailableModel {
            id: id.to_owned(),
            display_name,
        });
    }
    Ok(models)
}

fn request_error(error: reqwest::Error) -> ProviderError {
    ProviderError::Request(error.to_string())
}

async fn decode_json(response: reqwest::Response) -> Result<Value, ProviderError> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::Authentication);
    }
    if !status.is_success() {
        return Err(ProviderError::Request(format!(
            "provider returned {status}"
        )));
    }
    response.json().await.map_err(|_| ProviderError::Decode)
}

fn extract_text(kind: ProviderKind, response: &Value) -> Result<&str, ProviderError> {
    match kind {
        ProviderKind::OpenAi => {
            response
                .get("output")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .flat_map(|item| {
                            item.get("content")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                        })
                        .find_map(|content| {
                            (content.get("type").and_then(Value::as_str) == Some("output_text"))
                                .then(|| content.get("text").and_then(Value::as_str))
                                .flatten()
                        })
                })
        }
        ProviderKind::Anthropic => {
            response
                .get("content")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find_map(|content| {
                        (content.get("type").and_then(Value::as_str) == Some("text"))
                            .then(|| content.get("text").and_then(Value::as_str))
                            .flatten()
                    })
                })
        }
        ProviderKind::Gemini => response
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str),
    }
    .ok_or(ProviderError::Decode)
}

fn capability_probe_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "ok": { "type": "boolean" }
        },
        "required": ["ok"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_model_lists_and_filters_gemini_generation_models() {
        let openai = json!({"data":[{"id":"gpt-example"},{"id":"gpt-example"}]});
        let anthropic = json!({"data":[{"id":"claude-example","display_name":"Claude Example"}]});
        let gemini = json!({
            "models":[
                {"name":"models/gemini-text","displayName":"Gemini Text","supportedGenerationMethods":["generateContent"]},
                {"name":"models/gemini-embed","displayName":"Gemini Embed","supportedGenerationMethods":["embedContent"]}
            ]
        });
        assert_eq!(
            parse_available_models(ProviderKind::OpenAi, &openai).unwrap(),
            vec![AvailableModel {
                id: "gpt-example".into(),
                display_name: "gpt-example".into()
            }]
        );
        assert_eq!(
            parse_available_models(ProviderKind::Anthropic, &anthropic).unwrap(),
            vec![AvailableModel {
                id: "claude-example".into(),
                display_name: "Claude Example".into()
            }]
        );
        assert_eq!(
            parse_available_models(ProviderKind::Gemini, &gemini).unwrap(),
            vec![AvailableModel {
                id: "gemini-text".into(),
                display_name: "Gemini Text".into()
            }]
        );
    }

    #[test]
    fn model_list_parser_is_bounded_and_rejects_malformed_shapes() {
        let response = json!({
            "data": (0..(MAX_DISCOVERED_MODELS + 50))
                .map(|index| json!({"id": format!("model-{index}")}))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            parse_available_models(ProviderKind::OpenAi, &response)
                .unwrap()
                .len(),
            MAX_DISCOVERED_MODELS
        );
        assert!(matches!(
            parse_available_models(ProviderKind::OpenAi, &json!({"models": []})),
            Err(ProviderError::Decode)
        ));
    }

    #[test]
    fn capability_probe_uses_runtime_structured_output_shape_with_small_budget() {
        let schema = capability_probe_schema();
        let key = SecretString::from("test-key".to_owned());
        for kind in [
            ProviderKind::OpenAi,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
        ] {
            let provider = SetupProvider::new(
                kind,
                key.clone(),
                "test-model".to_owned(),
                Url::parse("https://example.com/").unwrap(),
            )
            .unwrap();
            let body = provider.structured_request_body(
                "capability_probe",
                schema.clone(),
                "Return ok=true",
                CAPABILITY_PROBE_SYSTEM_PROMPT,
                CAPABILITY_PROBE_MAX_OUTPUT_TOKENS,
            );
            match kind {
                ProviderKind::OpenAi => {
                    assert_eq!(body["model"], "test-model");
                    assert_eq!(
                        body["max_output_tokens"],
                        CAPABILITY_PROBE_MAX_OUTPUT_TOKENS
                    );
                    assert_eq!(body["text"]["format"]["type"], "json_schema");
                    assert_eq!(body["text"]["format"]["strict"], true);
                }
                ProviderKind::Anthropic => {
                    assert_eq!(body["model"], "test-model");
                    assert_eq!(body["max_tokens"], CAPABILITY_PROBE_MAX_OUTPUT_TOKENS);
                    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
                }
                ProviderKind::Gemini => {
                    assert!(body.get("model").is_none());
                    assert_eq!(
                        body["generationConfig"]["maxOutputTokens"],
                        CAPABILITY_PROBE_MAX_OUTPUT_TOKENS
                    );
                    assert_eq!(
                        body["generationConfig"]["responseMimeType"],
                        "application/json"
                    );
                    assert_eq!(body["generationConfig"]["responseJsonSchema"], schema);
                }
            }
        }
    }

    #[test]
    fn capability_probe_schema_requires_only_boolean_result() {
        let schema = capability_probe_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["ok"]));
        assert_eq!(schema["properties"]["ok"]["type"], "boolean");
    }
}
