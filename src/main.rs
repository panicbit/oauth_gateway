use std::convert::{Infallible, TryFrom};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::{fs, mem};

use hyper::header::{FORWARDED, HOST, HeaderValue};
use hyper::http::uri::Scheme;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, Uri};
use reqwest::Client;
use anyhow::*;
use self::config::Config;

mod config;

#[tokio::main]
pub async fn main() -> Result<()> {
    // pretty_env_logger::init();
    let app = App::new()?;
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

    let addr = ([127, 0, 0, 1], 8080).into();

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    server.await?;

    Ok(())
}

struct App {
    config: Config,
    http: Client,
}

impl App {
    fn new() -> Result<Self> {
        let config = Config::read("config.toml")
            .context("failed to read app config")?;

        Ok(Self {
            config,
            http: Client::new(),
        })
    }

    async fn proxy_request(&self, remote_addr: &SocketAddr, mut request: Request<Body>) -> Response<Body> {
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

        let mut upstream_request = reqwest::Request::try_from(request).expect("failed to convert request");

        {
            let addr = match remote_addr {
                SocketAddr::V4(v4) => v4.to_string(),
                SocketAddr::V6(v6) => format!("\"{}\"", v6),
            };
            let forwarded = format!("for={}", addr);
            let forwarded = HeaderValue::from_str(&forwarded).expect("Failed to construct forwarded header value");

            upstream_request.headers_mut().insert(FORWARDED, forwarded);
        }

        {
            upstream_request.headers_mut().insert("X-User-Id", HeaderValue::from_static("12345"));
            upstream_request.headers_mut().insert("X-User-Name", HeaderValue::from_static("tester"));
        }

        let mut upstream_response = self.http.execute(upstream_request).await.expect("upstream request failed");
        let mut response = Response::builder()
        // loses status line text
        .status(upstream_response.status())
        .version(http_version);
        
        mem::swap(upstream_response.headers_mut(), response.headers_mut().expect("failed to get builder headers"));

        let body = Body::wrap_stream(upstream_response.bytes_stream());

        response.body(body).expect("failed to set response body")
    }
}
