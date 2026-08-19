use std::time::Duration;

use nopager_core::IncidentState;
use nopager_db::{Database, IncidentTransition, Job, JobType};
use serde_json::{Value, json};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod production;

// Keep the already-validated incident pipeline byte-for-byte intact while the
// production durability boundary is separated into a small orchestration layer.
#[allow(dead_code)]
mod legacy {
    pub(super) async fn execute_job_public(
        database: &nopager_db::Database,
        job: &nopager_db::Job,
    ) -> anyhow::Result<()> {
        execute_job(database, job).await
    }

    pub(super) async fn github_client_public(
        database: &nopager_db::Database,
        work: &nopager_db::IncidentWork,
    ) -> anyhow::Result<nopager_connectors::github::GitHubClient> {
        github_client(database, work).await
    }

    pub(super) async fn vercel_client_public(
        database: &nopager_db::Database,
        work: &nopager_db::IncidentWork,
    ) -> anyhow::Result<nopager_connectors::vercel::VercelClient> {
        vercel_client(database, work).await
    }

    pub(super) async fn begin_rollback_public(
        database: &nopager_db::Database,
        work: &nopager_db::IncidentWork,
        incident_id: uuid::Uuid,
        attempt_id: uuid::Uuid,
        reason: &str,
    ) -> anyhow::Result<()> {
        begin_rollback(database, work, incident_id, attempt_id, reason).await
    }

    pub(super) async fn enqueue_cleanup_public(
        database: &nopager_db::Database,
        incident_id: uuid::Uuid,
        attempt_id: uuid::Uuid,
    ) -> anyhow::Result<()> {
        enqueue_cleanup(database, incident_id, attempt_id).await
    }

    pub(super) async fn require_healthy_public(value: &str) -> anyhow::Result<()> {
        require_healthy(value).await
    }

    include!("legacy_pipeline.rs");
}

const MAX_PROMOTION_ACTIVATION_POLLS: u64 = 60;
const PROMOTION_ACTIVATION_POLL_SECONDS: i64 = 5;
const MAX_DURABLE_DEPLOYMENT_POLLS: u64 = 60;
const DURABLE_DEPLOYMENT_POLL_SECONDS: i64 = 10;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();
    info!("NoPager worker ready");
    let database_url = std::env::var("DATABASE_URL")?;
    let database = Database::connect(&database_url).await?;
    database.migrate().await?;
    let worker_id = format!("worker-{}", std::process::id());

    let mut scheduler = tokio::time::interval(Duration::from_secs(5));
    scheduler.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = scheduler.tick() => {
                if let Err(error) = database.enqueue_due_health_checks(100).await {
                    error!(%error, "failed to schedule health checks");
                }
                if let Err(error) = database.enqueue_due_vercel_polls(100).await {
                    error!(%error, "failed to schedule Vercel polling fallback");
                }
            }
            () = tokio::time::sleep(Duration::from_millis(750)) => {
                if let Some(job) = database.claim_next(&worker_id).await? {
                    match execute_job(&database, &job).await {
                        Ok(()) => database.complete_job(job.id, &worker_id).await?,
                        Err(error) => {
                            error!(job_id = %job.id, %error, "job failed");
                            database.fail_job(&job, &worker_id, &error.to_string()).await?;
                            if job.attempt >= job.max_attempts
                                && let Some(incident_id) = job.correlation_id
                            {
                                database.escalate_incident(incident_id, &error.to_string()).await?;
                            }
                        }
                    }
                }
            }
            result = tokio::signal::ctrl_c() => {
                result?;
                info!("worker shutting down");
                return Ok(());
            }
        }
    }
}

async fn execute_job(database: &Database, job: &Job) -> anyhow::Result<()> {
    match job.job_type.as_str() {
        "production-action"
            if job.payload_json.get("action").and_then(Value::as_str) == Some("land-repair") =>
        {
            persist_verified_repair(database, &job.payload_json).await
        }
        "verify" if job.payload_json.get("phase").and_then(Value::as_str) == Some("production") => {
            verify_promoted_production(database, &job.payload_json).await
        }
        "verify"
            if job.payload_json.get("phase").and_then(Value::as_str)
                == Some("durable-production") =>
        {
            verify_durable_production(database, &job.payload_json).await
        }
        "post-deploy-watch"
            if job.payload_json.get("phase").and_then(Value::as_str)
                == Some("durable-production") =>
        {
            watch_durable_production(database, &job.payload_json).await
        }
        "post-deploy-watch" => watch_promoted_production(database, &job.payload_json).await,
        _ => legacy::execute_job_public(database, job).await,
    }
}

