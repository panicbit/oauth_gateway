use std::net::SocketAddr;
use std::path::Path;
use std::fs;
use anyhow::*;
use regex::RegexSet;
use serde::{Deserialize, Deserializer, de};

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

fn deserialize_patterns<'de, D>(de: D) -> Result<RegexSet, D::Error>
where
    D: Deserializer<'de>,
{
    let mut patterns = Option::<Vec<String>>::deserialize(de)?
        .unwrap_or_default();

    for pattern in &mut patterns {
        *pattern = format!("^{}$", pattern);
    }

    let patterns = RegexSet::new(&patterns)
        .map_err(de::Error::custom)?;

    Ok(patterns)
}
