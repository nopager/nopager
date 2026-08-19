use nopager_connectors::{
    github::GitHubAppAuth,
    github_pull::{PullRequestStatus, get_pull_request_status},
    github_revert::{RevertPullRequest, RevertPullRequestOutcome, open_revert_pull_request_once},
};
use nopager_core::IncidentState;
use nopager_db::{Database, IncidentTransition, IncidentWork, JobType, RepairAttemptWork};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::production::{self, ProductionDiscovery};

const MAX_SOURCE_REVERT_REVIEW_POLLS: u64 = 2_016;
const SOURCE_REVERT_REVIEW_POLL_SECONDS: i64 = 300;
const MAX_SOURCE_RECOVERY_DEPLOYMENT_POLLS: u64 = 60;
const SOURCE_RECOVERY_DEPLOYMENT_POLL_SECONDS: i64 = 10;
const SOURCE_RECOVERY_WATCH_SECONDS: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RevertIdentity {
    number: u64,
    node_id: String,
    head_sha: String,
    base_sha: String,
}

pub(crate) async fn prepare_draft_source_revert(
    database: &Database,
    work: &IncidentWork,
    incident_id: Uuid,
    attempt_id: Uuid,
    attempt: &RepairAttemptWork,
    known_good_deployment_id: &str,
    auth: anyhow::Result<GitHubAppAuth>,
) -> anyhow::Result<()> {
    let result = async {
        let auth = auth.map_err(|error| error.to_string())?;
        let repair_branch = attempt
            .repair_branch
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "repair branch is missing for source recovery".to_owned())?;
        let repair_head_sha = attempt
            .repair_commit_sha
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "repair commit SHA is missing for source recovery".to_owned())?;
        open_revert_pull_request_once(
            &auth,
            &work.repo_owner,
            &work.repo_name,
            repair_branch,
            repair_head_sha,
            &attempt.base_commit_sha,
        )
        .await
        .map_err(|error| error.to_string())
    }
    .await;

    match result {
        Ok(RevertPullRequestOutcome::Created(pull_request)) => {
            let metadata = revert_metadata(&pull_request);
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "github.repair_pr.revert_created",
                    &pull_request.number.to_string(),
                    "success",
                    &metadata,
                )
                .await?;
            enqueue_source_revert_review(database, incident_id, &pull_request, 0, 30).await?;
            transition_to_source_revert_review(
                database,
                work,
                incident_id,
                known_good_deployment_id,
                &metadata,
                "Production recovered by rollback; a draft source-revert PR was created and requires review",
            )
            .await?;
        }
        Ok(RevertPullRequestOutcome::ExistingCandidate(pull_request)) => {
            let metadata = revert_metadata(&pull_request);
            if trusted_created_revert_exists(database, incident_id, &identity(&pull_request))
                .await?
            {
                enqueue_source_revert_review(database, incident_id, &pull_request, 0, 30).await?;
                transition_to_source_revert_review(
                    database,
                    work,
                    incident_id,
                    known_good_deployment_id,
                    &metadata,
                    "Production recovered by rollback; the previously created draft source-revert PR was recovered and requires review",
                )
                .await?;
                return Ok(());
            }

            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "github.repair_pr.revert_candidate_found",
                    &pull_request.number.to_string(),
                    "needs_review",
                    &metadata,
                )
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::RolledBack,
                    next: IncidentState::Escalated,
                    actor: "worker".into(),
                    message: "Production recovered by rollback; an existing source-revert candidate requires verification".into(),
                    metadata: json!({
                        "deploymentId": known_good_deployment_id,
                        "actionRequired": "verify_existing_source_revert",
                        "sourceRepairMerged": true,
                        "sourceRevertCandidate": metadata
                    }),
                })
                .await?;
        }
        Err(error) => {
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "github.repair_pr.revert_create",
                    &attempt_id.to_string(),
                    "failure",
                    &json!({
                        "error": truncate_error(&error),
                        "mutationRetrySuppressed": true
                    }),
                )
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::RolledBack,
                    next: IncidentState::Escalated,
                    actor: "worker".into(),
                    message: "Production recovered by rollback, but source-revert preparation needs human verification".into(),
                    metadata: json!({
                        "deploymentId": known_good_deployment_id,
                        "actionRequired": "create_or_verify_source_revert",
                        "sourceRepairMerged": true,
                        "sourceRevertError": truncate_error(&error),
                        "mutationRetrySuppressed": true
                    }),
                })
                .await?;
        }
    }

    Ok(())
}

