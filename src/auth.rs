use std::str;

use anyhow::*;
use hyper::{Body, Request, header::AUTHORIZATION};
use openidconnect::{AccessToken, ClientId, ClientSecret, IntrospectionUrl, IssuerUrl, TokenIntrospectionResponse, core::{CoreClient, CoreProviderMetadata}, reqwest::async_http_client};

use crate::Config;

pub async fn create_oidc_client(config: &Config) -> Result<CoreClient> {
    let provider_metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new("https://horizonpedia.com:9001/auth/realms/Horizonpedia".to_string())?,
            async_http_client, // FIXME: async_http_client does not reuse http client
        )
        .await
        .context("Failed to discover oauth endpoints")?;

    let client_id = ClientId::new(config.client_id.clone());
    let introspection_url = IntrospectionUrl::new(config.introspect_uri.clone())
        .context("Failed to create introspection URL")?;
    let client_secret = ClientSecret::new(config.client_secret.clone());

    let oidc_client = CoreClient::from_provider_metadata(provider_metadata, client_id, Some(client_secret))
        .set_introspection_uri(introspection_url);

    Ok(oidc_client)
}

fn extract_access_token(request: &Request<Body>) -> Option<AccessToken> {
    let auth = request.headers().get(AUTHORIZATION)?;
    let auth = str::from_utf8(auth.as_bytes()).ok()?;
    let mut auth = auth.split_whitespace();

    let kind = auth.next()?;
    let token = auth.next()?;

    if !kind.eq_ignore_ascii_case("token") && !kind.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let token = AccessToken::new(token.to_string());

    Some(token)
}

pub async fn verify_access_token(oidc: &CoreClient, request: &Request<Body>) -> Result<Option<()>> {
    let access_token = match extract_access_token(request) {
        Some(access_token) => access_token,
        None => return Ok(None),
    };

    let introspection = oidc.introspect(&access_token)
        .context("Failed to create introspection request")?
        .request_async(async_http_client) // FIXME: async_http_client does not reuse http client
        .await
        .context("Token introspection failed")?;

    if introspection.active() {
        return Ok(Some(()));
    }

    Ok(None)
}