async fn verify_promoted_production(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = required_string(payload, "incidentId")?.parse()?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let poll = payload.get("poll").and_then(Value::as_u64).unwrap_or(0);
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    let attempt = database.repair_attempt(attempt_id).await?;
    let deployment_id = attempt
        .preview_deployment_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("promoted deployment id is missing"))?;
    let commit_sha = attempt
        .repair_commit_sha
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("repair commit SHA is missing"))?;
    let vercel = legacy::vercel_client_public(database, &work).await?;

    if poll > MAX_PROMOTION_ACTIVATION_POLLS {
        legacy::begin_rollback_public(
            database,
            &work,
            incident_id,
            attempt_id,
            "timed out waiting for the promoted repair to become current production",
        )
        .await?;
        return Ok(());
    }

    match production::current_production_readiness(&vercel, deployment_id, commit_sha).await {
        Ok(production::ProductionReadiness::Pending) => {
            let next = poll + 1;
            database
                .enqueue_after(
                    JobType::Verify,
                    &format!("incident:{incident_id}:verify-production:{next}"),
                    Some(incident_id),
                    &json!({
                        "incidentId": incident_id,
                        "attemptId": attempt_id,
                        "phase": "production",
                        "poll": next
                    }),
                    3,
                    PROMOTION_ACTIVATION_POLL_SECONDS,
                )
                .await?;
            return Ok(());
        }
        Ok(production::ProductionReadiness::Ready) => {}
        Err(error) => {
            legacy::begin_rollback_public(
                database,
                &work,
                incident_id,
                attempt_id,
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    }

    if let Err(error) = legacy::require_healthy_public(&work.health_check_url).await {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }

    database
        .enqueue_after(
            JobType::PostDeployWatch,
            &format!("incident:{incident_id}:watch:0"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "attemptId": attempt_id,
                "check": 0,
                "phase": "promoted-production"
            }),
            3,
            10,
        )
        .await?;
    Ok(())
}

async fn watch_promoted_production(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = required_string(payload, "incidentId")?.parse()?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let check = required_check(payload)?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    let attempt = database.repair_attempt(attempt_id).await?;
    let deployment_id = attempt
        .preview_deployment_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("promoted deployment id is missing"))?;
    let commit_sha = attempt
        .repair_commit_sha
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("repair commit SHA is missing"))?;
    let vercel = legacy::vercel_client_public(database, &work).await?;

    if let Err(error) =
        production::verify_current_production(&vercel, deployment_id, commit_sha).await
    {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }
    if let Err(error) = legacy::require_healthy_public(&work.health_check_url).await {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }

    if check < 2 {
        let next = check + 1;
        database
            .enqueue_after(
                JobType::PostDeployWatch,
                &format!("incident:{incident_id}:watch:{next}"),
                Some(incident_id),
                &json!({
                    "incidentId": incident_id,
                    "attemptId": attempt_id,
                    "check": next,
                    "phase": "promoted-production"
                }),
                3,
                10,
            )
            .await?;
        return Ok(());
    }

    database
        .mark_deployment_known_good(work.project_id, deployment_id)
        .await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "worker",
            "vercel.production.verified",
            deployment_id,
            "success",
            &json!({ "commitSha": commit_sha, "checks": 3 }),
        )
        .await?;
    database
        .enqueue(
            JobType::ProductionAction,
            &format!("incident:{incident_id}:land-repair:0"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "attemptId": attempt_id,
                "action": "land-repair",
                "poll": 0
            }),
            3,
        )
        .await?;
    Ok(())
}