pub(crate) async fn verify_source_revert_review(
    database: &Database,
    payload: &Value,
) -> anyhow::Result<()> {
    let incident_id = required_uuid(payload, "incidentId")?;
    let poll = payload.get("poll").and_then(Value::as_u64).unwrap_or(0);
    let expected = identity_from_payload(payload)?;
    let work = database.incident_work(incident_id).await?;

    if work.state == IncidentState::RolledBack {
        if poll < 3 {
            enqueue_review_identity(database, incident_id, &expected, poll + 1, 10).await?;
        }
        return Ok(());
    }
    if work.state != IncidentState::Escalated {
        return Ok(());
    }
    if !trusted_created_revert_exists(database, incident_id, &expected).await? {
        audit_source_recovery_failure(
            database,
            &work,
            incident_id,
            "github.repair_pr.revert_review",
            &expected.number.to_string(),
            "provider-attested revert creation record is missing or changed",
        )
        .await?;
        return Ok(());
    }

    let auth = crate::legacy::github_auth_public(database, &work).await?;
    let status =
        match get_pull_request_status(&auth, &work.repo_owner, &work.repo_name, expected.number)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                audit_source_recovery_failure(
                    database,
                    &work,
                    incident_id,
                    "github.repair_pr.revert_review",
                    &expected.number.to_string(),
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        };

    if !pull_request_identity_matches(&status, &expected) {
        audit_source_recovery_failure(
            database,
            &work,
            incident_id,
            "github.repair_pr.revert_review",
            &expected.number.to_string(),
            "source-revert pull request identity changed after NoPager created it",
        )
        .await?;
        return Ok(());
    }

    let Some(merge_sha) = status
        .merged_at
        .as_ref()
        .and(status.merge_commit_sha.as_deref())
        .filter(|value| !value.trim().is_empty())
    else {
        if poll >= MAX_SOURCE_REVERT_REVIEW_POLLS {
            audit_source_recovery_failure(
                database,
                &work,
                incident_id,
                "github.repair_pr.revert_review",
                &expected.number.to_string(),
                "source-revert PR was not merged within the seven-day automatic review window",
            )
            .await?;
            return Ok(());
        }
        enqueue_review_identity(
            database,
            incident_id,
            &expected,
            poll + 1,
            SOURCE_REVERT_REVIEW_POLL_SECONDS,
        )
        .await?;
        return Ok(());
    };

    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "worker",
            "github.repair_pr.revert_merged",
            &expected.number.to_string(),
            "success",
            &json!({
                "mergeCommitSha": merge_sha,
                "headSha": expected.head_sha,
                "baseSha": expected.base_sha,
                "nodeId": expected.node_id
            }),
        )
        .await?;
    database
        .enqueue(
            JobType::Verify,
            &format!("incident:{incident_id}:source-recovery-production:0:{merge_sha}"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "phase": "source-recovery-production",
                "poll": 0,
                "pullRequestNumber": expected.number,
                "sourceRevertNodeId": expected.node_id,
                "sourceRevertHeadSha": expected.head_sha,
                "sourceRevertBaseSha": expected.base_sha,
                "sourceRevertMergeSha": merge_sha
            }),
            3,
        )
        .await?;
    Ok(())
}

