use std::net::SocketAddr;
use std::path::Path;
use std::fs;

use anyhow::*;
use serde::Deserialize;

use self::{openid::Openid, server::Server};

mod openid;
mod server;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub openid: Openid,
    #[serde(rename = "server")]
    pub servers: Vec<Server>,
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
