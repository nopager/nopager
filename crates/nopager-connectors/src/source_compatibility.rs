use thiserror::Error;

use crate::vercel::ProjectDetails;

#[derive(Debug, Clone, Copy)]
pub struct GitHubSourceIdentity<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub repo_id: Option<u64>,
    pub default_branch: &'a str,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SourceCompatibilityError {
    #[error("Vercel project is not linked to a supported GitHub repository")]
    UnsupportedGitLink,
    #[error("Vercel GitHub link did not expose a repository owner")]
    MissingOwner,
    #[error("Vercel GitHub owner '{linked}' does not match protected GitHub owner '{expected}'")]
    OwnerMismatch { linked: String, expected: String },
    #[error("Vercel GitHub repository '{linked}' does not match protected repository '{expected}'")]
    RepositoryMismatch { linked: String, expected: String },
    #[error(
        "Vercel GitHub repository id {linked} does not match protected GitHub repository id {expected}"
    )]
    RepositoryIdMismatch { linked: u64, expected: u64 },
    #[error(
        "Vercel GitHub link did not expose enough repository identity to prove the protected repository"
    )]
    MissingRepositoryIdentity,
    #[error("Vercel GitHub link did not expose an explicit Production Branch")]
    MissingProductionBranch,
    #[error(
        "Vercel Production Branch '{linked}' does not match protected GitHub default branch '{expected}'"
    )]
    ProductionBranchMismatch { linked: String, expected: String },
}

pub fn validate_vercel_github_source(
    project: &ProjectDetails,
    expected: GitHubSourceIdentity<'_>,
) -> Result<(), SourceCompatibilityError> {
    let link = project
        .github_link()
        .ok_or(SourceCompatibilityError::UnsupportedGitLink)?;
    let linked_owner = link
        .org
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(SourceCompatibilityError::MissingOwner)?;
    if !linked_owner.eq_ignore_ascii_case(expected.owner) {
        return Err(SourceCompatibilityError::OwnerMismatch {
            linked: linked_owner.to_owned(),
            expected: expected.owner.to_owned(),
        });
    }

    let linked_repo = link
        .repo
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    if let Some(linked_repo) = linked_repo
        && !linked_repo.eq_ignore_ascii_case(expected.repo)
    {
        return Err(SourceCompatibilityError::RepositoryMismatch {
            linked: format!("{linked_owner}/{linked_repo}"),
            expected: format!("{}/{}", expected.owner, expected.repo),
        });
    }
    if let (Some(expected_id), Some(linked_id)) = (expected.repo_id, link.repo_id)
        && expected_id != linked_id
    {
        return Err(SourceCompatibilityError::RepositoryIdMismatch {
            linked: linked_id,
            expected: expected_id,
        });
    }
    if linked_repo.is_none() && (expected.repo_id.is_none() || link.repo_id.is_none()) {
        return Err(SourceCompatibilityError::MissingRepositoryIdentity);
    }

    let production_branch = project
        .git_production_branch()
        .ok_or(SourceCompatibilityError::MissingProductionBranch)?;
    if production_branch != expected.default_branch {
        return Err(SourceCompatibilityError::ProductionBranchMismatch {
            linked: production_branch.to_owned(),
            expected: expected.default_branch.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn project(link: serde_json::Value) -> ProjectDetails {
        serde_json::from_value(json!({ "id": "prj_1", "name": "demo", "link": link })).unwrap()
    }

    fn expected() -> GitHubSourceIdentity<'static> {
        GitHubSourceIdentity {
            owner: "example",
            repo: "app",
            repo_id: Some(42),
            default_branch: "main",
        }
    }

    #[test]
    fn matching_github_source_is_compatible() {
        let project = project(json!({
            "type": "github",
            "org": "Example",
            "repo": "App",
            "repoId": 42,
            "productionBranch": "main"
        }));
        assert_eq!(validate_vercel_github_source(&project, expected()), Ok(()));
    }

    #[test]
    fn rejects_other_provider_repository_id_and_branch() {
        let gitlab = project(json!({ "type": "gitlab", "productionBranch": "main" }));
        assert_eq!(
            validate_vercel_github_source(&gitlab, expected()),
            Err(SourceCompatibilityError::UnsupportedGitLink)
        );

        let wrong_repo = project(json!({
            "type": "github",
            "org": "example",
            "repo": "other",
            "repoId": 42,
            "productionBranch": "main"
        }));
        assert!(matches!(
            validate_vercel_github_source(&wrong_repo, expected()),
            Err(SourceCompatibilityError::RepositoryMismatch { .. })
        ));

        let wrong_id = project(json!({
            "type": "github",
            "org": "example",
            "repo": "app",
            "repoId": 99,
            "productionBranch": "main"
        }));
        assert_eq!(
            validate_vercel_github_source(&wrong_id, expected()),
            Err(SourceCompatibilityError::RepositoryIdMismatch {
                linked: 99,
                expected: 42
            })
        );

        let wrong_branch = project(json!({
            "type": "github",
            "org": "example",
            "repo": "app",
            "repoId": 42,
            "productionBranch": "production"
        }));
        assert!(matches!(
            validate_vercel_github_source(&wrong_branch, expected()),
            Err(SourceCompatibilityError::ProductionBranchMismatch { .. })
        ));
    }

    #[test]
    fn repository_id_can_prove_identity_when_name_is_missing() {
        let project = project(json!({
            "type": "github-limited",
            "org": "example",
            "repoId": 42,
            "productionBranch": "main"
        }));
        assert_eq!(validate_vercel_github_source(&project, expected()), Ok(()));
    }

    #[test]
    fn missing_identity_or_branch_fails_closed() {
        let missing_identity = project(json!({
            "type": "github",
            "org": "example",
            "productionBranch": "main"
        }));
        assert_eq!(
            validate_vercel_github_source(&missing_identity, expected()),
            Err(SourceCompatibilityError::MissingRepositoryIdentity)
        );

        let missing_branch = project(json!({
            "type": "github",
            "org": "example",
            "repo": "app",
            "repoId": 42
        }));
        assert_eq!(
            validate_vercel_github_source(&missing_branch, expected()),
            Err(SourceCompatibilityError::MissingProductionBranch)
        );
    }
}
