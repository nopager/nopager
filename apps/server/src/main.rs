use std::sync::Arc;

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use nopager_config::ServerConfig;
use nopager_connectors::{github::GitHubAppAuth, vercel::VercelClient};
use nopager_crypto::SecretCipher;
use nopager_db::{Database, ProtectedAppSetup};
use nopager_monitor::{check_http, validate_health_url};
use nopager_providers::{
    AnthropicProvider, GeminiProvider, ModelProvider, OpenAiProvider, ProviderError,
    discover_available_models,
};
use nopager_webhooks::{verify_github, verify_vercel};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration as TimeDuration, OffsetDateTime};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;
use url::Url;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

#[derive(Deserialize)]
struct AdminCredentials {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtectAppRequest {
    name: String,
    repo_owner: String,
    repo_name: String,
    github_app_id: u64,
    github_installation_id: u64,
    github_private_key: String,
    github_webhook_secret: String,
    #[serde(default)]
    vercel_team_id: String,
    vercel_project_id: String,
    vercel_token: String,
    #[serde(default)]
    vercel_webhook_secret: String,
    provider: String,
    provider_api_key: String,
    provider_model: String,
    production_url: String,
    health_check_url: String,
    safety_mode: String,
}

#[derive(Deserialize)]
struct SafetyModeRequest {
    mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubConnectionRequest {
    app_id: u64,
    installation_id: u64,
    private_key: String,
    repo_owner: String,
    repo_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VercelConnectionRequest {
    #[serde(default)]
    team_id: String,
    project_id: String,
    token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConnectionRequest {
    provider: String,
    api_key: String,
    model: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelsRequest {
    provider: String,
    api_key: String,
}

#[derive(Deserialize)]
struct HealthConnectionRequest {
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthDiscoveryRequest {
    production_url: String,
}

#[derive(Clone)]
struct ServerState {
    database: Option<Database>,
    github_webhook_secret: Option<Arc<[u8]>>,
    vercel_webhook_secret: Option<Arc<[u8]>>,
    admin_token: Option<Arc<[u8]>>,
    secret_cipher: Option<Arc<SecretCipher>>,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "nopager-server",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn readyz(State(state): State<ServerState>) -> (StatusCode, Json<HealthResponse>) {
    let ready = match state.database {
        Some(database) => database.ready().await.is_ok(),
        None => false,
    };
    let response = HealthResponse {
        status: if ready { "ready" } else { "not_ready" },
        service: "nopager-server",
        version: env!("CARGO_PKG_VERSION"),
    };
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(response),
    )
}

async fn overview(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.overview().await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => {
            tracing::error!(%error, "failed to load overview");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn incidents(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.incidents(100).await {
        Ok(value) => (StatusCode::OK, Json(json!({ "incidents": value }))),
        Err(error) => {
            tracing::error!(%error, "failed to load incidents");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn incident_detail(
    State(state): State<ServerState>,
    Path(incident_id): Path<uuid::Uuid>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.incident_detail(incident_id).await {
        Ok(Some(value)) => (StatusCode::OK, Json(value)),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "incident_not_found"),
        Err(error) => {
            tracing::error!(%incident_id, %error, "failed to load incident detail");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn settings(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.app_settings().await {
        Ok(Some(value)) => (StatusCode::OK, Json(value)),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "app_not_configured"),
        Err(error) => {
            tracing::error!(%error, "failed to load app settings");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn set_safety_mode(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<SafetyModeRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    let mode = if request.mode == "autopilot" {
        "autopilot_experimental"
    } else {
        request.mode.as_str()
    };
    match database.set_safety_mode(mode, "api-admin").await {
        Ok(updated) => (
            StatusCode::OK,
            Json(json!({ "safetyMode": mode, "updatedProjects": updated })),
        ),
        Err(nopager_db::DatabaseError::InvalidSafetyMode) => {
            api_error(StatusCode::BAD_REQUEST, "invalid_safety_mode")
        }
        Err(error) => {
            tracing::error!(%error, "failed to update safety mode");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn setup_status(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match tokio::try_join!(database.admin_exists(), database.project_exists()) {
        Ok((admin_created, app_protected)) => (
            StatusCode::OK,
            Json(json!({
                "adminCreated": admin_created,
                "appProtected": app_protected,
                "authenticated": authorized(&state, &headers).await
            })),
        ),
        Err(error) => {
            tracing::error!(%error, "failed to load setup status");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn test_github_connection(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<GitHubConnectionRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let auth = match GitHubAppAuth::new(
        request.app_id,
        request.installation_id,
        SecretString::from(normalize_pem(&request.private_key)),
    ) {
        Ok(auth) => auth,
        Err(error) => {
            tracing::warn!(%error, "GitHub connection configuration failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "github_connection_failed");
        }
    };
    let client = match auth.installation_client(&request.repo_name).await {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "GitHub installation token test failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "github_connection_failed");
        }
    };
    match client
        .get_repository(&request.repo_owner, &request.repo_name)
        .await
    {
        Ok(repository) => (
            StatusCode::OK,
            Json(json!({
                "connected": true,
                "repository": {
                    "id": repository.id,
                    "fullName": repository.full_name,
                    "defaultBranch": repository.default_branch
                }
            })),
        ),
        Err(error) => {
            tracing::warn!(%error, "GitHub repository access test failed");
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "github_repository_not_accessible",
            )
        }
    }
}

async fn test_vercel_connection(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<VercelConnectionRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let client = match VercelClient::new(
        SecretString::from(request.token),
        optional_nonempty(&request.team_id),
    ) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Vercel connection configuration failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "vercel_connection_failed");
        }
    };
    let project = match client.get_project(&request.project_id).await {
        Ok(project) => project,
        Err(error) => {
            tracing::warn!(%error, "Vercel project lookup failed");
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "vercel_project_not_accessible",
            );
        }
    };
    match client.list_deployments(&project.id, 10).await {
        Ok(deployments) if deployments.iter().any(vercel_production_is_ready) => (
            StatusCode::OK,
            Json(json!({
                "connected": true,
                "project": { "id": project.id, "name": project.name },
                "productionDeploymentFound": true
            })),
        ),
        Ok(_) => api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "vercel_production_deployment_not_found",
        ),
        Err(error) => {
            tracing::warn!(%error, "Vercel connection test failed");
            api_error(StatusCode::UNPROCESSABLE_ENTITY, "vercel_connection_failed")
        }
    }
}

async fn discover_provider_models(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<ProviderModelsRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !matches!(request.provider.as_str(), "openai" | "anthropic" | "gemini") {
        return api_error(StatusCode::BAD_REQUEST, "unsupported_provider");
    }
    if request.api_key.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "provider_api_key_required");
    }
    match discover_available_models(&request.provider, SecretString::from(request.api_key)).await {
        Ok(models) if !models.is_empty() => (StatusCode::OK, Json(json!({ "models": models }))),
        Ok(_) => api_error(StatusCode::UNPROCESSABLE_ENTITY, "provider_models_empty"),
        Err(error) => {
            tracing::warn!(%error, "model provider discovery failed");
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "provider_connection_failed",
            )
        }
    }
}

async fn test_provider_connection(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<ProviderConnectionRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let provider = configured_provider(&request.provider, request.api_key, request.model);
    match provider {
        Ok(provider) => match provider.test_connection().await {
            Ok(()) => connection_success(),
            Err(error) => {
                tracing::warn!(%error, "model provider connection test failed");
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    provider_connection_error_code(&error),
                )
            }
        },
        Err(()) => api_error(StatusCode::BAD_REQUEST, "unsupported_provider"),
    }
}

async fn discover_health_connection(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<HealthDiscoveryRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let production_url = match Url::parse(&request.production_url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "unsafe_production_url"),
    };
    let candidates = health_discovery_candidates(&production_url);
    for candidate in &candidates {
        if validate_health_url(candidate).is_err() {
            continue;
        }
        if let Ok(observation) = check_http(candidate, 200, std::time::Duration::from_secs(5)).await
            && observation.success
        {
            return (
                StatusCode::OK,
                Json(json!({
                    "found": true,
                    "url": candidate.as_str(),
                    "attempted": candidates.len()
                })),
            );
        }
    }
    (
        StatusCode::OK,
        Json(json!({ "found": false, "attempted": candidates.len() })),
    )
}

fn health_discovery_candidates(production_url: &Url) -> Vec<Url> {
    let mut candidates = Vec::new();
    let mut exact = production_url.clone();
    exact.set_fragment(None);
    candidates.push(exact);

    for path in ["/health", "/healthz", "/api/health", "/api/healthz"] {
        let mut candidate = production_url.clone();
        candidate.set_path(path);
        candidate.set_query(None);
        candidate.set_fragment(None);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

async fn test_health_connection(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<HealthConnectionRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let url = match Url::parse(&request.url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "unsafe_health_check_url"),
    };
    match check_http(&url, 200, std::time::Duration::from_secs(10)).await {
        Ok(observation) if observation.success => connection_success(),
        Ok(_) | Err(_) => api_error(StatusCode::UNPROCESSABLE_ENTITY, "production_health_failed"),
    }
}

fn configured_provider(
    kind: &str,
    api_key: String,
    model: String,
) -> Result<Box<dyn ModelProvider>, ()> {
    let key = SecretString::from(api_key);
    match kind {
        "openai" => OpenAiProvider::new(key, model)
            .map(|value| Box::new(value) as Box<dyn ModelProvider>)
            .map_err(|_| ()),
        "anthropic" => AnthropicProvider::new(key, model)
            .map(|value| Box::new(value) as Box<dyn ModelProvider>)
            .map_err(|_| ()),
        "gemini" => GeminiProvider::new(key, model)
            .map(|value| Box::new(value) as Box<dyn ModelProvider>)
            .map_err(|_| ()),
        _ => Err(()),
    }
}

fn connection_success() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "connected": true })))
}

fn provider_connection_error_code(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::ModelUnavailable(_) => "provider_model_unavailable",
        ProviderError::CapabilityProbeFailed { .. } => "provider_model_capability_failed",
        _ => "provider_connection_failed",
    }
}

async fn protect_app(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<ProtectAppRequest>,
) -> Response {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured")
            .into_response();
    };
    let Some(cipher) = &state.secret_cipher else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "master_key_not_configured")
            .into_response();
    };
    let production_url = match Url::parse(&request.production_url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "unsafe_production_url").into_response(),
    };
    let health_check_url = match Url::parse(&request.health_check_url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "unsafe_health_check_url").into_response(),
    };
    if !valid_setup(&request) {
        return api_error(StatusCode::BAD_REQUEST, "invalid_setup").into_response();
    }
    match check_http(&health_check_url, 200, std::time::Duration::from_secs(10)).await {
        Ok(observation) if observation.success => {}
        Ok(_) => {
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "production_health_failed")
                .into_response();
        }
        Err(error) => {
            tracing::warn!(%error, "production health connection test failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "production_health_failed")
                .into_response();
        }
    }

