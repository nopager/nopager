use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, RequestBuilder};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ConnectorError, decode, github::GitHubAppAuth};

const API_VERSION: &str = "2022-11-28";
const GITHUB_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_EXISTING_REVERT_PAGES: u8 = 5;
const REVERT_PAGE_SIZE: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertPullRequest {
    pub number: u64,
    pub html_url: String,
    pub node_id: String,
    pub branch: String,
    pub head_sha: String,
    pub base_sha: String,
    pub draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertPullRequestOutcome {
    /// A pull request carrying NoPager's deterministic marker already exists.
    /// This is useful for at-most-once recovery, but the marker alone is not
    /// provider-attested proof that the existing PR was created by the revert
    /// mutation. Callers must not auto-merge this variant without separate
    /// verification.
    ExistingCandidate(RevertPullRequest),
    /// GitHub confirmed this exact call created the revert pull request.
    Created(RevertPullRequest),
}

#[derive(Debug, Serialize)]
struct Claims {
    iat: u64,
    exp: u64,
    iss: String,
}

#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RestPullRequest {
    number: u64,
    html_url: String,
    node_id: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
    head: RestGitObject,
    base: RestGitObject,
}

#[derive(Debug, Clone, Deserialize)]
struct RestGitObject {
    sha: String,
    #[serde(rename = "ref")]
    ref_name: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlEnvelope {
    data: Option<GraphQlData>,
    #[serde(default)]
    errors: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlData {
    revert_pull_request: Option<RevertPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevertPayload {
    pull_request: Option<OriginalPullRequest>,
    revert_pull_request: Option<GraphPullRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OriginalPullRequest {
    number: u64,
    id: String,
    merge_commit: Option<GraphCommit>,
}

#[derive(Debug, Deserialize)]
struct GraphCommit {
    oid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphPullRequest {
    number: u64,
    url: String,
    id: String,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
    base_ref_oid: String,
}

pub async fn open_revert_pull_request_once(
    auth: &GitHubAppAuth,
    owner: &str,
    repository: &str,
    repair_branch: &str,
    expected_repair_head_sha: &str,
    expected_repair_base_sha: &str,
) -> Result<RevertPullRequestOutcome, ConnectorError> {
    validate_segment(owner, "owner")?;
    validate_segment(repository, "repository")?;
    validate_git_ref(repair_branch, "repair branch")?;
    validate_segment(expected_repair_head_sha, "repair head SHA")?;
    validate_segment(expected_repair_base_sha, "repair base SHA")?;

    let http = github_http_client()?;
    let token = installation_token(auth, repository, &http).await?;
    let repo = format!("repos/{owner}/{repository}");
    let pulls: Vec<RestPullRequest> = decode(
        request(
            &http,
            auth,
            &token,
            reqwest::Method::GET,
            &format!("{repo}/pulls"),
        )?
        .query(&[
            ("state", "all"),
            ("head", &format!("{owner}:{repair_branch}")),
            ("per_page", "20"),
        ])
        .send()
        .await?,
    )
    .await?;
    let repair = pulls
        .into_iter()
        .find(|pull_request| pull_request.head.sha == expected_repair_head_sha)
        .ok_or_else(|| {
            ConnectorError::InvalidConfiguration(
                "merged NoPager repair pull request could not be found at the expected head SHA"
                    .into(),
            )
        })?;
    validate_merged_repair(&repair, expected_repair_head_sha, expected_repair_base_sha)?;
    let merge_sha = repair
        .merge_commit_sha
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "merged GitHub repair pull request omitted merge commit SHA".into(),
        })?;
    let marker = revert_marker(owner, repository, repair.number, merge_sha);

    if let Some(existing) =
        find_existing_revert_candidate(&http, auth, &token, &repo, &marker).await?
    {
        return Ok(RevertPullRequestOutcome::ExistingCandidate(existing));
    }

    let endpoint = auth
        .api_base
        .join("graphql")
        .map_err(|error| ConnectorError::InvalidConfiguration(error.to_string()))?;
    let title = format!("Revert failed NoPager repair #{}", repair.number);
    let body = format!(
        "Production traffic was restored to the previous known-good deployment after the merged NoPager repair failed durable Production verification. This draft reverts the merged source repair so the protected branch can converge back to the recovered runtime state.\n\n{marker}\n\nReview the revert and repository checks before merging it."
    );
    let response = github_headers(http.post(endpoint))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "query": "mutation CreateNoPagerRevert($pullRequestId: ID!, $title: String!, $body: String!, $draft: Boolean!) { revertPullRequest(input: { pullRequestId: $pullRequestId, title: $title, body: $body, draft: $draft }) { pullRequest { number id mergeCommit { oid } } revertPullRequest { number url id isDraft headRefName headRefOid baseRefOid } } }",
            "variables": {
                "pullRequestId": repair.node_id,
                "title": title,
                "body": body,
                "draft": true
            }
        }))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(ConnectorError::Api {
            status,
            message: response.text().await.unwrap_or_default(),
        });
    }
    let envelope: GraphQlEnvelope = response.json().await?;
    if !envelope.errors.is_empty() {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            message: Value::Array(envelope.errors).to_string(),
        });
    }
    let payload = envelope
        .data
        .and_then(|data| data.revert_pull_request)
        .ok_or_else(|| ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub revertPullRequest response omitted payload".into(),
        })?;
    let original = payload.pull_request.ok_or_else(|| ConnectorError::Api {
        status: reqwest::StatusCode::BAD_GATEWAY,
        message: "GitHub revertPullRequest response omitted original pull request".into(),
    })?;
    if original.number != repair.number || original.id != repair.node_id {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message:
                "GitHub revertPullRequest response referenced an unexpected original pull request"
                    .into(),
        });
    }
    if original
        .merge_commit
        .as_ref()
        .map(|commit| commit.oid.as_str())
        != Some(merge_sha)
    {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub revertPullRequest response referenced an unexpected merge commit"
                .into(),
        });
    }
    let revert = payload
        .revert_pull_request
        .ok_or_else(|| ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub revertPullRequest response omitted the new revert pull request".into(),
        })?;
    let created = graph_revert(revert)?;
    if !created.draft {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub did not create the source-recovery revert as a draft pull request"
                .into(),
        });
    }
    Ok(RevertPullRequestOutcome::Created(created))
}

