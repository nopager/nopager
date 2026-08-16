use std::{env, net::SocketAddr};

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub address: SocketAddr,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let raw = env::var("NOPAGER_SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
        let address = raw
            .parse()
            .map_err(|_| ConfigError::InvalidServerAddress(raw))?;
        Ok(Self { address })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("NOPAGER_SERVER_ADDR is invalid: {0}")]
    InvalidServerAddress(String),
}