    let github_private_key = normalize_pem(&request.github_private_key);
    let github_client = match GitHubAppAuth::new(
        request.github_app_id,
        request.github_installation_id,
        SecretString::from(github_private_key.clone()),
    ) {
        Ok(auth) => match auth.installation_client(&request.repo_name).await {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "GitHub setup installation token test failed");
                return api_error(StatusCode::UNPROCESSABLE_ENTITY, "github_connection_failed")
                    .into_response();
            }
        },
        Err(error) => {
            tracing::warn!(%error, "GitHub setup configuration failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "github_connection_failed")
                .into_response();
        }
    };
    let github_repository = match github_client
        .get_repository(&request.repo_owner, &request.repo_name)
        .await
    {
        Ok(repository) => repository,
        Err(error) => {
            tracing::warn!(%error, "GitHub setup repository lookup failed");
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "github_repository_not_accessible",
            )
            .into_response();
        }
    };

    let team_id = optional_nonempty(&request.vercel_team_id);
    let vercel = match VercelClient::new(
        SecretString::from(request.vercel_token.clone()),
        team_id.clone(),
    ) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Vercel setup configuration failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "vercel_connection_failed")
                .into_response();
        }
    };
    let vercel_project = match vercel.get_project(&request.vercel_project_id).await {
        Ok(project) => project,
        Err(error) => {
            tracing::warn!(%error, "Vercel setup project lookup failed");
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "vercel_project_not_accessible",
            )
            .into_response();
        }
    };
    let deployments = match vercel.list_deployments(&vercel_project.id, 10).await {
        Ok(deployments) => deployments,
        Err(error) => {
            tracing::warn!(%error, "Vercel setup connection test failed");
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "vercel_connection_failed")
                .into_response();
        }
    };
    let Some(initial_deployment) = deployments.into_iter().find(vercel_production_is_ready) else {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "vercel_production_deployment_not_found",
        )
        .into_response();
    };

    let provider = match configured_provider(
        &request.provider,
        request.provider_api_key.clone(),
        request.provider_model.clone(),
    ) {
        Ok(provider) => provider,
        Err(()) => {
            return api_error(StatusCode::BAD_REQUEST, "unsupported_provider").into_response();
        }
    };
    if let Err(error) = provider.test_connection().await {
        tracing::warn!(%error, "model provider setup connection test failed");
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            provider_connection_error_code(&error),
        )
        .into_response();
    }

    let encrypt = |value: Value| cipher.encrypt(&SecretString::from(value.to_string()));
    let provider_key_suffix = request
        .provider_api_key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let github_credentials = match encrypt(json!({
        "appId": request.github_app_id,
        "installationId": request.github_installation_id,
        "privateKey": github_private_key,
        "webhookSecret": request.github_webhook_secret
    })) {
        Ok(value) => value,
        Err(error) => return encryption_error(error),
    };
    let vercel_credentials = match encrypt(json!({
        "token": request.vercel_token,
        "webhookSecret": request.vercel_webhook_secret
    })) {
        Ok(value) => value,
        Err(error) => return encryption_error(error),
    };
    let provider_credentials = match encrypt(json!({ "apiKey": request.provider_api_key })) {
        Ok(value) => value,
        Err(error) => return encryption_error(error),
    };
    let slug = slugify(&request.name);
    let safety_mode = if request.safety_mode == "autopilot" {
        "autopilot_experimental".to_owned()
    } else {
        request.safety_mode
    };
    let setup = ProtectedAppSetup {
        name: request.name,
        slug,
        repo_owner: request.repo_owner.clone(),
        repo_name: request.repo_name.clone(),
        production_url: production_url.to_string(),
        health_check_url: health_check_url.to_string(),
        safety_mode,
        github_external_account_id: request.github_installation_id.to_string(),
        github_external_project_id: github_repository.full_name.clone(),
        github_credentials,
        github_metadata: json!({
            "repoId": github_repository.id,
            "baseBranch": github_repository.default_branch
        }),
        vercel_external_account_id: team_id.clone().unwrap_or_else(|| "personal".into()),
        vercel_external_project_id: vercel_project.id.clone(),
        vercel_credentials,
        vercel_metadata: json!({
            "projectName": vercel_project.name,
            "teamId": team_id
        }),
        provider_external_account_id: request.provider.clone(),
        provider_credentials,
        provider_metadata: json!({
            "provider": request.provider,
            "model": request.provider_model,
            "keySuffix": provider_key_suffix
        }),
        initial_deployment_id: initial_deployment.id,
        initial_deployment_url: https_url(&initial_deployment.url),
        initial_deployment_sha: initial_deployment
            .meta
            .get("githubCommitSha")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
    };
    match database.create_protected_app(&setup).await {
        Ok(project_id) => (
            StatusCode::CREATED,
            Json(json!({ "protected": true, "projectId": project_id })),
        )
            .into_response(),
        Err(nopager_db::DatabaseError::ProjectAlreadyExists) => {
            api_error(StatusCode::CONFLICT, "app_already_protected").into_response()
        }
        Err(error) => {
            tracing::error!(%error, "failed to create protected app");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response()
        }
    }
}