async fn find_existing_revert_candidate(
    http: &Client,
    auth: &GitHubAppAuth,
    token: &str,
    repo: &str,
    marker: &str,
) -> Result<Option<RevertPullRequest>, ConnectorError> {
    for page in 1..=MAX_EXISTING_REVERT_PAGES {
        let page_text = page.to_string();
        let page_size = REVERT_PAGE_SIZE.to_string();
        let pulls: Vec<RestPullRequest> = decode(
            request(
                http,
                auth,
                token,
                reqwest::Method::GET,
                &format!("{repo}/pulls"),
            )?
            .query(&[
                ("state", "all"),
                ("sort", "created"),
                ("direction", "desc"),
                ("per_page", page_size.as_str()),
                ("page", page_text.as_str()),
            ])
            .send()
            .await?,
        )
        .await?;
        let count = pulls.len();
        if let Some(existing) = pulls.into_iter().find(|pull_request| {
            pull_request
                .body
                .as_deref()
                .is_some_and(|body| body.contains(marker))
        }) {
            return Ok(Some(rest_revert(existing)?));
        }
        if count < REVERT_PAGE_SIZE as usize {
            return Ok(None);
        }
    }

    Err(ConnectorError::InvalidConfiguration(format!(
        "GitHub pull request history exceeds the bounded NoPager revert scan of {} pull requests; refusing another source-revert mutation until an existing revert can be ruled out",
        MAX_EXISTING_REVERT_PAGES as usize * REVERT_PAGE_SIZE as usize
    )))
}

fn rest_revert(pull_request: RestPullRequest) -> Result<RevertPullRequest, ConnectorError> {
    if pull_request.node_id.trim().is_empty()
        || pull_request.html_url.trim().is_empty()
        || pull_request.head.ref_name.trim().is_empty()
        || pull_request.head.sha.trim().is_empty()
        || pull_request.base.sha.trim().is_empty()
    {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub existing revert candidate omitted required pull request identity"
                .into(),
        });
    }
    Ok(RevertPullRequest {
        number: pull_request.number,
        html_url: pull_request.html_url,
        node_id: pull_request.node_id,
        branch: pull_request.head.ref_name,
        head_sha: pull_request.head.sha,
        base_sha: pull_request.base.sha,
        draft: pull_request.draft,
    })
}

fn graph_revert(pull_request: GraphPullRequest) -> Result<RevertPullRequest, ConnectorError> {
    if pull_request.id.trim().is_empty()
        || pull_request.url.trim().is_empty()
        || pull_request.head_ref_name.trim().is_empty()
        || pull_request.head_ref_oid.trim().is_empty()
        || pull_request.base_ref_oid.trim().is_empty()
    {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub revertPullRequest response omitted required pull request identity"
                .into(),
        });
    }
    Ok(RevertPullRequest {
        number: pull_request.number,
        html_url: pull_request.url,
        node_id: pull_request.id,
        branch: pull_request.head_ref_name,
        head_sha: pull_request.head_ref_oid,
        base_sha: pull_request.base_ref_oid,
        draft: pull_request.is_draft,
    })
}

fn validate_merged_repair(
    pull_request: &RestPullRequest,
    expected_head_sha: &str,
    expected_base_sha: &str,
) -> Result<(), ConnectorError> {
    if pull_request.head.sha != expected_head_sha {
        return Err(ConnectorError::InvalidConfiguration(
            "repair pull request head changed after verification".into(),
        ));
    }
    if pull_request.base.sha != expected_base_sha {
        return Err(ConnectorError::InvalidConfiguration(
            "repair pull request base changed after verification".into(),
        ));
    }
    if pull_request.merged_at.is_none() {
        return Err(ConnectorError::InvalidConfiguration(
            "source recovery requires a merged repair pull request".into(),
        ));
    }
    Ok(())
}

