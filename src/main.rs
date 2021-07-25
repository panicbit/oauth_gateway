use std::convert::{Infallible, TryFrom};
use std::net::SocketAddr;
use std::sync::Arc;
use std::mem;

use hyper::header::{AUTHORIZATION, FORWARDED, HOST, HeaderValue};
use hyper::http::uri::Scheme;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, Uri};
use openidconnect::core::CoreClient;
use reqwest::Client;
use anyhow::*;
use self::config::Config;

mod config;
mod auth;

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
        let remote_addr = Arc::new(socket.remote_addr());
        // This is the `Service` that will handle the connection.
        // `service_fn` is a helper to convert a function that
        // returns a Response into a `Service`.
        async move {
            let service = service_fn(move |request| {
                let app = app.clone();
                let remote_addr = remote_addr.clone();

                async move {
                    let response = app.proxy_request(&remote_addr, request).await;

                    Ok::<_, Error>(response)
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

    async fn proxy_request(&self, remote_addr: &SocketAddr, mut request: Request<Body>) -> Response<Body> {
        let user_info = auth::verify_access_token(&self.oidc, &request).await;
        let user_info = match user_info {
            Ok(user_info) => user_info,
            Err(err) => {
                eprintln!("Token verification failed: {:?}", err);

                let response = Response::builder()
                    .status(500)
                    .body(Body::empty())
                    .unwrap();

                return response;
            },
        };

        let upstream_authority = self.config.upstream_authority.parse().expect("failed to parse upstream_host as authority");
        let upstream_scheme = match self.config.upstream_use_https {
                true => Scheme::HTTPS,
                false => Scheme::HTTP,
        };
        let http_version = request.version();

        {
            let mut parts = request.uri().clone().into_parts();
            parts.scheme = Some(upstream_scheme);
            parts.authority = Some(upstream_authority);

            let upstream_uri = Uri::from_parts(parts).expect("failed to build upstream uri");

            *request.uri_mut() = upstream_uri;
        }

        request.headers_mut().remove(HOST);
        request.headers_mut().remove(AUTHORIZATION);

        let mut upstream_request = reqwest::Request::try_from(request)
            .expect("failed to convert request");

        {
            let addr = match remote_addr {
                SocketAddr::V4(v4) => v4.to_string(),
                SocketAddr::V6(v6) => format!("\"{}\"", v6),
            };
            let forwarded = format!("for={}", addr);
            let forwarded = HeaderValue::from_str(&forwarded)
                .expect("Failed to construct forwarded header value");

            upstream_request.headers_mut().insert(FORWARDED, forwarded);
        }

        let is_authenticated_str = if user_info.is_some() { "true" } else { "false" };
        upstream_request.headers_mut().insert("X-User-Authenticated", HeaderValue::from_static(is_authenticated_str));

        if user_info.is_some() {
            upstream_request.headers_mut().insert("X-User-Id", HeaderValue::from_static("12345"));
            upstream_request.headers_mut().insert("X-User-Name", HeaderValue::from_static("tester"));
        }

        let mut upstream_response = self.http.execute(upstream_request).await
            .expect("upstream request failed");
        let mut response = Response::builder()
        // loses status line text
        .status(upstream_response.status())
        .version(http_version);

        mem::swap(upstream_response.headers_mut(), response.headers_mut()
            .expect("failed to get builder headers"));

        let body = Body::wrap_stream(upstream_response.bytes_stream());

        response.body(body).expect("failed to set response body")
    }
}