pub(crate) async fn verify_source_recovery_production(
    database: &Database,
    payload: &Value,
) -> anyhow::Result<()> {
    let incident_id = required_uuid(payload, "incidentId")?;
    let poll = payload.get("poll").and_then(Value::as_u64).unwrap_or(0);
    let expected = identity_from_payload(payload)?;
    let merge_sha = required_string(payload, "sourceRevertMergeSha")?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::Escalated {
        return Ok(());
    }
    if !trusted_created_revert_exists(database, incident_id, &expected).await? {
        return Ok(());
    }
    if poll > MAX_SOURCE_RECOVERY_DEPLOYMENT_POLLS {
        audit_source_recovery_failure(
            database,
            &work,
            incident_id,
            "vercel.source_recovery.production",
            merge_sha,
            "timed out waiting for the merged source revert to become current Production",
        )
        .await?;
        return Ok(());
    }

    let vercel_project_id = work
        .vercel_project_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
    let vercel = crate::legacy::vercel_client_public(database, &work).await?;
    let deployment =
        match production::find_current_production_for_commit(&vercel, vercel_project_id, merge_sha)
            .await
        {
            Ok(ProductionDiscovery::Pending) => {
                enqueue_source_recovery_production(
                    database,
                    incident_id,
                    &expected,
                    merge_sha,
                    poll + 1,
                )
                .await?;
                return Ok(());
            }
            Ok(ProductionDiscovery::Ready(deployment)) => deployment,
            Err(error) => {
                audit_source_recovery_failure(
                    database,
                    &work,
                    incident_id,
                    "vercel.source_recovery.production",
                    merge_sha,
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        };

    if !authoritative_current_target_matches(&vercel, vercel_project_id, &deployment.id).await? {
        enqueue_source_recovery_production(database, incident_id, &expected, merge_sha, poll + 1)
            .await?;
        return Ok(());
    }

    database
        .save_deployment(
            work.project_id,
            &deployment.id,
            "production",
            merge_sha,
            &deployment.url,
            "READY",
            false,
        )
        .await?;
    if let Err(error) = crate::legacy::require_healthy_public(&work.health_check_url).await {
        audit_source_recovery_failure(
            database,
            &work,
            incident_id,
            "vercel.source_recovery.production_health",
            &deployment.id,
            &error.to_string(),
        )
        .await?;
        return Ok(());
    }

    database
        .enqueue_after(
            JobType::PostDeployWatch,
            &format!(
                "incident:{incident_id}:source-recovery-watch:0:{}",
                deployment.id
            ),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "phase": "source-recovery-production",
                "check": 0,
                "pullRequestNumber": expected.number,
                "sourceRevertNodeId": expected.node_id,
                "sourceRevertHeadSha": expected.head_sha,
                "sourceRevertBaseSha": expected.base_sha,
                "sourceRevertMergeSha": merge_sha,
                "productionDeploymentId": deployment.id,
                "productionCommitSha": merge_sha
            }),
            3,
            SOURCE_RECOVERY_WATCH_SECONDS,
        )
        .await?;
    Ok(())
}

