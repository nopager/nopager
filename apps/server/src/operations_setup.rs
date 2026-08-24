use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use nopager_monitor::{check_http, validate_health_url};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::json;
use url::Url;
use uuid::Uuid;

use super::{ServerState, api_error, authorized, configured_provider};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProtectOperationsAppRequest {
    name: String,
    provider: String,
    provider_api_key: String,
    provider_model: String,
    production_url: String,
    health_check_url: String,
    docker_target: String,
    safety_mode: String,
}

pub(super) async fn protect_operations_app(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<ProtectOperationsAppRequest>,
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

    if !valid_request(&request) {
        return api_error(StatusCode::BAD_REQUEST, "invalid_operations_setup").into_response();
    }

    let production_url = match Url::parse(&request.production_url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "unsafe_production_url").into_response(),
    };
    let health_check_url = match Url::parse(&request.health_check_url) {
        Ok(url) if validate_health_url(&url).is_ok() => url,
        _ => {
            return api_error(StatusCode::BAD_REQUEST, "unsafe_health_check_url")
                .into_response();
        }
    };

    match check_http(
        &health_check_url,
        200,
        std::time::Duration::from_secs(10),
    )
    .await
    {
        Ok(observation) if observation.success => {}
        Ok(_) | Err(_) => {
            return api_error(StatusCode::UNPROCESSABLE_ENTITY, "production_health_failed")
                .into_response();
        }
    }

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
        tracing::warn!(%error, "operations setup model provider test failed");
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            super::provider_connection_error_code(&error),
        )
        .into_response();
    }

    let provider_key_suffix = request
        .provider_api_key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let provider_credentials = match cipher.encrypt(&SecretString::from(
        json!({ "apiKey": &request.provider_api_key }).to_string(),
    )) {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "failed to encrypt operations provider credentials");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
                .into_response();
        }
    };
    let docker_credentials = match cipher.encrypt(&SecretString::from("{}".to_owned())) {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "failed to encrypt operations connector placeholder");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
                .into_response();
        }
    };

    let safety_mode = if request.safety_mode == "autopilot" {
        "autopilot_experimental"
    } else {
        request.safety_mode.as_str()
    };
    let project_id = Uuid::now_v7();
    let health_id = Uuid::now_v7();
    let policy_id = Uuid::now_v7();
    let provider_id = Uuid::now_v7();
    let docker_id = Uuid::now_v7();
    let audit_id = Uuid::now_v7();

    let mut tx = match database.pool().begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(%error, "failed to begin operations setup transaction");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
                .into_response();
        }
    };

    if let Err(error) = sqlx::query("SELECT pg_advisory_xact_lock(7061676573)")
        .execute(&mut *tx)
        .await
    {
        tracing::error!(%error, "failed to acquire operations setup lock");
        let _ = tx.rollback().await;
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
    }
    let exists = match sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM projects)")
        .fetch_one(&mut *tx)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "failed to inspect existing protected app");
            let _ = tx.rollback().await;
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
                .into_response();
        }
    };
    if exists {
        let _ = tx.rollback().await;
        return api_error(StatusCode::CONFLICT, "app_already_protected").into_response();
    }

    let setup_result: Result<(), sqlx::Error> = async {
        sqlx::query(
            "INSERT INTO projects (id, name, slug, repo_owner, repo_name, production_url, safety_mode) VALUES ($1, $2, 'operations-only', '', '', $3, $4)",
        )
        .bind(project_id)
        .bind(&request.name)
        .bind(production_url.as_str())
        .bind(safety_mode)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO integrations (id, project_id, type, external_account_id, encrypted_credentials, metadata_json, status) VALUES ($1, $2, 'model_provider', $3, $4, $5, 'CONNECTED')",
        )
        .bind(provider_id)
        .bind(project_id)
        .bind(&request.provider)
        .bind(&provider_credentials)
        .bind(json!({
            "provider": &request.provider,
            "model": &request.provider_model,
            "keySuffix": provider_key_suffix
        }))
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO integrations (id, project_id, type, external_account_id, external_project_id, encrypted_credentials, metadata_json, status) VALUES ($1, $2, 'docker_ops', 'local-docker', $3, $4, $5, 'CONNECTED')",
        )
        .bind(docker_id)
        .bind(project_id)
        .bind(&request.docker_target)
        .bind(&docker_credentials)
        .bind(json!({
            "targetId": &request.docker_target,
            "allowedActions": ["restart_container"],
            "runtimePreflightRequired": true
        }))
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO health_checks (id, project_id, url, status, consecutive_successes, last_checked_at) VALUES ($1, $2, $3, 'HEALTHY', 1, now())",
        )
        .bind(health_id)
        .bind(project_id)
        .bind(health_check_url.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO policies (id, project_id, safety_mode, allowed_actions_json, required_checks_json) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(policy_id)
        .bind(project_id)
        .bind(safety_mode)
        .bind(json!(["observe_only", "restart_container", "escalate"]))
        .bind(json!(["external_http", "container_status"]))
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO audit_events (id, project_id, actor, action, target, outcome, metadata_json) VALUES ($1, $2, 'api-admin', 'protect_operations_app', $3, 'success', $4)",
        )
        .bind(audit_id)
        .bind(project_id)
        .bind(&request.docker_target)
        .bind(json!({
            "mode": "operations_only",
            "healthUrl": health_check_url.as_str(),
            "safetyMode": safety_mode,
            "restartCapability": true
        }))
        .execute(&mut *tx)
        .await?;
        Ok(())
    }
    .await;

    if let Err(error) = setup_result {
        tracing::error!(%error, "failed to persist operations-only protected app");
        let _ = tx.rollback().await;
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
    }
    if let Err(error) = tx.commit().await {
        tracing::error!(%error, "failed to commit operations-only protected app");
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error").into_response();
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "protected": true,
            "mode": "operations_only",
            "projectId": project_id,
            "dockerTarget": request.docker_target,
            "safetyMode": safety_mode
        })),
    )
        .into_response()
}

fn valid_request(request: &ProtectOperationsAppRequest) -> bool {
    !request.name.trim().is_empty()
        && request.name.chars().count() <= 100
        && valid_docker_target(&request.docker_target)
        && matches!(request.provider.as_str(), "openai" | "anthropic" | "gemini")
        && !request.provider_api_key.trim().is_empty()
        && !request.provider_model.trim().is_empty()
        && matches!(request.safety_mode.as_str(), "safe" | "autopilot")
}

fn valid_docker_target(value: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(target: &str) -> ProtectOperationsAppRequest {
        ProtectOperationsAppRequest {
            name: "test-app".into(),
            provider: "openai".into(),
            provider_api_key: "test-key".into(),
            provider_model: "test-model".into(),
            production_url: "https://example.com".into(),
            health_check_url: "https://example.com/health".into(),
            docker_target: target.into(),
            safety_mode: "safe".into(),
        }
    }

    #[test]
    fn accepts_only_bounded_docker_target_identifiers() {
        assert!(valid_request(&request("checkout-api_1")));
        for invalid in ["", "--host=x", "web;rm", "../web", "/web", "web container"] {
            assert!(!valid_request(&request(invalid)), "{invalid}");
        }
    }
}
