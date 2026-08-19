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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductionReadiness {
    Pending,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProductionLanding {
    Pending { merge_sha: String },
    Ready(ProductionDeployment),
}

pub(crate) async fn land_and_find_production(
    github: &GitHubClient,
    vercel: &VercelClient,
    work: &IncidentWork,
    attempt: &RepairAttemptWork,
    vercel_project_id: &str,
) -> anyhow::Result<ProductionLanding> {
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
    let Some(candidate) = deployments
        .into_iter()
        .find(|deployment| deployment_matches_commit(deployment, &merge_sha))
    else {
        return Ok(ProductionLanding::Pending { merge_sha });
    };

    if deployment_failed(&candidate) {
        anyhow::bail!(
            "Vercel production deployment {} failed before becoming current",
            candidate.id
        );
    }

    match current_production_readiness(vercel, &candidate.id, &merge_sha).await? {
        ProductionReadiness::Pending => Ok(ProductionLanding::Pending { merge_sha }),
        ProductionReadiness::Ready => {
            let current = vercel.get_deployment(&candidate.id).await?;
            Ok(ProductionLanding::Ready(ProductionDeployment {
                id: current.id,
                commit_sha: merge_sha,
                url: https_url(&current.url),
            }))
        }
    }
}

pub(crate) async fn current_production_readiness(
    vercel: &VercelClient,
    deployment_id: &str,
    commit_sha: &str,
) -> anyhow::Result<ProductionReadiness> {
    let current = vercel.get_deployment(deployment_id).await?;
    if deployment_failed(&current) {
        anyhow::bail!("Vercel production deployment {deployment_id} failed");
    }
    if !deployment_matches_commit(&current, commit_sha) {
        anyhow::bail!(
            "Vercel production deployment {deployment_id} no longer matches repair commit {commit_sha}"
        );
    }
    if current.ready_state.as_deref() != Some("READY")
        || current.target.as_deref() != Some("production")
    {
        return Ok(ProductionReadiness::Pending);
    }
    Ok(ProductionReadiness::Ready)
}

pub(crate) async fn verify_current_production(
    vercel: &VercelClient,
    deployment_id: &str,
    commit_sha: &str,
) -> anyhow::Result<()> {
    match current_production_readiness(vercel, deployment_id, commit_sha).await? {
        ProductionReadiness::Ready => Ok(()),
        ProductionReadiness::Pending => {
            anyhow::bail!("Vercel production deployment {deployment_id} is not current/live yet")
        }
    }
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