pub(crate) async fn watch_source_recovery_production(
    database: &Database,
    payload: &Value,
) -> anyhow::Result<()> {
    let incident_id = required_uuid(payload, "incidentId")?;
    let check = payload
        .get("check")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("source recovery watch check is missing"))?;
    let expected = identity_from_payload(payload)?;
    let merge_sha = required_string(payload, "sourceRevertMergeSha")?;
    let deployment_id = required_string(payload, "productionDeploymentId")?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::Escalated {
        return Ok(());
    }
    if !trusted_created_revert_exists(database, incident_id, &expected).await? {
        return Ok(());
    }

    let vercel_project_id = work
        .vercel_project_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
    let vercel = crate::legacy::vercel_client_public(database, &work).await?;
    let verification = async {
        production::verify_current_production(&vercel, deployment_id, merge_sha).await?;
        if !authoritative_current_target_matches(&vercel, vercel_project_id, deployment_id).await? {
            anyhow::bail!(
                "source-recovery deployment is no longer the authoritative Production target"
            );
        }
        crate::legacy::require_healthy_public(&work.health_check_url).await
    }
    .await;
    if let Err(error) = verification {
        audit_source_recovery_failure(
            database,
            &work,
            incident_id,
            "vercel.source_recovery.production_watch",
            deployment_id,
            &error.to_string(),
        )
        .await?;
        return Ok(());
    }

    if check < 2 {
        let next = check + 1;
        database
            .enqueue_after(
                JobType::PostDeployWatch,
                &format!("incident:{incident_id}:source-recovery-watch:{next}:{deployment_id}"),
                Some(incident_id),
                &json!({
                    "incidentId": incident_id,
                    "phase": "source-recovery-production",
                    "check": next,
                    "pullRequestNumber": expected.number,
                    "sourceRevertNodeId": expected.node_id,
                    "sourceRevertHeadSha": expected.head_sha,
                    "sourceRevertBaseSha": expected.base_sha,
                    "sourceRevertMergeSha": merge_sha,
                    "productionDeploymentId": deployment_id,
                    "productionCommitSha": merge_sha
                }),
                3,
                SOURCE_RECOVERY_WATCH_SECONDS,
            )
            .await?;
        return Ok(());
    }

    database
        .mark_deployment_known_good(work.project_id, deployment_id)
        .await?;
    resolve_escalated_source_recovery(
        database,
        &work,
        incident_id,
        &expected,
        merge_sha,
        deployment_id,
    )
    .await?;
    Ok(())
}

async fn transition_to_source_revert_review(
    database: &Database,
    work: &IncidentWork,
    incident_id: Uuid,
    known_good_deployment_id: &str,
    metadata: &Value,
    message: &str,
) -> anyhow::Result<()> {
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::RolledBack,
            next: IncidentState::Escalated,
            actor: "worker".into(),
            message: message.into(),
            metadata: json!({
                "deploymentId": known_good_deployment_id,
                "actionRequired": "review_source_revert",
                "sourceRepairMerged": true,
                "sourceRevert": metadata
            }),
        })
        .await?;
    Ok(())
}

async fn enqueue_source_revert_review(
    database: &Database,
    incident_id: Uuid,
    pull_request: &RevertPullRequest,
    poll: u64,
    delay_seconds: i64,
) -> anyhow::Result<()> {
    enqueue_review_identity(
        database,
        incident_id,
        &identity(pull_request),
        poll,
        delay_seconds,
    )
    .await
}

async fn enqueue_review_identity(
    database: &Database,
    incident_id: Uuid,
    identity: &RevertIdentity,
    poll: u64,
    delay_seconds: i64,
) -> anyhow::Result<()> {
    database
        .enqueue_after(
            JobType::Verify,
            &format!(
                "incident:{incident_id}:source-revert-review:{poll}:{}",
                identity.number
            ),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "phase": "source-revert-review",
                "poll": poll,
                "pullRequestNumber": identity.number,
                "sourceRevertNodeId": identity.node_id,
                "sourceRevertHeadSha": identity.head_sha,
                "sourceRevertBaseSha": identity.base_sha
            }),
            3,
            delay_seconds,
        )
        .await?;
    Ok(())
}

async fn enqueue_source_recovery_production(
    database: &Database,
    incident_id: Uuid,
    identity: &RevertIdentity,
    merge_sha: &str,
    poll: u64,
) -> anyhow::Result<()> {
    database
        .enqueue_after(
            JobType::Verify,
            &format!("incident:{incident_id}:source-recovery-production:{poll}:{merge_sha}"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "phase": "source-recovery-production",
                "poll": poll,
                "pullRequestNumber": identity.number,
                "sourceRevertNodeId": identity.node_id,
                "sourceRevertHeadSha": identity.head_sha,
                "sourceRevertBaseSha": identity.base_sha,
                "sourceRevertMergeSha": merge_sha
            }),
            3,
            SOURCE_RECOVERY_DEPLOYMENT_POLL_SECONDS,
        )
        .await?;
    Ok(())
}

