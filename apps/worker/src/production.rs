use nopager_connectors::{
    github::GitHubClient,
    vercel::{Deployment, VercelClient},
};
use nopager_db::{IncidentWork, RepairAttemptWork};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductionDeployment {
    pub id: String,
    pub commit_sha: String,
    pub url: String,
}

pub(crate) async fn land_and_find_production(
    github: &GitHubClient,
    vercel: &VercelClient,
    work: &IncidentWork,
    attempt: &RepairAttemptWork,
    vercel_project_id: &str,
) -> anyhow::Result<ProductionDeployment> {
    let repair_branch = attempt
        .repair_branch
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("repair branch is missing"))?;
    let repair_commit_sha = attempt
        .repair_commit_sha
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("repair commit SHA is missing"))?;

    let merge_sha = github
        .land_repair_pr(
            &work.repo_owner,
            &work.repo_name,
            repair_branch,
            repair_commit_sha,
            &attempt.base_commit_sha,
        )
        .await?;

    let deployments = vercel.list_deployments(vercel_project_id, 50).await?;
    let candidate = deployments
        .into_iter()
        .find(|deployment| deployment_matches_commit(deployment, &merge_sha))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Vercel has not created the production deployment for merged repair {merge_sha} yet"
            )
        })?;

    if deployment_failed(&candidate) {
        anyhow::bail!(
            "Vercel production deployment {} failed before becoming current",
            candidate.id
        );
    }

    let current = vercel.get_deployment(&candidate.id).await?;
    if deployment_failed(&current) {
        anyhow::bail!(
            "Vercel production deployment {} failed before becoming current",
            current.id
        );
    }
    if current.ready_state.as_deref() != Some("READY") {
        anyhow::bail!(
            "Vercel production deployment {} is not READY yet",
            current.id
        );
    }
    if current.target.as_deref() != Some("production") {
        anyhow::bail!(
            "Vercel production deployment {} is not current/live yet",
            current.id
        );
    }
    if !deployment_matches_commit(&current, &merge_sha) {
        anyhow::bail!(
            "Vercel current production deployment no longer matches merged repair {merge_sha}"
        );
    }

    Ok(ProductionDeployment {
        id: current.id,
        commit_sha: merge_sha,
        url: https_url(&current.url),
    })
}

fn deployment_matches_commit(deployment: &Deployment, commit_sha: &str) -> bool {
    deployment
        .meta
        .get("githubCommitSha")
        .and_then(serde_json::Value::as_str)
        == Some(commit_sha)
}

fn deployment_failed(deployment: &Deployment) -> bool {
    matches!(
        deployment
            .ready_state
            .as_deref()
            .or(deployment.state.as_deref()),
        Some("ERROR" | "CANCELED" | "CANCELLED")
    )
}

fn https_url(value: &str) -> String {
    if value.starts_with("https://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deployment(sha: &str, ready: &str, target: Option<&str>, live: Option<bool>) -> Deployment {
        Deployment {
            id: "dpl_repair".into(),
            url: "repair.example.vercel.app".into(),
            ready_state: Some(ready.into()),
            state: None,
            created: None,
            target: target.map(ToOwned::to_owned),
            live,
            meta: json!({ "githubCommitSha": sha }),
        }
    }

    #[test]
    fn production_candidate_must_match_the_merge_commit() {
        let candidate = deployment("merge-sha", "READY", Some("production"), Some(true));
        assert!(deployment_matches_commit(&candidate, "merge-sha"));
        assert!(!deployment_matches_commit(&candidate, "preview-sha"));
    }

    #[test]
    fn failed_production_build_is_not_treated_as_pending_success() {
        for state in ["ERROR", "CANCELED", "CANCELLED"] {
            assert!(deployment_failed(&deployment(
                "merge-sha",
                state,
                Some("production"),
                Some(false)
            )));
        }
        assert!(!deployment_failed(&deployment(
            "merge-sha",
            "READY",
            Some("production"),
            Some(true)
        )));
    }

    #[test]
    fn production_urls_are_normalized_to_https() {
        assert_eq!(
            https_url("repair.example.vercel.app"),
            "https://repair.example.vercel.app"
        );
        assert_eq!(https_url("https://example.com"), "https://example.com");
    }
}
