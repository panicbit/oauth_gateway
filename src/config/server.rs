use anyhow::*;
use hyper::Uri;
use regex::RegexSet;
use serde::{Deserialize, Deserializer, de};

#[derive(Debug, Deserialize, Clone)]
pub struct Server {
    pub host: String,
    pub upstream: String,
    #[serde(default)]
    pub upstream_tls: bool,
    #[serde(deserialize_with = "deserialize_patterns")]
    pub public_routes: RegexSet,
}

impl Server {
    pub fn is_public_route(&self, uri: &Uri) -> bool {
        let path = uri.path();

        self.public_routes.is_match(path)
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