fn valid_setup(request: &ProtectAppRequest) -> bool {
    !request.name.trim().is_empty()
        && request.name.len() <= 100
        && !request.repo_owner.trim().is_empty()
        && !request.repo_name.trim().is_empty()
        && request.github_app_id > 0
        && request.github_installation_id > 0
        && !request.github_private_key.trim().is_empty()
        && request.github_webhook_secret.len() >= 16
        && !request.vercel_project_id.trim().is_empty()
        && !request.vercel_token.trim().is_empty()
        && matches!(request.provider.as_str(), "openai" | "anthropic" | "gemini")
        && !request.provider_api_key.trim().is_empty()
        && !request.provider_model.trim().is_empty()
        && matches!(request.safety_mode.as_str(), "safe" | "autopilot")
}

fn normalize_pem(value: &str) -> String {
    value.trim().replace(
        "\\n", "
",
    )
}

fn vercel_production_is_ready(deployment: &nopager_connectors::vercel::Deployment) -> bool {
    deployment.target.as_deref() == Some("production")
        && matches!(
            deployment
                .ready_state
                .as_deref()
                .or(deployment.state.as_deref()),
            Some("READY")
        )
}

fn optional_nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn slugify(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn https_url(value: &str) -> String {
    if value.starts_with("https://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

fn encryption_error(error: nopager_crypto::CryptoError) -> Response {
    tracing::error!(%error, "failed to encrypt setup credentials");
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response()
}

async fn create_admin(
    State(state): State<ServerState>,
    Json(credentials): Json<AdminCredentials>,
) -> Response {
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured")
            .into_response();
    };
    if !valid_credentials(&credentials) {
        return api_error(StatusCode::BAD_REQUEST, "invalid_credentials").into_response();
    }
    let password_hash = match hash_password(credentials.password).await {
        Ok(hash) => hash,
        Err(error) => {
            tracing::error!(%error, "failed to hash administrator password");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
        }
    };
    match database
        .create_local_admin(&credentials.username, &password_hash)
        .await
    {
        Ok(admin_id) => issue_session(database, admin_id).await,
        Err(nopager_db::DatabaseError::AdminAlreadyExists) => {
            api_error(StatusCode::CONFLICT, "admin_already_exists").into_response()
        }
        Err(error) => {
            tracing::error!(%error, "failed to create local administrator");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response()
        }
    }
}

async fn login(
    State(state): State<ServerState>,
    Json(credentials): Json<AdminCredentials>,
) -> Response {
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured")
            .into_response();
    };
    if credentials.password.is_empty() || credentials.password.len() > 1024 {
        return api_error(StatusCode::UNAUTHORIZED, "invalid_login").into_response();
    }
    let stored = match database
        .local_admin_credentials(&credentials.username)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "failed to load local administrator");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
        }
    };
    let Some((admin_id, password_hash)) = stored else {
        let _ = hash_password(credentials.password).await;
        return api_error(StatusCode::UNAUTHORIZED, "invalid_login").into_response();
    };
    match verify_password(credentials.password, password_hash).await {
        Ok(true) => issue_session(database, admin_id).await,
        Ok(false) => api_error(StatusCode::UNAUTHORIZED, "invalid_login").into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to verify administrator password");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response()
        }
    }
}

