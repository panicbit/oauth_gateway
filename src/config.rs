use std::net::SocketAddr;
use std::path::Path;
use std::fs;
use anyhow::*;
use regex::RegexSet;
use serde::{Deserialize, Deserializer, de};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub upstream_authority: String,
    pub upstream_use_https: bool,
    pub issuer_url: String,
    pub introspect_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(deserialize_with = "deserialize_patterns")]
    pub public_route_patterns: RegexSet,
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