async fn persist_verified_repair(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = required_string(payload, "incidentId")?.parse()?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let poll = payload.get("poll").and_then(Value::as_u64).unwrap_or(0);
    let work = database.incident_work(incident_id).await?;

    if work.protection_paused {
        if work.state != IncidentState::Paused {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: work.state,
                    next: IncidentState::Paused,
                    actor: "worker".into(),
                    message: "Protection paused before persisting the verified production repair"
                        .into(),
                    metadata: json!({}),
                })
                .await?;
        }
        return Ok(());
    }
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    if poll > MAX_DURABLE_DEPLOYMENT_POLLS {
        anyhow::bail!("timed out waiting for the merged repair to become current production");
    }

    let attempt = database.repair_attempt(attempt_id).await?;
    let vercel_project_id = work
        .vercel_project_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
    let github = legacy::github_client_public(database, &work).await?;
    let vercel = legacy::vercel_client_public(database, &work).await?;

    match production::land_and_find_production(&github, &vercel, &work, &attempt, vercel_project_id)
        .await?
    {
        production::ProductionLanding::Pending { merge_sha } => {
            if poll == 0 {
                database
                    .record_audit_event(
                        work.project_id,
                        Some(incident_id),
                        "worker",
                        "github.repair_pr.land",
                        attempt.repair_branch.as_deref().unwrap_or("repair"),
                        "success",
                        &json!({ "mergeSha": merge_sha }),
                    )
                    .await?;
            }
            let next = poll + 1;
            database
                .enqueue_after(
                    JobType::ProductionAction,
                    &format!("incident:{incident_id}:land-repair:{next}"),
                    Some(incident_id),
                    &json!({
                        "incidentId": incident_id,
                        "attemptId": attempt_id,
                        "action": "land-repair",
                        "poll": next
                    }),
                    3,
                    DURABLE_DEPLOYMENT_POLL_SECONDS,
                )
                .await?;
        }
        production::ProductionLanding::Ready(deployment) => {
            if poll == 0 {
                database
                    .record_audit_event(
                        work.project_id,
                        Some(incident_id),
                        "worker",
                        "github.repair_pr.land",
                        attempt.repair_branch.as_deref().unwrap_or("repair"),
                        "success",
                        &json!({ "mergeSha": deployment.commit_sha }),
                    )
                    .await?;
            }
            database
                .save_deployment(
                    work.project_id,
                    &deployment.id,
                    "production",
                    &deployment.commit_sha,
                    &deployment.url,
                    "READY",
                    false,
                )
                .await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "vercel.production.current",
                    &deployment.id,
                    "success",
                    &json!({ "commitSha": deployment.commit_sha, "url": deployment.url }),
                )
                .await?;
            database
                .enqueue_after(
                    JobType::Verify,
                    &format!("incident:{incident_id}:verify-durable-production"),
                    Some(incident_id),
                    &json!({
                        "incidentId": incident_id,
                        "attemptId": attempt_id,
                        "phase": "durable-production",
                        "productionDeploymentId": deployment.id,
                        "productionCommitSha": deployment.commit_sha
                    }),
                    5,
                    5,
                )
                .await?;
        }
    }
    Ok(())
}

async fn verify_durable_production(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = required_string(payload, "incidentId")?.parse()?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let deployment_id = required_string(payload, "productionDeploymentId")?;
    let commit_sha = required_string(payload, "productionCommitSha")?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    let vercel = legacy::vercel_client_public(database, &work).await?;

    if let Err(error) =
        production::verify_current_production(&vercel, deployment_id, commit_sha).await
    {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }
    if let Err(error) = legacy::require_healthy_public(&work.health_check_url).await {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }

    database
        .enqueue_after(
            JobType::PostDeployWatch,
            &format!("incident:{incident_id}:durable-watch:0"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "attemptId": attempt_id,
                "check": 0,
                "phase": "durable-production",
                "productionDeploymentId": deployment_id,
                "productionCommitSha": commit_sha
            }),
            3,
            10,
        )
        .await?;
    Ok(())
}

async fn watch_durable_production(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = required_string(payload, "incidentId")?.parse()?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let check = required_check(payload)?;
    let deployment_id = required_string(payload, "productionDeploymentId")?;
    let commit_sha = required_string(payload, "productionCommitSha")?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    let vercel = legacy::vercel_client_public(database, &work).await?;

    if let Err(error) =
        production::verify_current_production(&vercel, deployment_id, commit_sha).await
    {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }
    if let Err(error) = legacy::require_healthy_public(&work.health_check_url).await {
        legacy::begin_rollback_public(database, &work, incident_id, attempt_id, &error.to_string())
            .await?;
        return Ok(());
    }

    if check < 2 {
        let next = check + 1;
        database
            .enqueue_after(
                JobType::PostDeployWatch,
                &format!("incident:{incident_id}:durable-watch:{next}"),
                Some(incident_id),
                &json!({
                    "incidentId": incident_id,
                    "attemptId": attempt_id,
                    "check": next,
                    "phase": "durable-production",
                    "productionDeploymentId": deployment_id,
                    "productionCommitSha": commit_sha
                }),
                3,
                10,
            )
            .await?;
        return Ok(());
    }

    database
        .mark_deployment_known_good(work.project_id, deployment_id)
        .await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::VerifyingProduction,
            next: IncidentState::Resolved,
            actor: "worker".into(),
            message:
                "Production repair stayed healthy and was durably merged into the source branch"
                    .into(),
            metadata: json!({
                "checks": 3,
                "deploymentId": deployment_id,
                "commitSha": commit_sha
            }),
        })
        .await?;
    legacy::enqueue_cleanup_public(database, incident_id, attempt_id).await?;
    Ok(())
}

fn required_check(value: &Value) -> anyhow::Result<u64> {
    value
        .get("check")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("watch check is missing"))
}

fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}
