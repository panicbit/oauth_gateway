use std::convert::{Infallible, TryFrom};
use std::net::SocketAddr;
use std::sync::Arc;
use std::mem;

use auth::IntrospectionResult;
use header::{X_USER_ID, X_USER_NAME};
use hyper::header::{AUTHORIZATION, FORWARDED, HOST, HeaderValue};
use hyper::http::uri::Scheme;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode, Uri};
use oauth2::TokenIntrospectionResponse;
use openidconnect::core::CoreClient;
use reqwest::Client;
use anyhow::*;
use self::config::Config;

mod config;
mod auth;
mod header;

#[tokio::main]
pub async fn main() -> Result<()> {
    let config = Config::read("config.toml")
        .context("failed to read config")?;

    let listen_addr = config.listen_addr;

    // pretty_env_logger::init();
    let app = App::new(config).await?;
    let app = Arc::new(app);

    // For every connection, we must make a `Service` to handle all
    // incoming HTTP requests on said connection.
    let make_service = make_service_fn(|socket: &AddrStream| {
        let app = app.clone();
        let client_addr = Arc::new(socket.remote_addr());
        // This is the `Service` that will handle the connection.
        // `service_fn` is a helper to convert a function that
        // returns a Response into a `Service`.
        async move {
            let service = service_fn(move |request| {
                let app = app.clone();
                let client_addr = client_addr.clone();

                async move {
                    let response = app.proxy_request(&client_addr, request).await;

                    if let Err(err) = response {
                        eprintln!("{}", err);

                        let response = Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::empty())
                            .unwrap();

                        return Ok(response)
                    }

                    response
                }
            });

            Ok::<_, Infallible>(service)
        }
    });

    let server = Server::bind(&listen_addr).serve(make_service);

    println!("Listening on http://{}", listen_addr);

    server.await?;

    Ok(())
}

struct App {
    config: Config,
    oidc: CoreClient,
    http: Client,
}

impl App {
    async fn new(config: Config) -> Result<Self> {
        let oidc = auth::create_oidc_client(&config).await
            .context("failed to create oidc client")?;

        Ok(Self {
            config,
            oidc,
            http: Client::new(),
        })
    }

    async fn proxy_request(&self, client_addr: &SocketAddr, mut request: Request<Body>) -> Result<Response<Body>> {
        let is_public_route = self.is_public_route(request.uri());

        let token_info = if is_public_route {
            None
        } else {
            let token_info = auth::verify_access_token(&self.oidc, &request).await
                .context("Token verification failed")?;

            match token_info {
                Some(token_info) => Some(token_info),
                None => {
                    eprintln!("Unauthenticated");

                    let response = Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::empty())
                        .unwrap();

                    return Ok(response)
                }
            }
        };

        if let Some(token_info) = &token_info {
            eprintln!("{:#?}", token_info);
        }

        let upstream_authority = self.config.upstream_authority.parse()
            .context("failed to parse upstream_host as authority")?;
        let upstream_scheme = match self.config.upstream_use_https {
                true => Scheme::HTTPS,
                false => Scheme::HTTP,
        };
        let http_version = request.version();

        {
            let mut parts = request.uri().clone().into_parts();
            parts.scheme = Some(upstream_scheme);
            parts.authority = Some(upstream_authority);

            let upstream_uri = Uri::from_parts(parts)
                .context("failed to build upstream uri")?;

            *request.uri_mut() = upstream_uri;
        }

        remove_dangerous_headers(&mut request);

        let mut upstream_request = create_upstream_request(request, client_addr);

        // let is_authenticated_str = if user_info.is_some() { "true" } else { "false" };
        // upstream_request.headers_mut().insert("X-User-Authenticated", HeaderValue::from_static(is_authenticated_str));

        if let Some(token_info) = token_info {
            enrich_request_with_token_info(&mut upstream_request, &token_info)?;
        }

        let mut upstream_response = self.http.execute(upstream_request).await
            .context("upstream request failed")?;
        let mut response = Response::builder()
            // loses status line text
            .status(upstream_response.status())
            .version(http_version);

        mem::swap(upstream_response.headers_mut(), response.headers_mut().context("failed to get builder headers")?);

        let body = Body::wrap_stream(upstream_response.bytes_stream());
        let response = response.body(body).context("failed to set response body")?;

        Ok(response)
    }

    fn is_public_route(&self, uri: &Uri) -> bool {
        let path = uri.path();

        self.config.public_route_patterns.is_match(path)
    }
}

fn create_upstream_request(request: Request<Body>, client_addr: &SocketAddr) -> reqwest::Request {
    let mut upstream_request = reqwest::Request::try_from(request)
        .expect("failed to convert request");
    {
        let addr = match client_addr {
            SocketAddr::V4(v4) => v4.to_string(),
            SocketAddr::V6(v6) => format!("\"{}\"", v6),
        };
        let forwarded = format!("for={}", addr);
        let forwarded = HeaderValue::from_str(&forwarded)
            .expect("Failed to construct forwarded header value");

        upstream_request.headers_mut().insert(FORWARDED, forwarded);
    }
    upstream_request
}

fn remove_dangerous_headers(request: &mut Request<Body>) {
    let headers = request.headers_mut();

    headers.remove(HOST);
    headers.remove(AUTHORIZATION);
    headers.remove(X_USER_ID);
    headers.remove(X_USER_NAME);
}

fn enrich_request_with_token_info(request: &mut reqwest::Request, token_info: &IntrospectionResult) -> Result<()> {
    let headers = request.headers_mut();

    if let Some(user_id) = token_info.sub() {
        headers.insert(X_USER_ID, user_id.parse()?);
    }

    if let Some(username) = token_info.username() {
        request.headers_mut().insert(X_USER_NAME, username.parse()?);
    }

    Ok(())
}
