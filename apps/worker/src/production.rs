use nopager_connectors::{
    github::GitHubClient,
    vercel::{Deployment, ProjectLink, VercelClient},
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
pub(crate) enum ProductionDiscovery {
    Pending,
    Ready(ProductionDeployment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProductionLanding {
    Pending { merge_sha: String },
    Ready(ProductionDeployment),
}

pub(crate) async fn find_promoted_production(
    vercel: &VercelClient,
    vercel_project_id: &str,
    repair_commit_sha: &str,
) -> anyhow::Result<ProductionDiscovery> {
    find_current_production_for_commit(vercel, vercel_project_id, repair_commit_sha).await
}

pub(crate) async fn land_and_find_production(
    github: &GitHubClient,
    vercel: &VercelClient,
    work: &IncidentWork,
    attempt: &RepairAttemptWork,
    vercel_project_id: &str,
) -> anyhow::Result<ProductionLanding> {
    ensure_durable_source_compatible(vercel, vercel_project_id, work).await?;

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

    match find_current_production_for_commit(vercel, vercel_project_id, &merge_sha).await? {
        ProductionDiscovery::Pending => Ok(ProductionLanding::Pending { merge_sha }),
        ProductionDiscovery::Ready(deployment) => Ok(ProductionLanding::Ready(deployment)),
    }
}

async fn ensure_durable_source_compatible(
    vercel: &VercelClient,
    vercel_project_id: &str,
    work: &IncidentWork,
) -> anyhow::Result<()> {
    let protected_branch = work
        .github_metadata
        .get("baseBranch")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("protected GitHub default branch metadata is missing"))?;
    let protected_repo_id = work
        .github_metadata
        .get("repoId")
        .and_then(serde_json::Value::as_u64);
    let project = vercel.get_project(vercel_project_id).await?;
    validate_durable_source(
        &work.repo_owner,
        &work.repo_name,
        protected_repo_id,
        protected_branch,
        project.github_link(),
    )
}

fn validate_durable_source(
    protected_owner: &str,
    protected_repo: &str,
    protected_repo_id: Option<u64>,
    protected_branch: &str,
    vercel_link: Option<&ProjectLink>,
) -> anyhow::Result<()> {
    let link = vercel_link.ok_or_else(|| {
        anyhow::anyhow!(
            "Vercel project is not linked to a supported GitHub repository; durable NoPager repair requires Vercel GitHub integration"
        )
    })?;
    let linked_owner = link
        .org
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Vercel GitHub link did not expose a repository owner"))?;
    if !linked_owner.eq_ignore_ascii_case(protected_owner) {
        anyhow::bail!(
            "Vercel GitHub owner '{linked_owner}' does not match protected GitHub owner '{protected_owner}'; refusing durable source mutation"
        );
    }

    let linked_repo = link
        .repo
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    if let Some(linked_repo) = linked_repo
        && !linked_repo.eq_ignore_ascii_case(protected_repo)
    {
        anyhow::bail!(
            "Vercel GitHub repository '{linked_owner}/{linked_repo}' does not match protected repository '{protected_owner}/{protected_repo}'; refusing durable source mutation"
        );
    }
    if let (Some(expected), Some(linked)) = (protected_repo_id, link.repo_id)
        && expected != linked
    {
        anyhow::bail!(
            "Vercel GitHub repository id {linked} does not match protected GitHub repository id {expected}; refusing durable source mutation"
        );
    }
    if linked_repo.is_none() && (protected_repo_id.is_none() || link.repo_id.is_none()) {
        anyhow::bail!(
            "Vercel GitHub link did not expose enough repository identity to prove it matches '{protected_owner}/{protected_repo}'"
        );
    }

    let production_branch = link
        .production_branch
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Vercel GitHub link did not expose an explicit Production Branch; refusing a repair merge whose production path cannot be proven"
            )
        })?;
    if production_branch != protected_branch {
        anyhow::bail!(
            "Vercel Production Branch '{production_branch}' does not match protected GitHub default branch '{protected_branch}'; refusing to merge a repair that would not automatically become production"
        );
    }
    Ok(())
}