fn revert_marker(owner: &str, repository: &str, pull_number: u64, merge_sha: &str) -> String {
    format!(
        "<!-- nopager-source-revert:{owner}/{repository}:repair-pr-{pull_number}:merge-{merge_sha} -->"
    )
}

async fn installation_token(
    auth: &GitHubAppAuth,
    repository: &str,
    http: &Client,
) -> Result<String, ConnectorError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ConnectorError::Credential(error.to_string()))?
        .as_secs();
    let endpoint = auth
        .api_base
        .join(&format!(
            "app/installations/{}/access_tokens",
            auth.installation_id
        ))
        .map_err(|error| ConnectorError::InvalidConfiguration(error.to_string()))?;
    let response = github_headers(http.post(endpoint))
        .bearer_auth(jwt_at(auth, now)?)
        .json(&serde_json::json!({
            "repositories": [repository],
            "permissions": {
                "contents": "write",
                "pull_requests": "write"
            }
        }))
        .send()
        .await?;
    Ok(decode::<InstallationTokenResponse>(response).await?.token)
}

fn jwt_at(auth: &GitHubAppAuth, now: u64) -> Result<String, ConnectorError> {
    let key = EncodingKey::from_rsa_pem(auth.private_key_pem.expose_secret().as_bytes())
        .map_err(|error| ConnectorError::Credential(error.to_string()))?;
    encode(
        &Header::new(Algorithm::RS256),
        &Claims {
            iat: now.saturating_sub(60),
            exp: now + 540,
            iss: auth.app_id.to_string(),
        },
        &key,
    )
    .map_err(|error| ConnectorError::Credential(error.to_string()))
}

fn github_http_client() -> Result<Client, ConnectorError> {
    Ok(Client::builder()
        .connect_timeout(GITHUB_CONNECT_TIMEOUT)
        .timeout(GITHUB_REQUEST_TIMEOUT)
        .build()?)
}

fn request(
    http: &Client,
    auth: &GitHubAppAuth,
    token: &str,
    method: reqwest::Method,
    path: &str,
) -> Result<RequestBuilder, ConnectorError> {
    let url = auth
        .api_base
        .join(path)
        .map_err(|error| ConnectorError::InvalidConfiguration(error.to_string()))?;
    Ok(github_headers(http.request(method, url)).bearer_auth(token))
}

fn github_headers(request: RequestBuilder) -> RequestBuilder {
    request
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", "NoPager")
}

fn validate_segment(value: &str, label: &str) -> Result<(), ConnectorError> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err(ConnectorError::InvalidConfiguration(format!(
            "invalid {label}"
        )));
    }
    Ok(())
}

fn validate_git_ref(value: &str, label: &str) -> Result<(), ConnectorError> {
    let invalid_component = value.split('/').any(|component| {
        component.is_empty()
            || component.starts_with('.')
            || component.ends_with('.')
            || component.ends_with(".lock")
    });
    let invalid_character = value.chars().any(|character| {
        character.is_ascii_control()
            || character == ' '
            || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    });
    if value.is_empty()
        || value == "@"
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || invalid_component
        || invalid_character
    {
        return Err(ConnectorError::InvalidConfiguration(format!(
            "invalid {label}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest_pull(body: Option<&str>) -> RestPullRequest {
        RestPullRequest {
            number: 7,
            html_url: "https://github.com/example/app/pull/7".into(),
            node_id: "PR_revert".into(),
            body: body.map(ToOwned::to_owned),
            draft: true,
            merged_at: None,
            merge_commit_sha: None,
            head: RestGitObject {
                sha: "revert-sha".into(),
                ref_name: "revert-7".into(),
            },
            base: RestGitObject {
                sha: "base-sha".into(),
                ref_name: "main".into(),
            },
        }
    }

    #[test]
    fn revert_marker_binds_repository_pr_and_merge_identity() {
        assert_eq!(
            revert_marker("example", "app", 42, "merge-sha"),
            "<!-- nopager-source-revert:example/app:repair-pr-42:merge-merge-sha -->"
        );
    }

    #[test]
    fn existing_candidate_keeps_draft_and_git_identity() {
        let candidate = rest_revert(rest_pull(Some("marker"))).unwrap();
        assert_eq!(candidate.number, 7);
        assert_eq!(candidate.branch, "revert-7");
        assert_eq!(candidate.head_sha, "revert-sha");
        assert_eq!(candidate.base_sha, "base-sha");
        assert!(candidate.draft);
    }

    #[test]
    fn merged_repair_validation_requires_exact_verified_identity() {
        let mut repair = rest_pull(None);
        repair.merged_at = Some("2026-08-19T00:00:00Z".into());
        repair.merge_commit_sha = Some("merge-sha".into());
        repair.head.sha = "repair-sha".into();
        assert!(validate_merged_repair(&repair, "repair-sha", "base-sha").is_ok());
        assert!(validate_merged_repair(&repair, "other", "base-sha").is_err());
        assert!(validate_merged_repair(&repair, "repair-sha", "other").is_err());
    }
}