async fn pause(State(state): State<ServerState>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    set_paused(&state, &headers, true).await
}

async fn resume(State(state): State<ServerState>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    set_paused(&state, &headers, false).await
}

async fn approve_incident(
    State(state): State<ServerState>,
    Path(incident_id): Path<uuid::Uuid>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.approve_incident(incident_id, "api-admin").await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({ "approved": true, "incidentId": incident_id })),
        ),
        Err(nopager_db::DatabaseError::ProtectionPaused) => {
            api_error(StatusCode::CONFLICT, "protection_paused")
        }
        Err(nopager_db::DatabaseError::IncidentNotAwaitingApproval) => {
            api_error(StatusCode::CONFLICT, "incident_not_waiting_approval")
        }
        Err(error) => {
            tracing::error!(%incident_id, %error, "failed to approve incident");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn reject_incident(
    State(state): State<ServerState>,
    Path(incident_id): Path<uuid::Uuid>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !authorized(&state, &headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.reject_incident(incident_id, "api-admin").await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "rejected": true, "incidentId": incident_id })),
        ),
        Err(nopager_db::DatabaseError::IncidentNotAwaitingApproval) => {
            api_error(StatusCode::CONFLICT, "incident_not_waiting_approval")
        }
        Err(error) => {
            tracing::error!(%incident_id, %error, "failed to reject incident repair");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn set_paused(
    state: &ServerState,
    headers: &HeaderMap,
    paused: bool,
) -> (StatusCode, Json<Value>) {
    if !authorized(state, headers).await {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database.set_protection_paused(paused, "api-admin").await {
        Ok(updated) => (
            StatusCode::OK,
            Json(json!({ "protectionPaused": paused, "updatedProjects": updated })),
        ),
        Err(error) => {
            tracing::error!(%error, "failed to update protection state");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

async fn authorized(state: &ServerState, headers: &HeaderMap) -> bool {
    use subtle::ConstantTimeEq;
    if let (Some(expected), Some(value)) = (
        &state.admin_token,
        header(headers, "authorization").and_then(|value| value.strip_prefix("Bearer ")),
    ) && bool::from(value.as_bytes().ct_eq(expected.as_ref()))
    {
        return true;
    }
    let Some(token) = cookie(headers, "nopager_session") else {
        return false;
    };
    let Some(database) = &state.database else {
        return false;
    };
    let hash = Sha256::digest(token.as_bytes());
    database.admin_session_valid(&hash).await.unwrap_or(false)
}

fn valid_credentials(credentials: &AdminCredentials) -> bool {
    (3..=64).contains(&credentials.username.len())
        && credentials
            .username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && (12..=1024).contains(&credentials.password.len())
}

async fn hash_password(password: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn verify_password(password: String, encoded: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&encoded).map_err(|error| error.to_string())?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn issue_session(database: &Database, admin_id: uuid::Uuid) -> Response {
    let mut token = [0_u8; 32];
    rand::rng().fill_bytes(&mut token);
    let token = URL_SAFE_NO_PAD.encode(token);
    let hash = Sha256::digest(token.as_bytes());
    let expires_at = OffsetDateTime::now_utc() + TimeDuration::hours(24);
    if let Err(error) = database
        .create_admin_session(admin_id, &hash, expires_at)
        .await
    {
        tracing::error!(%error, "failed to create administrator session");
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
    }
    let mut headers = HeaderMap::new();
    let secure = std::env::var("NOPAGER_COOKIE_SECURE").is_ok_and(|value| value == "true");
    let cookie = format!(
        "nopager_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400{}",
        if secure { "; Secure" } else { "" }
    );
    headers.insert(
        SET_COOKIE,
        cookie.parse().expect("session cookie is a valid header"),
    );
    (
        StatusCode::CREATED,
        headers,
        Json(json!({ "authenticated": true })),
    )
        .into_response()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    header(headers, "cookie")?
        .split(';')
        .map(str::trim)
        .find_map(|pair| {
            pair.split_once('=')
                .filter(|(key, _)| *key == name)
                .map(|(_, value)| value)
        })
}

async fn github_webhook(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(secret) = webhook_secret(&state, "github").await else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "integration_not_configured",
        );
    };
    let Some(signature) = header(&headers, "x-hub-signature-256") else {
        return api_error(StatusCode::UNAUTHORIZED, "invalid_signature");
    };
    if verify_github(&secret, &body, signature).is_err() {
        return api_error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    let Some(delivery_id) = header(&headers, "x-github-delivery") else {
        return api_error(StatusCode::BAD_REQUEST, "missing_delivery_id");
    };
    let event_type = header(&headers, "x-github-event").unwrap_or("unknown");
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "invalid_json"),
    };
    accept_webhook(&state, "github", delivery_id, event_type, payload).await
}

async fn vercel_webhook(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(secret) = webhook_secret(&state, "vercel").await else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "integration_not_configured",
        );
    };
    let Some(signature) = header(&headers, "x-vercel-signature") else {
        return api_error(StatusCode::UNAUTHORIZED, "invalid_signature");
    };
    if verify_vercel(&secret, &body, signature).is_err() {
        return api_error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "invalid_json"),
    };
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    // Vercel does not provide a universal delivery-id header. Its HMAC covers
    // the raw body, making the signature a stable, tamper-resistant dedup key.
    accept_webhook(&state, "vercel", signature, &event_type, payload).await
}

