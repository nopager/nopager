use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, RequestBuilder};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::{ConnectorError, decode, github::GitHubAppAuth};

const API_VERSION: &str = "2022-11-28";
const GITHUB_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestStatus {
    pub number: u64,
    pub node_id: String,
    pub draft: bool,
    pub merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub head_sha: String,
    pub base_sha: String,
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

#[derive(Debug, Deserialize)]
struct PullRequestResponse {
    number: u64,
    node_id: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
    head: GitObject,
    base: GitObject,
}

#[derive(Debug, Deserialize)]
struct GitObject {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GitRefResponse {
    object: GitObject,
}

pub async fn get_pull_request_status(
    auth: &GitHubAppAuth,
    owner: &str,
    repository: &str,
    number: u64,
) -> Result<PullRequestStatus, ConnectorError> {
    validate_segment(owner, "owner")?;
    validate_segment(repository, "repository")?;
    if number == 0 {
        return Err(ConnectorError::InvalidConfiguration(
            "pull request number must be positive".into(),
        ));
    }

    let http = github_http_client()?;
    let token = installation_token(auth, repository, &http).await?;
    let path = format!("repos/{owner}/{repository}/pulls/{number}");
    let response: PullRequestResponse = decode(
        request(&http, auth, &token, reqwest::Method::GET, &path)?
            .send()
            .await?,
    )
    .await?;
    pull_request_status(response)
}

pub async fn get_branch_head(
    auth: &GitHubAppAuth,
    owner: &str,
    repository: &str,
    branch: &str,
) -> Result<String, ConnectorError> {
    validate_segment(owner, "owner")?;
    validate_segment(repository, "repository")?;
    validate_git_ref(branch, "branch")?;

    let http = github_http_client()?;
    let token = installation_token(auth, repository, &http).await?;
    let path = format!("repos/{owner}/{repository}/git/ref/heads/{branch}");
    let response: GitRefResponse = decode(
        request(&http, auth, &token, reqwest::Method::GET, &path)?
            .send()
            .await?,
    )
    .await?;
    let sha = response.object.sha;
    if sha.trim().is_empty() {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub branch ref response omitted object SHA".into(),
        });
    }
    Ok(sha)
}

fn pull_request_status(response: PullRequestResponse) -> Result<PullRequestStatus, ConnectorError> {
    if response.number == 0
        || response.node_id.trim().is_empty()
        || response.head.sha.trim().is_empty()
        || response.base.sha.trim().is_empty()
    {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub pull request response omitted required identity".into(),
        });
    }
    if response
        .merge_commit_sha
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(ConnectorError::Api {
            status: reqwest::StatusCode::BAD_GATEWAY,
            message: "GitHub pull request response returned an empty merge commit SHA".into(),
        });
    }
    Ok(PullRequestStatus {
        number: response.number,
        node_id: response.node_id,
        draft: response.draft,
        merged_at: response.merged_at,
        merge_commit_sha: response.merge_commit_sha,
        head_sha: response.head.sha,
        base_sha: response.base.sha,
    })
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
                "contents": "read",
                "pull_requests": "read"
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

    fn response(merged: bool) -> PullRequestResponse {
        PullRequestResponse {
            number: 42,
            node_id: "PR_node".into(),
            draft: !merged,
            merged_at: merged.then(|| "2026-08-19T00:00:00Z".into()),
            merge_commit_sha: merged.then(|| "merge-sha".into()),
            head: GitObject {
                sha: "head-sha".into(),
            },
            base: GitObject {
                sha: "base-sha".into(),
            },
        }
    }

    #[test]
    fn keeps_provider_attested_pull_request_identity() {
        let status = pull_request_status(response(true)).unwrap();
        assert_eq!(status.number, 42);
        assert_eq!(status.node_id, "PR_node");
        assert_eq!(status.head_sha, "head-sha");
        assert_eq!(status.base_sha, "base-sha");
        assert_eq!(status.merge_commit_sha.as_deref(), Some("merge-sha"));
        assert!(status.merged_at.is_some());
    }

    #[test]
    fn rejects_incomplete_pull_request_identity() {
        let mut value = response(false);
        value.node_id.clear();
        assert!(pull_request_status(value).is_err());
    }

    #[test]
    fn accepts_protected_branch_names_with_path_components() {
        assert!(validate_git_ref("main", "branch").is_ok());
        assert!(validate_git_ref("release/2026.08", "branch").is_ok());
        assert!(validate_git_ref("feature//bad", "branch").is_err());
    }
}
