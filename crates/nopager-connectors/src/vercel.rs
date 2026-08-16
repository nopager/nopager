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
    pub target: Option<String>,
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct DeploymentList {
    pub deployments: Vec<Deployment>,
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
            team_id,
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

    pub async fn get_deployment(&self, id_or_url: &str) -> Result<Deployment, ConnectorError> {
        validate_id(id_or_url)?;
        decode(
            self.request(Method::GET, &format!("v13/deployments/{id_or_url}"))?
                .send()
                .await?,
        )
        .await
    }

    pub async fn list_deployments(
        &self,
        project_id: &str,
        limit: u8,
    ) -> Result<Vec<Deployment>, ConnectorError> {
        validate_id(project_id)?;
        let response = self
            .request(Method::GET, "v6/deployments")?
            .query(&[
                ("projectId", project_id),
                ("limit", &limit.min(100).to_string()),
            ])
            .send()
            .await?;
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
    #[test]
    fn rejects_path_in_identifier() {
        assert!(validate_id("prj_123").is_ok());
        assert!(validate_id("../../projects").is_err());
    }
}