async fn webhook_secret(state: &ServerState, kind: &str) -> Option<Vec<u8>> {
    let configured = match kind {
        "github" => &state.github_webhook_secret,
        "vercel" => &state.vercel_webhook_secret,
        _ => return None,
    };
    if let Some(secret) = configured {
        return (!secret.is_empty()).then(|| secret.to_vec());
    }
    let database = state.database.as_ref()?;
    let cipher = state.secret_cipher.as_ref()?;
    let integration = database.single_integration_secret(kind).await.ok()??;
    let plaintext = cipher.decrypt(&integration.encrypted_credentials).ok()?;
    let credentials: Value = serde_json::from_str(plaintext.expose_secret()).ok()?;
    credentials
        .get("webhookSecret")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| value.as_bytes().to_vec())
}

async fn accept_webhook(
    state: &ServerState,
    provider: &str,
    delivery_id: &str,
    event_type: &str,
    payload: Value,
) -> (StatusCode, Json<Value>) {
    let Some(database) = &state.database else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "database_not_configured");
    };
    match database
        .accept_webhook(
            provider,
            delivery_id,
            event_type,
            &json!({ "eventType": event_type, "payload": payload }),
        )
        .await
    {
        Ok(true) => (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))),
        Ok(false) => (
            StatusCode::OK,
            Json(json!({ "accepted": true, "duplicate": true })),
        ),
        Err(error) => {
            tracing::error!(%provider, %event_type, %error, "failed to persist webhook");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

fn api_error(status: StatusCode, code: &'static str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": code })))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();

    let config = ServerConfig::from_env()?;
    let database = match std::env::var("DATABASE_URL") {
        Ok(url) => {
            let database = Database::connect(&url).await?;
            database.migrate().await?;
            Some(database)
        }
        Err(_) => {
            tracing::warn!(
                "DATABASE_URL is not set; API readiness and persistent features are disabled"
            );
            None
        }
    };
    let state = ServerState {
        database,
        github_webhook_secret: secret_from_env("GITHUB_WEBHOOK_SECRET"),
        vercel_webhook_secret: secret_from_env("VERCEL_WEBHOOK_SECRET"),
        admin_token: secret_from_env("NOPAGER_ADMIN_TOKEN"),
        secret_cipher: match std::env::var("NOPAGER_MASTER_KEY") {
            Ok(key) => Some(Arc::new(SecretCipher::from_base64_key(
                &SecretString::from(key),
            )?)),
            Err(_) => {
                tracing::warn!(
                    "NOPAGER_MASTER_KEY is not set; setup credential storage is disabled"
                );
                None
            }
        },
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/overview", get(overview))
        .route("/api/v1/incidents", get(incidents))
        .route("/api/v1/incidents/{id}", get(incident_detail))
        .route("/api/v1/settings", get(settings))
        .route("/api/v1/setup/status", get(setup_status))
        .route("/api/v1/setup/admin", post(create_admin))
        .route("/api/v1/setup/test/github", post(test_github_connection))
        .route("/api/v1/setup/test/vercel", post(test_vercel_connection))
        .route(
            "/api/v1/setup/test/provider",
            post(test_provider_connection),
        )
        .route("/api/v1/setup/models", post(discover_provider_models))
        .route("/api/v1/setup/test/health", post(test_health_connection))
        .route(
            "/api/v1/setup/discover/health",
            post(discover_health_connection),
        )
        .route("/api/v1/setup/app", post(protect_app))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/protection/pause", post(pause))
        .route("/api/v1/protection/resume", post(resume))
        .route("/api/v1/safety/mode", post(set_safety_mode))
        .route("/api/v1/incidents/{id}/approve", post(approve_incident))
        .route("/api/v1/incidents/{id}/reject", post(reject_incident))
        .route("/api/v1/integrations/github/webhook", post(github_webhook))
        .route("/api/v1/integrations/vercel/webhook", post(vercel_webhook))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.address).await?;
    info!(address = %config.address, "NoPager API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn secret_from_env(name: &str) -> Option<Arc<[u8]>> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| Arc::from(value.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ServerState {
        ServerState {
            database: None,
            github_webhook_secret: None,
            vercel_webhook_secret: None,
            admin_token: Some(Arc::from(b"test-admin-token".as_slice())),
            secret_cipher: None,
        }
    }

    #[test]
    fn health_discovery_is_small_deterministic_and_same_origin() {
        let production = Url::parse("https://example.com/app?tenant=alpha#section").unwrap();
        let candidates = health_discovery_candidates(&production);
        assert_eq!(
            candidates.iter().map(Url::as_str).collect::<Vec<_>>(),
            vec![
                "https://example.com/app?tenant=alpha",
                "https://example.com/health",
                "https://example.com/healthz",
                "https://example.com/api/health",
                "https://example.com/api/healthz",
            ]
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.scheme() == "https" && candidate.host_str() == Some("example.com")
        }));
    }

    #[test]
    fn health_discovery_deduplicates_exact_health_path() {
        let production = Url::parse("https://example.com/health").unwrap();
        let candidates = health_discovery_candidates(&production);
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0].as_str(), "https://example.com/health");
    }

    #[tokio::test]
    async fn mutation_auth_requires_exact_bearer_token() {
        let mut headers = HeaderMap::new();
        assert!(!authorized(&state(), &headers).await);
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(!authorized(&state(), &headers).await);
        headers.insert("authorization", "Bearer test-admin-token".parse().unwrap());
        assert!(authorized(&state(), &headers).await);
    }
}