async fn find_current_production_for_commit(
    vercel: &VercelClient,
    vercel_project_id: &str,
    commit_sha: &str,
) -> anyhow::Result<ProductionDiscovery> {
    let deployments = vercel.list_deployments(vercel_project_id, 50).await?;
    let Some(candidate) = newest_production_candidate(deployments, commit_sha) else {
        return Ok(ProductionDiscovery::Pending);
    };

    if deployment_failed(&candidate) {
        anyhow::bail!(
            "Vercel production deployment {} failed before becoming current",
            candidate.id
        );
    }

    match current_production_readiness(vercel, &candidate.id, commit_sha).await? {
        ProductionReadiness::Pending => Ok(ProductionDiscovery::Pending),
        ProductionReadiness::Ready => {
            let current = vercel.get_deployment(&candidate.id).await?;
            Ok(ProductionDiscovery::Ready(ProductionDeployment {
                id: current.id,
                commit_sha: commit_sha.to_owned(),
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

fn newest_production_candidate(
    deployments: Vec<Deployment>,
    commit_sha: &str,
) -> Option<Deployment> {
    deployments
        .into_iter()
        .filter(|deployment| {
            deployment.target.as_deref() == Some("production")
                && deployment_matches_commit(deployment, commit_sha)
        })
        .max_by_key(|deployment| deployment.created.unwrap_or_default())
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

    fn deployment(
        id: &str,
        sha: &str,
        ready: &str,
        target: Option<&str>,
        live: Option<bool>,
        created: i64,
    ) -> Deployment {
        Deployment {
            id: id.into(),
            url: format!("{id}.example.vercel.app"),
            ready_state: Some(ready.into()),
            state: None,
            created: Some(created),
            target: target.map(ToOwned::to_owned),
            live,
            meta: json!({ "githubCommitSha": sha }),
        }
    }

    fn github_link(
        owner: &str,
        repo: Option<&str>,
        repo_id: Option<u64>,
        branch: Option<&str>,
    ) -> ProjectLink {
        ProjectLink {
            kind: Some("github".into()),
            org: Some(owner.into()),
            repo: repo.map(ToOwned::to_owned),
            repo_id,
            production_branch: branch.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn durable_source_merge_requires_matching_github_identity_and_branch() {
        let matching = github_link("Example", Some("App"), Some(42), Some("main"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&matching)).is_ok()
        );

        let wrong_owner = github_link("other", Some("app"), Some(42), Some("main"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&wrong_owner))
                .is_err()
        );

        let wrong_repo = github_link("example", Some("other"), Some(42), Some("main"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&wrong_repo)).is_err()
        );

        let wrong_id = github_link("example", Some("app"), Some(99), Some("main"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&wrong_id)).is_err()
        );

        let wrong_branch = github_link("example", Some("app"), Some(42), Some("production"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&wrong_branch))
                .is_err()
        );

        let missing_identity = github_link("example", None, None, Some("main"));
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&missing_identity))
                .is_err()
        );

        let missing_branch = github_link("example", Some("app"), Some(42), None);
        assert!(
            validate_durable_source("example", "app", Some(42), "main", Some(&missing_branch))
                .is_err()
        );
        assert!(validate_durable_source("example", "app", Some(42), "main", None).is_err());
    }

    #[test]
    fn durable_source_can_prove_repository_by_id_when_name_is_absent() {
        let link = github_link("example", None, Some(42), Some("release"));
        assert!(
            validate_durable_source("example", "app", Some(42), "release", Some(&link)).is_ok()
        );
    }

    #[test]
    fn production_candidate_must_match_commit_and_exclude_preview_build() {
        let preview = deployment(
            "dpl_preview",
            "repair-sha",
            "READY",
            Some("preview"),
            Some(false),
            30,
        );
        let production = deployment(
            "dpl_production",
            "repair-sha",
            "BUILDING",
            Some("production"),
            Some(false),
            20,
        );
        let candidate =
            newest_production_candidate(vec![preview, production.clone()], "repair-sha").unwrap();
        assert_eq!(candidate.id, production.id);
        assert!(deployment_matches_commit(&candidate, "repair-sha"));
        assert!(!deployment_matches_commit(&candidate, "preview-sha"));
    }

    #[test]
    fn newest_matching_production_build_wins() {
        let old = deployment(
            "dpl_old",
            "merge-sha",
            "READY",
            Some("production"),
            Some(false),
            10,
        );
        let new = deployment(
            "dpl_new",
            "merge-sha",
            "BUILDING",
            Some("production"),
            Some(false),
            20,
        );
        let candidate = newest_production_candidate(vec![old, new], "merge-sha").unwrap();
        assert_eq!(candidate.id, "dpl_new");
    }

    #[test]
    fn failed_production_build_is_not_treated_as_pending_success() {
        for state in ["ERROR", "CANCELED", "CANCELLED"] {
            assert!(deployment_failed(&deployment(
                "dpl_failed",
                "merge-sha",
                state,
                Some("production"),
                Some(false),
                1,
            )));
        }
        assert!(!deployment_failed(&deployment(
            "dpl_ready",
            "merge-sha",
            "READY",
            Some("production"),
            Some(true),
            1,
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
