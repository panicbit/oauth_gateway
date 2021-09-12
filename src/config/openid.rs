use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Openid {
    pub issuer_url: String,
    pub introspect_url: String,
    pub client_id: String,
    pub client_secret: String,
}
