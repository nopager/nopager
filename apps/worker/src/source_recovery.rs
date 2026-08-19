use nopager_connectors::{
    github::GitHubAppAuth,
    github_revert::{RevertPullRequest, RevertPullRequestOutcome, open_revert_pull_request_once},
};
use nopager_core::IncidentState;
use nopager_db::{Database, IncidentTransition, IncidentWork, RepairAttemptWork};
use serde_json::{Value, json};

pub(crate) async fn prepare_draft_source_revert(
    database: &Database,
    work: &IncidentWork,
    incident_id: uuid::Uuid,
    attempt_id: uuid::Uuid,
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
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::RolledBack,
                    next: IncidentState::Escalated,
                    actor: "worker".into(),
                    message: "Production recovered by rollback; a draft source-revert PR was created and requires review".into(),
                    metadata: json!({
                        "deploymentId": known_good_deployment_id,
                        "actionRequired": "review_source_revert",
                        "sourceRepairMerged": true,
                        "sourceRevert": metadata
                    }),
                })
                .await?;
        }
        Ok(RevertPullRequestOutcome::ExistingCandidate(pull_request)) => {
            let metadata = revert_metadata(&pull_request);
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

fn truncate_error(value: &str) -> String {
    const MAX_CHARS: usize = 1_000;
    value.chars().take(MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn source_revert_errors_are_bounded() {
        let value = "x".repeat(2_000);
        assert_eq!(truncate_error(&value).chars().count(), 1_000);
    }
}