async fn authoritative_current_target_matches(
    vercel: &nopager_connectors::vercel::VercelClient,
    vercel_project_id: &str,
    deployment_id: &str,
) -> anyhow::Result<bool> {
    Ok(vercel
        .get_project(vercel_project_id)
        .await?
        .current_production_target()
        .is_some_and(|target| target.id == deployment_id))
}

async fn trusted_created_revert_exists(
    database: &Database,
    incident_id: Uuid,
    identity: &RevertIdentity,
) -> anyhow::Result<bool> {
    let number = identity.number.to_string();
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM audit_events
            WHERE incident_id = $1
              AND action = 'github.repair_pr.revert_created'
              AND outcome = 'success'
              AND metadata_json #>> '{pullRequestNumber}' = $2
              AND metadata_json #>> '{nodeId}' = $3
              AND metadata_json #>> '{headSha}' = $4
              AND metadata_json #>> '{baseSha}' = $5
        )",
    )
    .bind(incident_id)
    .bind(number)
    .bind(&identity.node_id)
    .bind(&identity.head_sha)
    .bind(&identity.base_sha)
    .fetch_one(database.pool())
    .await?)
}

async fn resolve_escalated_source_recovery(
    database: &Database,
    work: &IncidentWork,
    incident_id: Uuid,
    identity: &RevertIdentity,
    merge_sha: &str,
    deployment_id: &str,
) -> anyhow::Result<()> {
    let mut tx = database.pool().begin().await?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM incidents WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(incident_id)
    .bind(work.project_id)
    .fetch_one(&mut *tx)
    .await?;
    if status != "ESCALATED" {
        tx.commit().await?;
        return Ok(());
    }

    let number = identity.number.to_string();
    let trusted = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM audit_events
            WHERE incident_id = $1
              AND action = 'github.repair_pr.revert_created'
              AND outcome = 'success'
              AND metadata_json #>> '{pullRequestNumber}' = $2
              AND metadata_json #>> '{nodeId}' = $3
              AND metadata_json #>> '{headSha}' = $4
              AND metadata_json #>> '{baseSha}' = $5
        )",
    )
    .bind(incident_id)
    .bind(&number)
    .bind(&identity.node_id)
    .bind(&identity.head_sha)
    .bind(&identity.base_sha)
    .fetch_one(&mut *tx)
    .await?;
    if !trusted {
        anyhow::bail!("source recovery identity is no longer provider-attested");
    }

    let updated = sqlx::query(
        "UPDATE incidents
         SET status = 'RESOLVED', resolved_at = now(), autonomous_resolution = false
         WHERE id = $1 AND project_id = $2 AND status = 'ESCALATED'",
    )
    .bind(incident_id)
    .bind(work.project_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated != 1 {
        anyhow::bail!("source recovery incident changed concurrently");
    }

    let metadata = json!({
        "actionRequired": Value::Null,
        "sourceRecoveryVerified": true,
        "pullRequestNumber": identity.number,
        "sourceRevertMergeSha": merge_sha,
        "deploymentId": deployment_id,
        "checks": 3,
        "autonomousResolution": false
    });
    sqlx::query(
        "INSERT INTO incident_events (id, incident_id, type, actor, message, metadata_json)
         VALUES ($1, $2, 'STATE_CHANGED', 'worker', $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(incident_id)
    .bind("Reviewed source revert is current in Production and remained healthy; source and runtime are aligned again")
    .bind(&metadata)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO audit_events (id, project_id, incident_id, actor, action, target, outcome, metadata_json)
         VALUES ($1, $2, $3, 'worker', 'source_recovery.verified', $4, 'success', $5)",
    )
    .bind(Uuid::now_v7())
    .bind(work.project_id)
    .bind(incident_id)
    .bind(deployment_id)
    .bind(&metadata)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn audit_source_recovery_failure(
    database: &Database,
    work: &IncidentWork,
    incident_id: Uuid,
    action: &str,
    target: &str,
    error: &str,
) -> anyhow::Result<()> {
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "worker",
            action,
            target,
            "failure",
            &json!({ "error": truncate_error(error) }),
        )
        .await?;
    Ok(())
}

fn identity(pull_request: &RevertPullRequest) -> RevertIdentity {
    RevertIdentity {
        number: pull_request.number,
        node_id: pull_request.node_id.clone(),
        head_sha: pull_request.head_sha.clone(),
        base_sha: pull_request.base_sha.clone(),
    }
}

fn identity_from_payload(payload: &Value) -> anyhow::Result<RevertIdentity> {
    Ok(RevertIdentity {
        number: payload
            .get("pullRequestNumber")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow::anyhow!("source revert pull request number is missing"))?,
        node_id: required_string(payload, "sourceRevertNodeId")?.to_owned(),
        head_sha: required_string(payload, "sourceRevertHeadSha")?.to_owned(),
        base_sha: required_string(payload, "sourceRevertBaseSha")?.to_owned(),
    })
}

