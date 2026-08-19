use std::{collections::BTreeMap, time::Duration};

use reqwest::{Client, Method, RequestBuilder};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::{ConnectorError, decode, expect_success};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PREVIEW_SECRETS_OVERRIDE: &str = "NOPAGER_ALLOW_PREVIEW_SECRETS";
const VERCEL_PAGE_LIMIT: u8 = 100;
const MAX_ENVIRONMENT_VARIABLE_PAGES: usize = 10;

#[derive(Clone)]
pub struct VercelClient {
    http: Client,
    token: SecretString,
    team_id: Option<String>,
    api_base: Url,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub ready_state: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub live: Option<bool>,
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct DeploymentList {
    pub deployments: Vec<Deployment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetails {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub link: Option<ProjectLink>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLink {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub repo_id: Option<u64>,
    #[serde(default)]
    pub production_branch: Option<String>,
}

impl ProjectDetails {
    pub fn github_link(&self) -> Option<&ProjectLink> {
        let link = self.link.as_ref()?;
        match link.kind.as_deref() {
            Some("github" | "github-limited") => Some(link),
            _ => None,
        }
    }

    pub fn git_production_branch(&self) -> Option<&str> {
        self.github_link()?
            .production_branch
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvironmentVariable {
    pub key: String,
    #[serde(default)]
    pub target: Vec<String>,
    #[serde(default, rename = "type")]
    pub variable_type: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    #[serde(default)]
    next: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ProjectEnvironmentVariableList {
    #[serde(default)]
    envs: Vec<ProjectEnvironmentVariable>,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitSource {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "repoId")]
    pub repo_id: u64,
    pub ref_name: String,
    pub sha: String,
}

impl VercelClient {
    pub fn new(token: SecretString, team_id: Option<String>) -> Result<Self, ConnectorError> {
        if token.expose_secret().is_empty() {
            return Err(ConnectorError::InvalidConfiguration(
                "Vercel token is empty".into(),
            ));
        }
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("NoPager/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            token,
            team_id: team_id.filter(|value| !value.trim().is_empty()),
            api_base: Url::parse("https://api.vercel.com/").expect("constant URL"),
        })
    }

    pub fn with_api_base(mut self, api_base: Url) -> Self {
        self.api_base = api_base;
        self
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, ConnectorError> {
        let url = self
            .api_base
            .join(path)
            .map_err(|error| ConnectorError::InvalidConfiguration(error.to_string()))?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token.expose_secret());
        if let Some(team_id) = &self.team_id {
            request = request.query(&[("teamId", team_id)]);
        }
        Ok(request)
    }

    pub async fn get_project(&self, id_or_name: &str) -> Result<ProjectDetails, ConnectorError> {
        validate_id(id_or_name)?;
        decode(
            self.request(Method::GET, &format!("v9/projects/{id_or_name}"))?
                .send()
                .await?,
        )
        .await
    }

    pub async fn get_deployment(&self, id_or_url: &str) -> Result<Deployment, ConnectorError> {
        validate_id(id_or_url)?;
        let deployment = decode(
            self.request(Method::GET, &format!("v13/deployments/{id_or_url}"))?
                .send()
                .await?,
        )
        .await?;
        Ok(normalize_current_target(deployment))
    }

    pub async fn list_deployments(
        &self,
        project_id: &str,
        limit: u8,
    ) -> Result<Vec<Deployment>, ConnectorError> {
        self.list_deployments_since(project_id, limit, None).await
    }

    pub async fn list_deployments_since(
        &self,
        project_id: &str,
        limit: u8,
        since_ms: Option<i64>,
    ) -> Result<Vec<Deployment>, ConnectorError> {
        validate_id(project_id)?;
        let limit = limit.min(100).to_string();
        let mut request = self
            .request(Method::GET, "v6/deployments")?
            .query(&[("projectId", project_id), ("limit", limit.as_str())]);
        if let Some(since_ms) = since_ms {
            let since = since_ms.to_string();
            request = request.query(&[("since", since.as_str())]);
        }
        let response = request.send().await?;
        Ok(decode::<DeploymentList>(response).await?.deployments)
    }

    pub async fn list_environment_variables(
        &self,
        project_id_or_name: &str,
    ) -> Result<Vec<ProjectEnvironmentVariable>, ConnectorError> {
        validate_id(project_id_or_name)?;
        let path = format!("v9/projects/{project_id_or_name}/env");
        let limit = VERCEL_PAGE_LIMIT.to_string();
        let mut variables = Vec::new();
        let mut until = None;

        for _ in 0..MAX_ENVIRONMENT_VARIABLE_PAGES {
            let mut request = self
                .request(Method::GET, &path)?
                .query(&[("limit", limit.as_str())]);
            let cursor = until.map(|value: i64| value.to_string());
            if let Some(cursor) = &cursor {
                request = request.query(&[("until", cursor.as_str())]);
            }
            let page = decode::<ProjectEnvironmentVariableList>(request.send().await?).await?;
            variables.extend(page.envs);

            match next_page_cursor(until, page.pagination.next)? {
                Some(next) => until = Some(next),
                None => return Ok(variables),
            }
        }

        Err(ConnectorError::InvalidConfiguration(format!(
            "Vercel environment variable metadata exceeds the bounded safety scan of {} records; reduce Preview variables before AI-authored Preview deployment",
            VERCEL_PAGE_LIMIT as usize * MAX_ENVIRONMENT_VARIABLE_PAGES
        )))
    }

    pub async fn create_preview(
        &self,
        project_name: &str,
        source: &GitSource,
    ) -> Result<Deployment, ConnectorError> {
        validate_id(project_name)?;
        if source.kind != "github" {
            return Err(ConnectorError::InvalidConfiguration(
                "only GitHub gitSource is supported".into(),
            ));
        }
        let environment_variables = self.list_environment_variables(project_name).await?;
        let risky = effective_sensitive_preview_keys(&environment_variables, &source.ref_name);
        if !risky.is_empty() && !preview_secrets_explicitly_allowed() {
            return Err(ConnectorError::InvalidConfiguration(format!(
                "Vercel Preview exposes sensitive-looking environment variables to AI-authored code: {}. Configure Preview with non-production, low-privilege credentials and then set {PREVIEW_SECRETS_OVERRIDE}=true to acknowledge the reviewed Preview boundary",
                risky.join(", ")
            )));
        }
        let payload = preview_payload(project_name, source);
        decode(
            self.request(Method::POST, "v13/deployments")?
                .json(&payload)
                .send()
                .await?,
        )
        .await
    }

    pub async fn promote(
        &self,
        project_id: &str,
        deployment_id: &str,
    ) -> Result<(), ConnectorError> {
        validate_id(project_id)?;
        validate_id(deployment_id)?;
        let project = self.get_project(project_id).await?;
        let response: Value = decode(
            self.request(Method::POST, "v13/deployments")?
                .json(&promotion_payload(&project.name, deployment_id))
                .send()
                .await?,
        )
        .await?;
        if response
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(ConnectorError::Api {
                status: reqwest::StatusCode::BAD_GATEWAY,
                message: "Vercel production rebuild response omitted deployment id".into(),
            });
        }
        Ok(())
    }

    pub async fn rollback(
        &self,
        project_id: &str,
        deployment_id: &str,
    ) -> Result<(), ConnectorError> {
        self.production_action("v1", "rollback", project_id, deployment_id)
            .await
    }

    async fn production_action(
        &self,
        version: &str,
        action: &str,
        project_id: &str,
        deployment_id: &str,
    ) -> Result<(), ConnectorError> {
        validate_id(project_id)?;
        validate_id(deployment_id)?;
        expect_success(
            self.request(
                Method::POST,
                &format!("{version}/projects/{project_id}/{action}/{deployment_id}"),
            )?
            .send()
            .await?,
        )
        .await
    }
}

fn next_page_cursor(
    current: Option<i64>,
    next: Option<i64>,
) -> Result<Option<i64>, ConnectorError> {
    if next.is_some() && next == current {
        return Err(ConnectorError::InvalidConfiguration(
            "Vercel pagination cursor did not advance while scanning environment variables".into(),
        ));
    }
    Ok(next)
}

fn preview_payload(project_name: &str, source: &GitSource) -> Value {
    json!({
        "name": project_name,
        "target": "preview",
        "gitSource": { "type": source.kind, "repoId": source.repo_id, "ref": source.ref_name, "sha": source.sha }
    })
}

fn promotion_payload(project_name: &str, deployment_id: &str) -> Value {
    json!({
        "deploymentId": deployment_id,
        "name": project_name,
        "target": "production",
        "meta": { "action": "promote" }
    })
}

fn effective_sensitive_preview_keys(
    variables: &[ProjectEnvironmentVariable],
    branch: &str,
) -> Vec<String> {
    let mut effective = BTreeMap::<String, &ProjectEnvironmentVariable>::new();
    for variable in variables {
        if !variable
            .target
            .iter()
            .any(|target| target.eq_ignore_ascii_case("preview"))
        {
            continue;
        }
        match variable.git_branch.as_deref() {
            None => {
                effective.entry(variable.key.clone()).or_insert(variable);
            }
            Some(configured_branch) if configured_branch == branch => {
                effective.insert(variable.key.clone(), variable);
            }
            Some(_) => {}
        }
    }
    effective
        .into_values()
        .filter(|variable| preview_variable_is_sensitive(variable))
        .map(|variable| variable.key.clone())
        .collect()
}

fn preview_variable_is_sensitive(variable: &ProjectEnvironmentVariable) -> bool {
    if variable
        .variable_type
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("sensitive"))
    {
        return true;
    }
    let key = variable
        .key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_uppercase())
        .collect::<String>();
    let public_by_design = variable.key.starts_with("NEXT_PUBLIC_")
        || variable.key.starts_with("VITE_")
        || variable.key.starts_with("PUBLIC_");
    !public_by_design
        && [
            "PASSWORD",
            "SECRET",
            "TOKEN",
            "APIKEY",
            "PRIVATEKEY",
            "DATABASEURL",
            "DBURL",
            "REDISURL",
            "CONNECTIONSTRING",
            "CREDENTIAL",
            "AUTHKEY",
            "SIGNINGKEY",
            "ENCRYPTIONKEY",
            "COOKIESECRET",
            "DSN",
        ]
        .iter()
        .any(|marker| key.contains(marker))
}

fn preview_secrets_explicitly_allowed() -> bool {
    std::env::var(PREVIEW_SECRETS_OVERRIDE)
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

fn normalize_current_target(mut deployment: Deployment) -> Deployment {
    // `target=production` identifies the deployment environment. It does not mean
    // the deployment is still the one serving production traffic. Vercel's v13
    // deployment response exposes `live` for that distinction. Keeping a stale
    // production target here would make rollback incorrectly skip a known-good
    // deployment that is no longer live.
    if deployment.target.as_deref() == Some("production") && deployment.live == Some(false) {
        deployment.target = None;
    }
    deployment
}

fn validate_id(value: &str) -> Result<(), ConnectorError> {
    if value.is_empty() || value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(ConnectorError::InvalidConfiguration(
            "unsafe Vercel identifier".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment(target: Option<&str>, live: Option<bool>) -> Deployment {
        Deployment {
            id: "dpl_123".into(),
            url: "example.vercel.app".into(),
            ready_state: Some("READY".into()),
            state: None,
            created: None,
            target: target.map(ToOwned::to_owned),
            live,
            meta: serde_json::Value::Null,
        }
    }

    fn env(key: &str, target: &[&str], git_branch: Option<&str>) -> ProjectEnvironmentVariable {
        ProjectEnvironmentVariable {
            key: key.into(),
            target: target.iter().map(ToString::to_string).collect(),
            variable_type: Some("encrypted".into()),
            git_branch: git_branch.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn rejects_path_in_identifier() {
        assert!(validate_id("prj_123").is_ok());
        assert!(validate_id("my-project").is_ok());
        assert!(validate_id("../../projects").is_err());
    }

    #[test]
    fn empty_team_scope_is_treated_as_personal_account() {
        let client = VercelClient::new(SecretString::from("token".to_owned()), Some("   ".into()))
            .expect("valid client");
        assert!(client.team_id.is_none());
    }

    #[test]
    fn project_git_link_requires_github_and_explicit_production_branch() {
        for kind in ["github", "github-limited"] {
            let project: ProjectDetails = serde_json::from_value(json!({
                "id": "prj_1",
                "name": "demo",
                "link": {
                    "type": kind,
                    "org": "example",
                    "repo": "app",
                    "repoId": 42,
                    "productionBranch": "release"
                }
            }))
            .unwrap();
            assert!(project.github_link().is_some());
            assert_eq!(project.git_production_branch(), Some("release"));
        }

        let gitlab: ProjectDetails = serde_json::from_value(json!({
            "id": "prj_1",
            "name": "demo",
            "link": { "type": "gitlab", "productionBranch": "main" }
        }))
        .unwrap();
        assert!(gitlab.github_link().is_none());
        assert_eq!(gitlab.git_production_branch(), None);

        let missing_branch: ProjectDetails = serde_json::from_value(json!({
            "id": "prj_1",
            "name": "demo",
            "link": { "type": "github", "org": "example", "repo": "app" }
        }))
        .unwrap();
        assert!(missing_branch.github_link().is_some());
        assert_eq!(missing_branch.git_production_branch(), None);

        let no_git_link: ProjectDetails = serde_json::from_value(json!({
            "id": "prj_1",
            "name": "demo"
        }))
        .unwrap();
        assert!(no_git_link.github_link().is_none());
    }

    #[test]
    fn environment_variable_page_reads_next_cursor() {
        let page: ProjectEnvironmentVariableList = serde_json::from_value(json!({
            "envs": [],
            "pagination": { "count": 100, "next": 1_725_000_000_000_i64, "prev": null }
        }))
        .expect("valid Vercel page");
        assert_eq!(page.pagination.next, Some(1_725_000_000_000));
    }

    #[test]
    fn environment_variable_page_without_pagination_is_complete() {
        let page: ProjectEnvironmentVariableList =
            serde_json::from_value(json!({ "envs": [] })).expect("valid Vercel page");
        assert_eq!(page.pagination.next, None);
    }

    #[test]
    fn repeated_environment_variable_cursor_fails_closed() {
        assert!(next_page_cursor(Some(123), Some(123)).is_err());
        assert_eq!(
            next_page_cursor(Some(123), None).expect("complete page"),
            None
        );
        assert_eq!(
            next_page_cursor(Some(123), Some(122)).expect("advanced cursor"),
            Some(122)
        );
    }

    #[test]
    fn preview_payload_is_explicitly_preview() {
        let source = GitSource {
            kind: "github".into(),
            repo_id: 42,
            ref_name: "nopager/incident-abcd-repair".into(),
            sha: "abc123".into(),
        };
        let payload = preview_payload("demo", &source);
        assert_eq!(payload["target"], "preview");
        assert_eq!(payload["gitSource"]["ref"], source.ref_name);
    }

    #[test]
    fn promotion_rebuilds_preview_with_production_environment() {
        let payload = promotion_payload("demo", "dpl_preview");
        assert_eq!(payload["deploymentId"], "dpl_preview");
        assert_eq!(payload["name"], "demo");
        assert_eq!(payload["target"], "production");
        assert_eq!(payload["meta"]["action"], "promote");
    }

    #[test]
    fn sensitive_preview_keys_are_blocked_by_default() {
        let variables = vec![
            env("DATABASE_URL", &["preview"], None),
            env("NEXT_PUBLIC_API_URL", &["preview"], None),
            env("PRODUCTION_TOKEN", &["production"], None),
            env("OTHER_BRANCH_SECRET", &["preview"], Some("feature/other")),
        ];
        assert_eq!(
            effective_sensitive_preview_keys(&variables, "nopager/incident-123-repair"),
            vec!["DATABASE_URL"]
        );
    }

    #[test]
    fn matching_branch_override_replaces_general_preview_value() {
        let variables = vec![
            env("DATABASE_URL", &["preview"], None),
            ProjectEnvironmentVariable {
                key: "DATABASE_URL".into(),
                target: vec!["preview".into()],
                variable_type: Some("plain".into()),
                git_branch: Some("nopager/incident-123-repair".into()),
            },
        ];
        assert_eq!(
            effective_sensitive_preview_keys(&variables, "nopager/incident-123-repair"),
            vec!["DATABASE_URL"]
        );
    }

    #[test]
    fn sensitive_vercel_type_is_blocked_even_with_innocent_key() {
        let variable = ProjectEnvironmentVariable {
            key: "BACKEND_VALUE".into(),
            target: vec!["preview".into()],
            variable_type: Some("sensitive".into()),
            git_branch: None,
        };
        assert!(preview_variable_is_sensitive(&variable));
    }

    #[test]
    fn stale_production_target_is_not_treated_as_current() {
        let normalized = normalize_current_target(deployment(Some("production"), Some(false)));
        assert_eq!(normalized.target, None);
    }

    #[test]
    fn live_production_target_remains_current() {
        let normalized = normalize_current_target(deployment(Some("production"), Some(true)));
        assert_eq!(normalized.target.as_deref(), Some("production"));
    }

    #[test]
    fn missing_live_signal_preserves_provider_target() {
        let normalized = normalize_current_target(deployment(Some("production"), None));
        assert_eq!(normalized.target.as_deref(), Some("production"));
    }
}
