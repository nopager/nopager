pub mod github;
pub mod github_revert;
pub mod source_compatibility;
pub mod vercel;

use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("invalid connector configuration: {0}")]
    InvalidConfiguration(String),
    #[error("remote API request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("remote API returned {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("credential could not be encoded: {0}")]
    Credential(String),
    #[error("repository archive is unsafe or unreadable: {0}")]
    Archive(String),
}

pub(crate) async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, ConnectorError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response.json().await?);
    }
    let message = response
        .text()
        .await
        .unwrap_or_else(|_| "unreadable response".into());
    Err(ConnectorError::Api { status, message })
}

pub(crate) async fn expect_success(response: Response) -> Result<(), ConnectorError> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let message = response
        .text()
        .await
        .unwrap_or_else(|_| "unreadable response".into());
    Err(ConnectorError::Api { status, message })
}
