use reqwest::{Client, Method, RequestBuilder};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{ConnectorError, decode, expect_success};

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
pub struct ProjectDetails {
    pub id: String,
    pub name: String,
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
        Ok(Self {
            http: Client::new(),
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
        let payload = serde_json::json!({
            "name": project_name,
            "target": null,
            "gitSource": { "type": source.kind, "repoId": source.repo_id, "ref": source.ref_name, "sha": source.sha }
        });
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
        self.production_action("v10", "promote", project_id, deployment_id)
            .await
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
            target: target.map(ToOwned::to_owned),
            live,
            meta: serde_json::Value::Null,
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