fn pull_request_identity_matches(status: &PullRequestStatus, expected: &RevertIdentity) -> bool {
    status.number == expected.number
        && status.node_id == expected.node_id
        && status.head_sha == expected.head_sha
        && status.base_sha == expected.base_sha
}

fn revert_metadata(pull_request: &RevertPullRequest) -> Value {
    json!({
        "pullRequestNumber": pull_request.number,
        "pullRequestUrl": pull_request.html_url,
        "nodeId": pull_request.node_id,
        "branch": pull_request.branch,
        "headSha": pull_request.head_sha,
        "baseSha": pull_request.base_sha,
        "draft": pull_request.draft
    })
}

fn required_uuid(value: &Value, key: &str) -> anyhow::Result<Uuid> {
    required_string(value, key)?.parse().map_err(Into::into)
}

fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}

fn truncate_error(value: &str) -> String {
    const MAX_CHARS: usize = 1_000;
    value.chars().take(MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_identity() -> RevertIdentity {
        RevertIdentity {
            number: 42,
            node_id: "PR_revert".into(),
            head_sha: "head-sha".into(),
            base_sha: "base-sha".into(),
        }
    }

    #[test]
    fn revert_metadata_keeps_review_identity() {
        let metadata = revert_metadata(&RevertPullRequest {
            number: 42,
            html_url: "https://github.com/example/app/pull/42".into(),
            node_id: "PR_revert".into(),
            branch: "revert-42".into(),
            head_sha: "head-sha".into(),
            base_sha: "base-sha".into(),
            draft: true,
        });
        assert_eq!(metadata["pullRequestNumber"], 42);
        assert_eq!(metadata["branch"], "revert-42");
        assert_eq!(metadata["headSha"], "head-sha");
        assert_eq!(metadata["baseSha"], "base-sha");
        assert_eq!(metadata["draft"], true);
    }

    #[test]
    fn pull_request_status_must_keep_exact_created_identity() {
        let expected = expected_identity();
        let mut status = PullRequestStatus {
            number: 42,
            node_id: "PR_revert".into(),
            draft: false,
            merged_at: Some("2026-08-19T00:00:00Z".into()),
            merge_commit_sha: Some("merge-sha".into()),
            head_sha: "head-sha".into(),
            base_sha: "base-sha".into(),
        };
        assert!(pull_request_identity_matches(&status, &expected));
        status.head_sha = "changed".into();
        assert!(!pull_request_identity_matches(&status, &expected));
    }

    #[test]
    fn source_revert_errors_are_bounded() {
        let value = "x".repeat(2_000);
        assert_eq!(truncate_error(&value).chars().count(), 1_000);
    }
}
