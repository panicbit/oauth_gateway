use std::path::Path;
use std::fs;

use anyhow::{Result, Context};
use serde::Deserialize;


pub mod openid;
pub use openid::Openid;

pub mod server;
pub use server::Server;

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
