use std::net::SocketAddr;
use std::path::Path;
use std::fs;
use anyhow::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub upstream_authority: String,
    pub upstream_use_https: bool,
    pub auth_uri: String,
    pub token_uri: String,
    pub introspect_uri: String,
    pub client_id: String,
    pub client_secret: String,
}

impl Config {
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let config = fs::read_to_string(path)
            .context("failed to read config")?;
        let config = toml::from_str(&config)
            .context("failed to parse config")?;

        Ok(config)
    }
}
