use std::time::Duration;

use nopager_core::IncidentState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use time::OffsetDateTime;
use tracing::instrument;
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

pub struct IncidentTransition {
    pub project_id: Uuid,
    pub incident_id: Uuid,
    pub expected: IncidentState,
    pub next: IncidentState,
    pub actor: String,
    pub message: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HealthCheck {
    pub id: Uuid,
    pub project_id: Uuid,
    pub url: String,
    pub timeout_ms: i32,
    pub expected_status: i32,
}

pub struct IncidentTrigger {
    pub project_id: Uuid,
    pub deduplication_key: String,
    pub trigger_type: String,
    pub severity: String,
    pub title: String,
    pub metadata: Value,
}

pub struct ProtectedAppSetup {
    pub name: String,
    pub slug: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub production_url: String,
    pub health_check_url: String,
    pub safety_mode: String,
    pub github_external_account_id: String,
    pub github_external_project_id: String,
    pub github_credentials: String,
    pub github_metadata: Value,
    pub vercel_external_account_id: String,
    pub vercel_external_project_id: String,
    pub vercel_credentials: String,
    pub vercel_metadata: Value,
    pub provider_external_account_id: String,
    pub provider_credentials: String,
    pub provider_metadata: Value,
    pub initial_deployment_id: String,
    pub initial_deployment_url: String,
    pub initial_deployment_sha: String,
}

#[derive(Debug, Clone)]
pub struct IncidentWork {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub state: IncidentState,
    pub trigger_context: Value,
    pub deployment_context: Value,
    pub project_name: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub production_url: String,
    pub health_check_url: String,
    pub safety_mode: String,
    pub protection_paused: bool,
    pub github_metadata: Value,
    pub vercel_project_id: Option<String>,
    pub vercel_metadata: Value,
}

#[derive(Debug, Clone)]
pub struct RepairAttemptWork {
    pub id: Uuid,
    pub incident_id: Uuid,
    pub attempt_number: i32,
    pub base_commit_sha: String,
    pub diagnosis: Value,
    pub proposal: Value,
    pub patch_diff: String,
    pub repair_branch: Option<String>,
    pub repair_commit_sha: Option<String>,
    pub preview_deployment_id: Option<String>,
    pub preview_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IntegrationSecret {
    pub encrypted_credentials: String,
    pub metadata: Value,
}

#[derive(sqlx::FromRow)]
struct RepairAttemptRow {
    incident_id: Uuid,
    attempt_number: i32,
    base_commit_sha: String,
    diagnosis_json: Option<Value>,
    plan_json: Option<Value>,
    patch_diff: Option<String>,
    repair_branch: Option<String>,
    repair_commit_sha: Option<String>,
    preview_deployment_id: Option<String>,
    preview_url: Option<String>,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn ready(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn admin_exists(&self) -> Result<bool, DatabaseError> {
        Ok(
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM local_admins)")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn create_local_admin(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<Uuid, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(7061676572)")
            .execute(&mut *tx)
            .await?;
        let exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM local_admins)")
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            return Err(DatabaseError::AdminAlreadyExists);
        }
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO local_admins (id, username, password_hash) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(username)
            .bind(password_hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn local_admin_credentials(
        &self,
        username: &str,
    ) -> Result<Option<(Uuid, String)>, DatabaseError> {
        Ok(
            sqlx::query_as("SELECT id, password_hash FROM local_admins WHERE username = $1")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn create_admin_session(
        &self,
        admin_id: Uuid,
        token_hash: &[u8],
        expires_at: OffsetDateTime,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO admin_sessions (token_hash, admin_id, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(token_hash)
        .bind(admin_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn admin_session_valid(&self, token_hash: &[u8]) -> Result<bool, DatabaseError> {
        Ok(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin_sessions WHERE token_hash = $1 AND expires_at > now())")
            .bind(token_hash).fetch_one(&self.pool).await?)
    }

    pub async fn project_exists(&self) -> Result<bool, DatabaseError> {
        Ok(
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM projects)")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn create_protected_app(
        &self,
        setup: &ProtectedAppSetup,
    ) -> Result<Uuid, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(7061676573)")
            .execute(&mut *tx)
            .await?;
        let exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM projects)")
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            return Err(DatabaseError::ProjectAlreadyExists);
        }

        let project_id = Uuid::now_v7();
        sqlx::query("INSERT INTO projects (id, name, slug, repo_owner, repo_name, production_url, safety_mode) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(project_id)
            .bind(&setup.name)
            .bind(&setup.slug)
            .bind(&setup.repo_owner)
            .bind(&setup.repo_name)
            .bind(&setup.production_url)
            .bind(&setup.safety_mode)
            .execute(&mut *tx)
            .await?;

        for (kind, account, external_project, credentials, metadata) in [
            (
                "github",
                &setup.github_external_account_id,
                Some(&setup.github_external_project_id),
                &setup.github_credentials,
                &setup.github_metadata,
            ),
            (
                "vercel",
                &setup.vercel_external_account_id,
                Some(&setup.vercel_external_project_id),
                &setup.vercel_credentials,
                &setup.vercel_metadata,
            ),
            (
                "model_provider",
                &setup.provider_external_account_id,
                None,
                &setup.provider_credentials,
                &setup.provider_metadata,
            ),
        ] {
            sqlx::query("INSERT INTO integrations (id, project_id, type, external_account_id, external_project_id, encrypted_credentials, metadata_json, status) VALUES ($1, $2, $3, $4, $5, $6, $7, 'CONNECTED')")
                .bind(Uuid::now_v7())
                .bind(project_id)
                .bind(kind)
                .bind(account)
                .bind(external_project)
                .bind(credentials)
                .bind(metadata)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("INSERT INTO health_checks (id, project_id, url, status, consecutive_successes, last_checked_at) VALUES ($1, $2, $3, 'HEALTHY', 1, now())")
            .bind(Uuid::now_v7())
            .bind(project_id)
            .bind(&setup.health_check_url)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO policies (id, project_id, safety_mode, allowed_actions_json, required_checks_json) VALUES ($1, $2, $3, $4, $5)")
            .bind(Uuid::now_v7())
            .bind(project_id)
            .bind(&setup.safety_mode)
            .bind(serde_json::json!(["diagnose", "repair", "preview", "rollback"]))
            .bind(serde_json::json!(["build", "test", "preview_health"]))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO deployments (id, project_id, provider_deployment_id, environment, commit_sha, url, status, known_good) VALUES ($1, $2, $3, 'production', $4, $5, 'READY', true)")
            .bind(Uuid::now_v7())
            .bind(project_id)
            .bind(&setup.initial_deployment_id)
            .bind(&setup.initial_deployment_sha)
            .bind(&setup.initial_deployment_url)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_events (id, project_id, actor, action, target, outcome, metadata_json) VALUES ($1, $2, 'local-admin', 'protect_app', $3, 'success', $4)")
            .bind(Uuid::now_v7())
            .bind(project_id)
            .bind(&setup.slug)
            .bind(serde_json::json!({ "safetyMode": setup.safety_mode }))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(project_id)
    }

    pub async fn integration_secret(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<IntegrationSecret, DatabaseError> {
        let (encrypted_credentials, metadata) = sqlx::query_as::<_, (String, Value)>(
            "SELECT encrypted_credentials, metadata_json FROM integrations WHERE project_id = $1 AND type = $2 AND status = 'CONNECTED'",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_one(&self.pool)
        .await?;
        Ok(IntegrationSecret {
            encrypted_credentials,
            metadata,
        })
    }

    pub async fn single_integration_secret(
        &self,
        kind: &str,
    ) -> Result<Option<IntegrationSecret>, DatabaseError> {
        Ok(sqlx::query_as::<_, (String, Value)>(
            "SELECT encrypted_credentials, metadata_json FROM integrations WHERE type = $1 AND status = 'CONNECTED' ORDER BY created_at LIMIT 1",
        )
        .bind(kind)
        .fetch_optional(&self.pool)
        .await?
        .map(|(encrypted_credentials, metadata)| IntegrationSecret {
            encrypted_credentials,
            metadata,
        }))
    }

    pub async fn enqueue_due_health_checks(&self, limit: i64) -> Result<u64, DatabaseError> {
        let checks = sqlx::query_as::<_, (Uuid, i32)>(
            "SELECT id, interval_seconds FROM health_checks WHERE last_checked_at IS NULL OR last_checked_at + (interval_seconds * interval '1 second') <= now() ORDER BY last_checked_at NULLS FIRST LIMIT $1",
        )
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        for (id, interval) in &checks {
            let bucket = now / i64::from(*interval);
            self.enqueue(
                JobType::HealthCheck,
                &format!("health:{id}:{bucket}"),
                None,
                &serde_json::json!({ "healthCheckId": id }),
                3,
            )
            .await?;
        }
        Ok(checks.len() as u64)
    }

    pub async fn health_check(&self, id: Uuid) -> Result<HealthCheck, DatabaseError> {
        Ok(sqlx::query_as::<_, HealthCheck>(
            "SELECT id, project_id, url, timeout_ms, expected_status FROM health_checks WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn record_health_observation(
        &self,
        check: &HealthCheck,
        success: bool,
        observation: &Value,
    ) -> Result<Option<Uuid>, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        let (failures, successes, old_status): (i32, i32, String) = sqlx::query_as(
            "SELECT consecutive_failures, consecutive_successes, status FROM health_checks WHERE id = $1 FOR UPDATE",
        )
        .bind(check.id)
        .fetch_one(&mut *tx)
        .await?;
        let (failures, successes, status) = if success {
            let successes = successes.saturating_add(1);
            (
                0,
                successes,
                if old_status == "DOWN" && successes < 2 {
                    "DOWN"
                } else {
                    "HEALTHY"
                },
            )
        } else {
            let failures = failures.saturating_add(1);
            (failures, 0, if failures >= 3 { "DOWN" } else { "FAILING" })
        };
        sqlx::query("UPDATE health_checks SET consecutive_failures = $1, consecutive_successes = $2, status = $3, last_checked_at = now() WHERE id = $4")
            .bind(failures).bind(successes).bind(status).bind(check.id).execute(&mut *tx).await?;

        let incident_id = if status == "DOWN" && old_status != "DOWN" {
            let trigger = IncidentTrigger {
                project_id: check.project_id,
                deduplication_key: format!("health:{}", check.id),
                trigger_type: "HEALTH_CHECK".into(),
                severity: "high".into(),
                title: format!("Production health check failed: {}", check.url),
                metadata: observation.clone(),
            };
            open_incident_tx(&mut tx, &trigger).await?
        } else {
            None
        };
        tx.commit().await?;
        Ok(incident_id)
    }

    pub async fn open_incident(
        &self,
        trigger: &IncidentTrigger,
    ) -> Result<Option<Uuid>, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        let id = open_incident_tx(&mut tx, trigger).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn webhook_metadata(
        &self,
        provider: &str,
        delivery_id: &str,
    ) -> Result<Value, DatabaseError> {
        Ok(sqlx::query_scalar("SELECT metadata_json FROM webhook_deliveries WHERE provider = $1 AND external_delivery_id = $2")
            .bind(provider).bind(delivery_id).fetch_one(&self.pool).await?)
    }

    pub async fn project_for_repository(
        &self,
        owner: &str,
        repository: &str,
    ) -> Result<Option<Uuid>, DatabaseError> {
        Ok(sqlx::query_scalar("SELECT id FROM projects WHERE repo_owner = $1 AND repo_name = $2 AND status = 'ACTIVE'")
            .bind(owner).bind(repository).fetch_optional(&self.pool).await?)
    }

    pub async fn project_for_integration(
        &self,
        kind: &str,
        external_project_id: &str,
    ) -> Result<Option<Uuid>, DatabaseError> {
        Ok(sqlx::query_scalar("SELECT project_id FROM integrations WHERE type = $1 AND external_project_id = $2 AND status = 'CONNECTED'")
            .bind(kind).bind(external_project_id).fetch_optional(&self.pool).await?)
    }

    pub async fn incident_work(&self, id: Uuid) -> Result<IncidentWork, DatabaseError> {
        let (project_id, title, status, project_name, repo_owner, repo_name, production_url, safety_mode, protection_paused): (Uuid, String, String, String, String, String, String, String, bool) =
            sqlx::query_as("SELECT i.project_id, i.title, i.status, p.name, p.repo_owner, p.repo_name, p.production_url, p.safety_mode, p.protection_paused FROM incidents i JOIN projects p ON p.id = i.project_id WHERE i.id = $1")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        let trigger_context = sqlx::query_scalar::<_, Value>(
            "SELECT metadata_json FROM incident_events WHERE incident_id = $1 ORDER BY created_at, id LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or_else(|| serde_json::json!({}));
        let deployment_context = sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(jsonb_agg(row_to_json(d) ORDER BY d.created_at DESC), '[]'::jsonb) FROM (SELECT provider_deployment_id, environment, commit_sha, url, status, known_good, created_at FROM deployments WHERE project_id = $1 ORDER BY created_at DESC LIMIT 5) d",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let health_check_url = sqlx::query_scalar::<_, String>(
            "SELECT url FROM health_checks WHERE project_id = $1 ORDER BY id LIMIT 1",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let github_metadata = sqlx::query_scalar::<_, Value>(
            "SELECT metadata_json FROM integrations WHERE project_id = $1 AND type = 'github' AND status = 'CONNECTED'",
        ).bind(project_id).fetch_optional(&self.pool).await?.unwrap_or_else(|| serde_json::json!({}));
        let vercel = sqlx::query_as::<_, (Option<String>, Value)>(
            "SELECT external_project_id, metadata_json FROM integrations WHERE project_id = $1 AND type = 'vercel' AND status = 'CONNECTED'",
        ).bind(project_id).fetch_optional(&self.pool).await?;
        let (vercel_project_id, vercel_metadata) = vercel.unwrap_or((None, serde_json::json!({})));
        Ok(IncidentWork {
            id,
            project_id,
            title,
            state: parse_state(&status)?,
            trigger_context,
            deployment_context,
            project_name,
            repo_owner,
            repo_name,
            production_url,
            health_check_url,
            safety_mode,
            protection_paused,
            github_metadata,
            vercel_project_id,
            vercel_metadata,
        })
    }

    pub async fn repair_attempt(&self, id: Uuid) -> Result<RepairAttemptWork, DatabaseError> {
        let row = sqlx::query_as::<_, RepairAttemptRow>(
            "SELECT incident_id, attempt_number, base_commit_sha, diagnosis_json, plan_json, patch_diff, repair_branch, repair_commit_sha, preview_deployment_id, preview_url FROM repair_attempts WHERE id = $1",
        ).bind(id).fetch_one(&self.pool).await?;
        Ok(RepairAttemptWork {
            id,
            incident_id: row.incident_id,
            attempt_number: row.attempt_number,
            base_commit_sha: row.base_commit_sha,
            diagnosis: row
                .diagnosis_json
                .ok_or(DatabaseError::IncompleteRepairAttempt)?,
            proposal: row
                .plan_json
                .ok_or(DatabaseError::IncompleteRepairAttempt)?,
            patch_diff: row
                .patch_diff
                .ok_or(DatabaseError::IncompleteRepairAttempt)?,
            repair_branch: row.repair_branch,
            repair_commit_sha: row.repair_commit_sha,
            preview_deployment_id: row.preview_deployment_id,
            preview_url: row.preview_url,
        })
    }

    pub async fn create_repair_attempt(
        &self,
        incident_id: Uuid,
        base_sha: &str,
        diagnosis: &Value,
    ) -> Result<Uuid, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        if let Some(id) = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT current_attempt_id FROM incidents WHERE id = $1",
        )
        .bind(incident_id)
        .fetch_one(&mut *tx)
        .await?
        {
            tx.commit().await?;
            return Ok(id);
        }
        let number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(attempt_number), 0) + 1 FROM repair_attempts WHERE incident_id = $1",
        )
        .bind(incident_id)
        .fetch_one(&mut *tx)
        .await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO repair_attempts (id, incident_id, attempt_number, base_commit_sha, diagnosis_json, risk_level, status) VALUES ($1, $2, $3, $4, $5, $6, 'DIAGNOSED')")
            .bind(id).bind(incident_id).bind(number).bind(base_sha).bind(diagnosis)
            .bind(diagnosis.get("riskLevel").and_then(Value::as_str)).execute(&mut *tx).await?;
        sqlx::query("UPDATE incidents SET current_attempt_id = $1 WHERE id = $2")
            .bind(id)
            .bind(incident_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn create_followup_repair_attempt(
        &self,
        incident_id: Uuid,
        base_sha: &str,
        diagnosis: &Value,
    ) -> Result<Uuid, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM incidents WHERE id = $1 FOR UPDATE")
            .bind(incident_id)
            .execute(&mut *tx)
            .await?;
        let number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(attempt_number), 0) + 1 FROM repair_attempts WHERE incident_id = $1",
        )
        .bind(incident_id)
        .fetch_one(&mut *tx)
        .await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO repair_attempts (id, incident_id, attempt_number, base_commit_sha, diagnosis_json, risk_level, status) VALUES ($1, $2, $3, $4, $5, $6, 'DIAGNOSED')")
            .bind(id)
            .bind(incident_id)
            .bind(number)
            .bind(base_sha)
            .bind(diagnosis)
            .bind(diagnosis.get("riskLevel").and_then(Value::as_str))
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE incidents SET current_attempt_id = $1 WHERE id = $2")
            .bind(id)
            .bind(incident_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn save_root_cause_summary(
        &self,
        incident_id: Uuid,
        summary: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE incidents SET root_cause_summary = $1 WHERE id = $2")
            .bind(summary)
            .bind(incident_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_repair_proposal(
        &self,
        attempt_id: Uuid,
        proposal: &Value,
        patch: &str,
        fingerprint: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE repair_attempts SET plan_json = $1, patch_diff = $2, patch_fingerprint = $3, status = 'PATCH_PROPOSED' WHERE id = $4")
            .bind(proposal).bind(patch).bind(fingerprint).bind(attempt_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_validation(
        &self,
        attempt_id: Uuid,
        validation: &Value,
        passed: bool,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE repair_attempts SET validation_json = $1, sandbox_status = $2, test_status = $2, status = $3 WHERE id = $4")
            .bind(validation)
            .bind(if passed { "PASSED" } else { "FAILED" })
            .bind(if passed { "VALIDATED" } else { "VALIDATION_FAILED" })
            .bind(attempt_id).execute(&self.pool).await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_pull_request(
        &self,
        attempt_id: Uuid,
        branch: &str,
        commit_sha: &str,
        number: u64,
        url: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE repair_attempts SET repair_branch = $1, repair_commit_sha = $2, pull_request_number = $3, pull_request_url = $4, status = 'PR_OPEN' WHERE id = $5")
            .bind(branch).bind(commit_sha).bind(i64::try_from(number).map_err(|_| DatabaseError::NumericOverflow)?).bind(url).bind(attempt_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_preview(
        &self,
        attempt_id: Uuid,
        deployment_id: &str,
        url: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE repair_attempts SET preview_deployment_id = $1, preview_url = $2, status = 'PREVIEW_DEPLOYED' WHERE id = $3")
            .bind(deployment_id).bind(url).bind(attempt_id).execute(&self.pool).await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_deployment(
        &self,
        project_id: Uuid,
        provider_id: &str,
        environment: &str,
        commit_sha: &str,
        url: &str,
        status: &str,
        known_good: bool,
    ) -> Result<(), DatabaseError> {
        sqlx::query("INSERT INTO deployments (id, project_id, provider_deployment_id, environment, commit_sha, url, status, known_good) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (project_id, provider_deployment_id) DO UPDATE SET environment = EXCLUDED.environment, commit_sha = EXCLUDED.commit_sha, url = EXCLUDED.url, status = EXCLUDED.status, known_good = deployments.known_good OR EXCLUDED.known_good")
            .bind(Uuid::now_v7()).bind(project_id).bind(provider_id).bind(environment).bind(commit_sha).bind(url).bind(status).bind(known_good).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn mark_deployment_known_good(
        &self,
        project_id: Uuid,
        provider_id: &str,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE deployments SET known_good = false WHERE project_id = $1 AND environment = 'production'")
            .bind(project_id).execute(&mut *tx).await?;
        let updated = sqlx::query("UPDATE deployments SET known_good = true, status = 'READY' WHERE project_id = $1 AND provider_deployment_id = $2")
            .bind(project_id).bind(provider_id).execute(&mut *tx).await?.rows_affected();
        if updated != 1 {
            return Err(DatabaseError::DeploymentNotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn latest_known_good_deployment(
        &self,
        project_id: Uuid,
    ) -> Result<Option<(String, String)>, DatabaseError> {
        Ok(sqlx::query_as("SELECT provider_deployment_id, url FROM deployments WHERE project_id = $1 AND environment = 'production' AND known_good ORDER BY created_at DESC LIMIT 1")
            .bind(project_id).fetch_optional(&self.pool).await?)
    }

    pub async fn escalate_incident(
        &self,
        incident_id: Uuid,
        reason: &str,
    ) -> Result<(), DatabaseError> {
        let work = self.incident_work(incident_id).await?;
        if matches!(
            work.state,
            IncidentState::Resolved
                | IncidentState::RolledBack
                | IncidentState::Escalated
                | IncidentState::Failed
                | IncidentState::Cancelled
        ) {
            return Ok(());
        }
        self.transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: work.state,
            next: IncidentState::Escalated,
            actor: "worker".into(),
            message: "NoPager could not safely complete the repair".into(),
            metadata: serde_json::json!({ "reason": truncate_error(reason) }),
        })
        .await
    }

    pub async fn approve_incident(
        &self,
        incident_id: Uuid,
        actor: &str,
    ) -> Result<(), DatabaseError> {
        let work = self.incident_work(incident_id).await?;
        if work.protection_paused {
            return Err(DatabaseError::ProtectionPaused);
        }
        if work.state == IncidentState::WaitingApproval {
            self.transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::WaitingApproval,
                next: IncidentState::ProductionDeploying,
                actor: actor.into(),
                message: "Production deployment approved".into(),
                metadata: serde_json::json!({}),
            })
            .await?;
        } else if work.state != IncidentState::ProductionDeploying {
            return Err(DatabaseError::IncidentNotAwaitingApproval);
        }
        let attempt_id = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT current_attempt_id FROM incidents WHERE id = $1",
        )
        .bind(incident_id)
        .fetch_one(&self.pool)
        .await?
        .ok_or(DatabaseError::IncompleteRepairAttempt)?;
        self.enqueue(
            JobType::ProductionAction,
            &format!("incident:{incident_id}:production"),
            Some(incident_id),
            &serde_json::json!({ "incidentId": incident_id, "attemptId": attempt_id, "action": "promote" }),
            5,
        )
        .await?;
        Ok(())
    }

    pub async fn reject_incident(
        &self,
        incident_id: Uuid,
        actor: &str,
    ) -> Result<(), DatabaseError> {
        let work = self.incident_work(incident_id).await?;
        if work.state != IncidentState::WaitingApproval {
            return Err(DatabaseError::IncidentNotAwaitingApproval);
        }
        self.transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::WaitingApproval,
            next: IncidentState::Cancelled,
            actor: actor.into(),
            message: "Production repair rejected by administrator".into(),
            metadata: serde_json::json!({}),
        })
        .await
    }

    pub async fn overview(&self) -> Result<Value, DatabaseError> {
        let project = sqlx::query_as::<_, (Uuid, String, String, bool)>(
            "SELECT id, name, safety_mode, protection_paused FROM projects WHERE status = 'ACTIVE' ORDER BY created_at LIMIT 1",
        ).fetch_optional(&self.pool).await?;
        let Some((project_id, name, safety_mode, paused)) = project else {
            return Ok(
                serde_json::json!({ "configured": false, "systemStatus": "UNCONFIGURED", "actionRequired": true }),
            );
        };
        let failing: i64 = sqlx::query_scalar("SELECT count(*) FROM health_checks WHERE project_id = $1 AND status IN ('FAILING', 'DOWN')")
            .bind(project_id).fetch_one(&self.pool).await?;
        let (health_check_count, last_checked_at): (i64, Option<OffsetDateTime>) = sqlx::query_as(
            "SELECT count(*), max(last_checked_at) FROM health_checks WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let (incidents_this_month, autonomous_this_month): (i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE autonomous_resolution) FROM incidents WHERE project_id = $1 AND opened_at >= date_trunc('month', now())",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let latest_deployment = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object('id', provider_deployment_id, 'environment', environment, 'commitSha', commit_sha, 'url', url, 'status', status, 'knownGood', known_good, 'createdAt', created_at) FROM deployments WHERE project_id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        let latest = sqlx::query_as::<_, (Uuid, String, String, String, OffsetDateTime)>(
            "SELECT id, title, status, severity, opened_at FROM incidents WHERE project_id = $1 ORDER BY opened_at DESC LIMIT 1",
        ).bind(project_id).fetch_optional(&self.pool).await?;
        Ok(serde_json::json!({
            "configured": true,
            "project": { "id": project_id, "name": name },
            "systemStatus": if paused { "PAUSED" } else if failing > 0 { "DEGRADED" } else { "HEALTHY" },
            "protectionMode": safety_mode,
            "protectionPaused": paused,
            "healthCheckCount": health_check_count,
            "lastCheckedAt": last_checked_at,
            "incidentsThisMonth": incidents_this_month,
            "autonomousThisMonth": autonomous_this_month,
            "latestDeployment": latest_deployment,
            "actionRequired": paused || failing > 0 || latest.as_ref().is_some_and(|(_, _, status, _, _)| status == "WAITING_APPROVAL"),
            "latestIncident": latest.map(|(id, title, status, severity, opened_at)| serde_json::json!({ "id": id, "title": title, "status": status, "severity": severity, "openedAt": opened_at })),
        }))
    }

    pub async fn incidents(&self, limit: i64) -> Result<Value, DatabaseError> {
        Ok(sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'id', i.id, 'projectId', i.project_id, 'title', i.title,
                'status', i.status, 'severity', i.severity, 'triggerType', i.trigger_type,
                'rootCauseSummary', i.root_cause_summary,
                'autonomousResolution', i.autonomous_resolution,
                'openedAt', i.opened_at, 'resolvedAt', i.resolved_at,
                'timeToRecoverySeconds', EXTRACT(EPOCH FROM (COALESCE(i.resolved_at, now()) - i.opened_at))::bigint,
                'actionRequired', i.status IN ('WAITING_APPROVAL', 'ESCALATED', 'FAILED')
             ) ORDER BY (i.resolved_at IS NOT NULL), i.opened_at DESC), '[]'::jsonb)
             FROM (SELECT * FROM incidents ORDER BY (resolved_at IS NOT NULL), opened_at DESC LIMIT $1) i",
        )
        .bind(limit.clamp(1, 200))
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn incident_detail(&self, incident_id: Uuid) -> Result<Option<Value>, DatabaseError> {
        let mut incident = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
                'id', i.id, 'projectId', i.project_id, 'projectName', p.name,
                'title', i.title, 'status', i.status, 'severity', i.severity,
                'triggerType', i.trigger_type, 'rootCauseSummary', i.root_cause_summary,
                'autonomousResolution', i.autonomous_resolution,
                'openedAt', i.opened_at, 'resolvedAt', i.resolved_at,
                'safetyMode', p.safety_mode, 'protectionPaused', p.protection_paused
             ) FROM incidents i JOIN projects p ON p.id = i.project_id WHERE i.id = $1",
        )
        .bind(incident_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(Value::Object(ref mut object)) = incident else {
            return Ok(None);
        };
        let events = sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'id', id, 'type', type, 'actor', actor, 'message', message,
                'metadata', metadata_json, 'createdAt', created_at
             ) ORDER BY created_at, id), '[]'::jsonb) FROM incident_events WHERE incident_id = $1",
        )
        .bind(incident_id)
        .fetch_one(&self.pool)
        .await?;
        let attempt = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
                'id', r.id, 'attemptNumber', r.attempt_number, 'baseCommitSha', r.base_commit_sha,
                'diagnosis', r.diagnosis_json, 'proposal', r.plan_json, 'patchDiff', r.patch_diff,
                'riskLevel', r.risk_level, 'sandboxStatus', r.sandbox_status,
                'testStatus', r.test_status, 'validation', r.validation_json,
                'repairBranch', r.repair_branch, 'repairCommitSha', r.repair_commit_sha,
                'pullRequestNumber', r.pull_request_number, 'pullRequestUrl', r.pull_request_url,
                'previewDeploymentId', r.preview_deployment_id, 'previewUrl', r.preview_url,
                'status', r.status, 'startedAt', r.started_at, 'completedAt', r.completed_at
             ) FROM incidents i JOIN repair_attempts r ON r.id = i.current_attempt_id WHERE i.id = $1",
        )
        .bind(incident_id)
        .fetch_optional(&self.pool)
        .await?;
        object.insert("events".into(), events);
        object.insert("currentAttempt".into(), attempt.unwrap_or(Value::Null));
        Ok(incident)
    }

    pub async fn app_settings(&self) -> Result<Option<Value>, DatabaseError> {
        let project = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
                'id', id, 'name', name, 'repoOwner', repo_owner, 'repoName', repo_name,
                'productionUrl', production_url, 'status', status, 'safetyMode', safety_mode,
                'protectionPaused', protection_paused
             ) FROM projects WHERE status = 'ACTIVE' ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(Value::Object(ref project_object)) = project else {
            return Ok(None);
        };
        let project_id = project_object
            .get("id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<Uuid>().ok())
            .ok_or(DatabaseError::InvalidStoredProject)?;
        let integrations = sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'type', type, 'externalAccountId', external_account_id,
                'externalProjectId', external_project_id, 'metadata', metadata_json,
                'status', status, 'createdAt', created_at
             ) ORDER BY type), '[]'::jsonb) FROM integrations WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let health_checks = sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'id', id, 'url', url, 'status', status, 'lastCheckedAt', last_checked_at
             ) ORDER BY url), '[]'::jsonb) FROM health_checks WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Some(serde_json::json!({
            "project": project,
            "integrations": integrations,
            "healthChecks": health_checks
        })))
    }

    pub async fn set_safety_mode(&self, mode: &str, actor: &str) -> Result<u64, DatabaseError> {
        if !matches!(mode, "safe" | "autopilot_experimental") {
            return Err(DatabaseError::InvalidSafetyMode);
        }
        let mut tx = self.pool.begin().await?;
        let project_ids = sqlx::query_scalar::<_, Uuid>(
            "UPDATE projects SET safety_mode = $1, updated_at = now() WHERE status = 'ACTIVE' AND safety_mode <> $1 RETURNING id",
        )
        .bind(mode)
        .fetch_all(&mut *tx)
        .await?;
        for project_id in &project_ids {
            sqlx::query("UPDATE policies SET safety_mode = $1 WHERE project_id = $2")
                .bind(mode)
                .bind(project_id)
                .execute(&mut *tx)
                .await?;
            insert_audit_event(
                &mut tx,
                *project_id,
                None,
                actor,
                "safety.mode.change",
                &project_id.to_string(),
                "success",
                &serde_json::json!({ "mode": mode }),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(project_ids.len() as u64)
    }

    pub async fn max_repair_attempts(&self, project_id: Uuid) -> Result<i32, DatabaseError> {
        Ok(
            sqlx::query_scalar("SELECT max_repair_attempts FROM policies WHERE project_id = $1")
                .bind(project_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn set_protection_paused(
        &self,
        paused: bool,
        actor: &str,
    ) -> Result<u64, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        let project_ids = sqlx::query_scalar::<_, Uuid>("UPDATE projects SET protection_paused = $1, updated_at = now() WHERE status = 'ACTIVE' AND protection_paused <> $1 RETURNING id")
            .bind(paused).fetch_all(&mut *tx).await?;
        for project_id in &project_ids {
            insert_audit_event(
                &mut tx,
                *project_id,
                None,
                actor,
                if paused {
                    "protection.pause"
                } else {
                    "protection.resume"
                },
                &project_id.to_string(),
                "success",
                &serde_json::json!({}),
            )
            .await?;

            if !paused {
                let paused_incidents = sqlx::query_scalar::<_, Uuid>(
                    "SELECT id FROM incidents WHERE project_id = $1 AND status = 'PAUSED' ORDER BY opened_at FOR UPDATE",
                )
                .bind(project_id)
                .fetch_all(&mut *tx)
                .await?;
                for incident_id in paused_incidents {
                    sqlx::query(
                        "UPDATE repair_attempts SET status = 'PAUSED' WHERE id = (SELECT current_attempt_id FROM incidents WHERE id = $1)",
                    )
                    .bind(incident_id)
                    .execute(&mut *tx)
                    .await?;
                    let updated = sqlx::query(
                        "UPDATE incidents SET status = 'COLLECTING_CONTEXT', current_attempt_id = NULL WHERE id = $1 AND status = 'PAUSED'",
                    )
                    .bind(incident_id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    if updated == 1 {
                        insert_incident_event(
                            &mut tx,
                            incident_id,
                            "STATE_CHANGED",
                            actor,
                            "Protection resumed; restarting incident from fresh context",
                            &serde_json::json!({ "from": "PAUSED", "to": "COLLECTING_CONTEXT" }),
                        )
                        .await?;
                        insert_audit_event(
                            &mut tx,
                            *project_id,
                            Some(incident_id),
                            actor,
                            "incident.resume",
                            &incident_id.to_string(),
                            "success",
                            &serde_json::json!({ "restartFrom": "COLLECTING_CONTEXT" }),
                        )
                        .await?;
                        sqlx::query("INSERT INTO jobs (id, job_type, idempotency_key, correlation_id, payload_json, max_attempts) VALUES ($1, 'diagnose', $2, $3, $4, 3)")
                            .bind(Uuid::now_v7())
                            .bind(format!("incident:{incident_id}:resume-diagnose:{}", Uuid::now_v7()))
                            .bind(incident_id)
                            .bind(serde_json::json!({ "incidentId": incident_id }))
                            .execute(&mut *tx)
                            .await?;
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(project_ids.len() as u64)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_audit_event(
        &self,
        project_id: Uuid,
        incident_id: Option<Uuid>,
        actor: &str,
        action: &str,
        target: &str,
        outcome: &str,
        metadata: &Value,
    ) -> Result<(), DatabaseError> {
        sqlx::query("INSERT INTO audit_events (id, project_id, incident_id, actor, action, target, outcome, metadata_json) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
            .bind(Uuid::now_v7())
            .bind(project_id)
            .bind(incident_id)
            .bind(actor)
            .bind(action)
            .bind(target)
            .bind(outcome)
            .bind(metadata)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn transition_incident(
        &self,
        transition: IncidentTransition,
    ) -> Result<(), DatabaseError> {
        let IncidentTransition {
            project_id,
            incident_id,
            expected,
            next,
            actor,
            message,
            metadata,
        } = transition;
        if !expected.can_transition_to(next) {
            return Err(DatabaseError::InvalidTransition { expected, next });
        }

        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE incidents SET status = $1,
                resolved_at = CASE WHEN $1 IN ('RESOLVED', 'ROLLED_BACK') THEN now() ELSE resolved_at END,
                autonomous_resolution = CASE WHEN $1 = 'RESOLVED' THEN NOT EXISTS (
                    SELECT 1 FROM incident_events WHERE incident_id = $2 AND actor = 'api-admin' AND message = 'Production deployment approved'
                ) ELSE autonomous_resolution END
             WHERE id = $2 AND project_id = $3 AND status = $4",
        )
        .bind(state_name(next))
        .bind(incident_id)
        .bind(project_id)
        .bind(state_name(expected))
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if updated != 1 {
            return Err(DatabaseError::ConcurrentIncidentUpdate);
        }

        insert_incident_event(
            &mut tx,
            incident_id,
            "STATE_CHANGED",
            &actor,
            &message,
            &metadata,
        )
        .await?;
        insert_audit_event(
            &mut tx,
            project_id,
            Some(incident_id),
            &actor,
            "incident.transition",
            &incident_id.to_string(),
            "success",
            &serde_json::json!({
                "from": state_name(expected),
                "to": state_name(next),
                "context": metadata,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn insert_incident_event(
    tx: &mut Transaction<'_, Postgres>,
    incident_id: Uuid,
    event_type: &str,
    actor: &str,
    message: &str,
    metadata: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO incident_events (id, incident_id, type, actor, message, metadata_json) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(Uuid::now_v7()).bind(incident_id).bind(event_type).bind(actor).bind(message).bind(metadata)
        .execute(&mut **tx).await?;
    Ok(())
}

async fn open_incident_tx(
    tx: &mut Transaction<'_, Postgres>,
    trigger: &IncidentTrigger,
) -> Result<Option<Uuid>, sqlx::Error> {
    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM incidents WHERE project_id = $1 AND deduplication_key = $2 AND status NOT IN ('RESOLVED', 'ROLLED_BACK', 'FAILED', 'ESCALATED', 'CANCELLED', 'IGNORED', 'DUPLICATE') FOR UPDATE",
    )
    .bind(trigger.project_id)
    .bind(&trigger.deduplication_key)
    .fetch_optional(&mut **tx)
    .await?;
    if existing.is_some() {
        return Ok(None);
    }
    let paused = sqlx::query_scalar::<_, bool>(
        "SELECT protection_paused FROM projects WHERE id = $1 FOR SHARE",
    )
    .bind(trigger.project_id)
    .fetch_one(&mut **tx)
    .await?;
    let id = Uuid::now_v7();
    let state = if paused { "PAUSED" } else { "OPEN" };
    sqlx::query("INSERT INTO incidents (id, project_id, deduplication_key, trigger_type, status, severity, title) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(id).bind(trigger.project_id).bind(&trigger.deduplication_key).bind(&trigger.trigger_type).bind(state).bind(&trigger.severity).bind(&trigger.title)
        .execute(&mut **tx).await?;
    insert_incident_event(
        tx,
        id,
        "INCIDENT_OPENED",
        "worker",
        &trigger.title,
        &trigger.metadata,
    )
    .await?;
    insert_audit_event(
        tx,
        trigger.project_id,
        Some(id),
        "worker",
        "incident.open",
        &id.to_string(),
        "success",
        &trigger.metadata,
    )
    .await?;
    if !paused {
        sqlx::query("INSERT INTO jobs (id, job_type, idempotency_key, correlation_id, payload_json, max_attempts) VALUES ($1, 'incident-context', $2, $3, $4, 5) ON CONFLICT (idempotency_key) DO NOTHING")
            .bind(Uuid::now_v7()).bind(format!("incident:{id}:context")).bind(id).bind(serde_json::json!({ "incidentId": id })).execute(&mut **tx).await?;
    }
    Ok(Some(id))
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit_event(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    incident_id: Option<Uuid>,
    actor: &str,
    action: &str,
    target: &str,
    outcome: &str,
    metadata: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events (id, project_id, incident_id, actor, action, target, outcome, metadata_json) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(Uuid::now_v7()).bind(project_id).bind(incident_id).bind(actor).bind(action).bind(target).bind(outcome).bind(metadata)
        .execute(&mut **tx).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobType {
    HealthCheck,
    WebhookProcess,
    IncidentContext,
    Diagnose,
    Repair,
    BuildTest,
    PreviewDeploy,
    Verify,
    ProductionAction,
    PostDeployWatch,
    Cleanup,
}

impl JobType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::HealthCheck => "health-check",
            Self::WebhookProcess => "webhook-process",
            Self::IncidentContext => "incident-context",
            Self::Diagnose => "diagnose",
            Self::Repair => "repair",
            Self::BuildTest => "build-test",
            Self::PreviewDeploy => "preview-deploy",
            Self::Verify => "verify",
            Self::ProductionAction => "production-action",
            Self::PostDeployWatch => "post-deploy-watch",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub job_type: String,
    pub idempotency_key: String,
    pub correlation_id: Option<Uuid>,
    pub payload_json: Value,
    pub attempt: i32,
    pub max_attempts: i32,
    pub available_at: OffsetDateTime,
}

impl Database {
    pub async fn accept_webhook(
        &self,
        provider: &str,
        external_delivery_id: &str,
        event_type: &str,
        metadata: &Value,
    ) -> Result<bool, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query("INSERT INTO webhook_deliveries (id, provider, external_delivery_id, event_type, metadata_json) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (provider, external_delivery_id) DO NOTHING")
            .bind(Uuid::now_v7())
            .bind(provider)
            .bind(external_delivery_id)
            .bind(event_type)
            .bind(metadata)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if inserted == 0 {
            tx.commit().await?;
            return Ok(false);
        }

        let idempotency_key = format!("webhook:{provider}:{external_delivery_id}");
        sqlx::query("INSERT INTO jobs (id, job_type, idempotency_key, payload_json, max_attempts) VALUES ($1, 'webhook-process', $2, $3, 5)")
            .bind(Uuid::now_v7())
            .bind(idempotency_key)
            .bind(serde_json::json!({
                "provider": provider,
                "deliveryId": external_delivery_id,
                "eventType": event_type,
            }))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn enqueue(
        &self,
        job_type: JobType,
        idempotency_key: &str,
        correlation_id: Option<Uuid>,
        payload: &Value,
        max_attempts: i32,
    ) -> Result<Uuid, DatabaseError> {
        let id = Uuid::now_v7();
        let stored = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO jobs (id, job_type, idempotency_key, correlation_id, payload_json, max_attempts) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (idempotency_key) DO UPDATE SET idempotency_key = EXCLUDED.idempotency_key RETURNING id",
        )
        .bind(id).bind(job_type.as_str()).bind(idempotency_key).bind(correlation_id).bind(payload).bind(max_attempts)
        .fetch_one(&self.pool).await?;
        Ok(stored)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn enqueue_after(
        &self,
        job_type: JobType,
        idempotency_key: &str,
        correlation_id: Option<Uuid>,
        payload: &Value,
        max_attempts: i32,
        delay_seconds: i64,
    ) -> Result<Uuid, DatabaseError> {
        let id = Uuid::now_v7();
        Ok(sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO jobs (id, job_type, idempotency_key, correlation_id, payload_json, max_attempts, available_at) VALUES ($1, $2, $3, $4, $5, $6, now() + ($7 * interval '1 second')) ON CONFLICT (idempotency_key) DO UPDATE SET idempotency_key = EXCLUDED.idempotency_key RETURNING id",
        )
        .bind(id).bind(job_type.as_str()).bind(idempotency_key).bind(correlation_id).bind(payload).bind(max_attempts).bind(delay_seconds.max(0))
        .fetch_one(&self.pool).await?)
    }

    pub async fn claim_next(&self, worker_id: &str) -> Result<Option<Job>, DatabaseError> {
        let mut tx = self.pool.begin().await?;
        let job = sqlx::query_as::<_, Job>(
            "SELECT id, job_type, idempotency_key, correlation_id, payload_json, attempt, max_attempts, available_at FROM jobs WHERE status IN ('PENDING', 'RETRY') AND available_at <= now() ORDER BY available_at, created_at FOR UPDATE SKIP LOCKED LIMIT 1",
        )
        .fetch_optional(&mut *tx).await?;
        let Some(job) = job else {
            tx.commit().await?;
            return Ok(None);
        };
        sqlx::query("UPDATE jobs SET status = 'RUNNING', locked_at = now(), locked_by = $1, attempt = attempt + 1 WHERE id = $2")
            .bind(worker_id).bind(job.id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(Job {
            attempt: job.attempt + 1,
            ..job
        }))
    }

    pub async fn complete_job(&self, id: Uuid, worker_id: &str) -> Result<(), DatabaseError> {
        let rows = sqlx::query("UPDATE jobs SET status = 'COMPLETED', completed_at = now(), locked_at = NULL, locked_by = NULL WHERE id = $1 AND status = 'RUNNING' AND locked_by = $2")
            .bind(id).bind(worker_id).execute(&self.pool).await?.rows_affected();
        if rows != 1 {
            return Err(DatabaseError::JobLeaseLost);
        }
        Ok(())
    }

    pub async fn fail_job(
        &self,
        job: &Job,
        worker_id: &str,
        error: &str,
    ) -> Result<(), DatabaseError> {
        let terminal = job.attempt >= job.max_attempts;
        let status = if terminal { "FAILED" } else { "RETRY" };
        let backoff_seconds = i64::from(2_i32.saturating_pow(job.attempt.clamp(1, 10) as u32));
        let rows = sqlx::query("UPDATE jobs SET status = $1, available_at = CASE WHEN $1 = 'RETRY' THEN now() + ($2 * interval '1 second') ELSE available_at END, last_error = $3, locked_at = NULL, locked_by = NULL WHERE id = $4 AND status = 'RUNNING' AND locked_by = $5")
            .bind(status).bind(backoff_seconds).bind(truncate_error(error)).bind(job.id).bind(worker_id)
            .execute(&self.pool).await?.rows_affected();
        if rows != 1 {
            return Err(DatabaseError::JobLeaseLost);
        }
        Ok(())
    }
}

fn truncate_error(error: &str) -> String {
    error.chars().take(4_000).collect()
}

const fn state_name(state: IncidentState) -> &'static str {
    match state {
        IncidentState::Open => "OPEN",
        IncidentState::CollectingContext => "COLLECTING_CONTEXT",
        IncidentState::Diagnosing => "DIAGNOSING",
        IncidentState::Planning => "PLANNING",
        IncidentState::Repairing => "REPAIRING",
        IncidentState::Testing => "TESTING",
        IncidentState::PreviewDeploying => "PREVIEW_DEPLOYING",
        IncidentState::VerifyingPreview => "VERIFYING_PREVIEW",
        IncidentState::WaitingApproval => "WAITING_APPROVAL",
        IncidentState::ProductionDeploying => "PRODUCTION_DEPLOYING",
        IncidentState::VerifyingProduction => "VERIFYING_PRODUCTION",
        IncidentState::RollingBack => "ROLLING_BACK",
        IncidentState::RolledBack => "ROLLED_BACK",
        IncidentState::Resolved => "RESOLVED",
        IncidentState::Failed => "FAILED",
        IncidentState::Escalated => "ESCALATED",
        IncidentState::Cancelled => "CANCELLED",
        IncidentState::Ignored => "IGNORED",
        IncidentState::Duplicate => "DUPLICATE",
        IncidentState::Paused => "PAUSED",
    }
}

fn parse_state(value: &str) -> Result<IncidentState, DatabaseError> {
    Ok(match value {
        "OPEN" => IncidentState::Open,
        "COLLECTING_CONTEXT" => IncidentState::CollectingContext,
        "DIAGNOSING" => IncidentState::Diagnosing,
        "PLANNING" => IncidentState::Planning,
        "REPAIRING" => IncidentState::Repairing,
        "TESTING" => IncidentState::Testing,
        "PREVIEW_DEPLOYING" => IncidentState::PreviewDeploying,
        "VERIFYING_PREVIEW" => IncidentState::VerifyingPreview,
        "WAITING_APPROVAL" => IncidentState::WaitingApproval,
        "PRODUCTION_DEPLOYING" => IncidentState::ProductionDeploying,
        "VERIFYING_PRODUCTION" => IncidentState::VerifyingProduction,
        "ROLLING_BACK" => IncidentState::RollingBack,
        "ROLLED_BACK" => IncidentState::RolledBack,
        "RESOLVED" => IncidentState::Resolved,
        "FAILED" => IncidentState::Failed,
        "ESCALATED" => IncidentState::Escalated,
        "CANCELLED" => IncidentState::Cancelled,
        "IGNORED" => IncidentState::Ignored,
        "DUPLICATE" => IncidentState::Duplicate,
        "PAUSED" => IncidentState::Paused,
        _ => return Err(DatabaseError::UnknownIncidentState(value.to_owned())),
    })
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("invalid incident transition from {expected:?} to {next:?}")]
    InvalidTransition {
        expected: IncidentState,
        next: IncidentState,
    },
    #[error("incident changed concurrently")]
    ConcurrentIncidentUpdate,
    #[error("job lease is no longer owned by this worker")]
    JobLeaseLost,
    #[error("database contains an unknown incident state: {0}")]
    UnknownIncidentState(String),
    #[error("repair attempt is missing diagnosis, proposal, or patch data")]
    IncompleteRepairAttempt,
    #[error("numeric value exceeds the database representation")]
    NumericOverflow,
    #[error("deployment record was not found")]
    DeploymentNotFound,
    #[error("production protection is paused")]
    ProtectionPaused,
    #[error("incident is not waiting for production approval")]
    IncidentNotAwaitingApproval,
    #[error("a local administrator already exists")]
    AdminAlreadyExists,
    #[error("the OSS installation already protects an app")]
    ProjectAlreadyExists,
    #[error("database contains an invalid project identifier")]
    InvalidStoredProject,
    #[error("invalid safety mode")]
    InvalidSafetyMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_output_is_bounded() {
        assert_eq!(truncate_error(&"x".repeat(5_000)).len(), 4_000);
    }

    #[test]
    fn job_names_are_stable() {
        assert_eq!(JobType::ProductionAction.as_str(), "production-action");
    }
}
