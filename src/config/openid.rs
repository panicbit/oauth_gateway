use std::env;

use anyhow::Context;
use serde::{Deserialize, Deserializer, de};

#[derive(Debug, Deserialize, Clone)]
pub struct Openid {
    pub issuer_url: String,
    pub introspect_url: String,
    #[serde(deserialize_with = "env_loadable")]
    pub client_id: String,
    #[serde(deserialize_with = "env_loadable")]
    pub client_secret: String,
}

fn env_loadable<'de, D: Deserializer<'de>>(de: D) -> Result<String, D::Error> {
    let value = String::deserialize(de)?;

    let env_key = match extract_env_key(&value) {
        Some(env_key) => env_key,
        None => return Ok(value),
    };

    let value = env::var(env_key)
        .with_context(|| format!("failed to load env var {env_key:?}"))
        .map_err(de::Error::custom)?;

    Ok(value)
}

fn extract_env_key(value: &str) -> Option<&str> {
    value.strip_prefix("ENV[")?.strip_suffix(']')
}
