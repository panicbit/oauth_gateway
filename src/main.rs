use std::convert::TryFrom;
use std::net::SocketAddr;
use std::sync::Arc;
use std::mem;

use anyhow::{Result, Context, Error, anyhow};
use auth::IntrospectionResult;
use futures::TryFutureExt;
use futures::future::{self, BoxFuture, FutureExt, Ready};
use header::{X_USER_ID, X_USER_NAME, X_USER_ROLE};
use hyper::{Body, Request, Response, StatusCode, Uri};
use hyper::header::{AUTHORIZATION, FORWARDED, HOST, HeaderValue};
use hyper::http::uri::Scheme;
use hyper::server::conn::Http;
use oauth2::TokenIntrospectionResponse;
use reqwest::Client;
use rustls::sign::{CertifiedKey, RsaSigningKey};
use rustls::{Certificate, PrivateKey};
use tls_manager::TlsManager;
use tokio::time::{self, Duration};
use unicase::Ascii;

use self::auth::extensions::Token;
use self::listener_manager::ListenerManager;
use self::hyperion::Service;
use self::config::Config;
use self::listener::Accepted;

mod config;
mod auth;
mod header;
mod hyperion;
mod listener;
mod listener_manager;
mod tls_manager;

#[tokio::main]
pub async fn main() -> Result<()> {
    let config = Config::read("config.toml")
        .context("failed to read config")?;

    let mut app = App::new(config).await?;
    let config = &app.config;

    for server_config in &config.servers {
        if let Some(tls_config) = &server_config.tls {
            let certified_key = load_certified_key(tls_config)
                .context("Failed to load tls certificate / key")?;

            app.tls_manager.add_certified_key(
                server_config.listen,
                server_config.name.clone(),
                certified_key,
            )?;
        }
    }

    for server_config in &config.servers {
        app.listener_manager.start_listening_on(server_config.listen).await
            .with_context(|| format!("Failed to listen on {}", server_config.listen))?;
        println!("Listening on {}", server_config.listen);
    }

    let app = Arc::new(app);

    loop {
        let accepted = match app.listener_manager.accept().await.context("Accept failed") {
            Ok(accepted) => accepted,
            Err(err) => {
                eprintln!("{:#}", err);
                time::sleep(Duration::from_secs(1)).await;
                continue;
            },
        };

        tokio::spawn(
            handle_client(
                app.clone(),
                accepted,
            )
            .map_err(|err| {
                eprintln!("{:#}", err);
            })
        );
    }
}

fn load_certified_key(tls_config: &config::server::Tls) -> Result<CertifiedKey> {
    let cert = std::fs::File::open(&tls_config.cert)
        .with_context(|| format!("Failed to open {:?}", tls_config.cert))?;
    let mut cert = std::io::BufReader::new(cert);
    let cert = rustls_pemfile::certs(&mut cert)
        .with_context(|| format!("Failed to read cert from {:?}", tls_config.cert))?
        .into_iter()
        .map(Certificate)
        .collect::<Vec<_>>();

    let key = std::fs::File::open(&tls_config.key)
        .with_context(|| format!("Failed to open {:?}", tls_config.key))?;
    let mut key = std::io::BufReader::new(key);
    let key = rustls_pemfile::pkcs8_private_keys(&mut key)
        .with_context(|| format!("Failed to read key from {:?}", tls_config.key))?
        .pop()
        .with_context(|| format!("No keys found in {:?}", tls_config.key))?;
    let key = PrivateKey(key);
    let key = RsaSigningKey::new(&key)
        .map_err(|_| anyhow!("Invalid key"))?;
    let certified_key = CertifiedKey::new(cert, Arc::new(key));

    Ok(certified_key)
}

async fn handle_client(
    app: Arc<App>,
    accepted: Accepted,
) -> Result<()> {
    let mut handler = RequestHandler {
        app: app.clone(),
        client_addr: accepted.remote_addr,
        listen_addr: accepted.listen_addr,
        sni_hostname: None,
    };

    match app.tls_manager.acceptor(&accepted.listen_addr) {
        Some(tls_acceptor) => {
            let tls_stream = tls_acceptor.accept(accepted.stream).await
                .context("Tls accept failed")?;

            handler.sni_hostname = tls_stream.get_ref().1.sni_hostname()
                .map(String::from)
                .map(Arc::new);

            Http::new().serve_connection(tls_stream, handler.compat()).await?;
        },
        None => {
            Http::new().serve_connection(accepted.stream, handler.compat()).await?;
        }
    }

    Ok(())
}

#[derive(Clone)]
struct RequestHandler {
    app: Arc<App>,
    client_addr: SocketAddr,
    listen_addr: SocketAddr,
    sni_hostname: Option<Arc<String>>,
}

impl<'a> Service<Request<Body>> for RequestHandler {
    type Response = Response<Body>;
    type Error = Error;
    type ReadyFuture = Ready<Result<(), Self::Error>>;
    type CallFuture = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn ready(&mut self) -> Self::ReadyFuture {
        future::ok(())
    }

    fn call(&mut self, request: Request<Body>) -> Self::CallFuture {
        let this = self.clone();

        async move {
            let response = this.proxy_request(request).await;

            if let Err(err) = response {
                eprintln!("{:#}", err);

                let response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap();

                return Ok(response)
            }

            response
        }
        .boxed()
    }
}

impl RequestHandler {
    async fn proxy_request(&self, mut request: Request<Body>) -> Result<Response<Body>> {
        let host_name = match self.extract_host_name(&request) {
            Ok(host_name) => host_name,
            Err(err) => {
                eprintln!("Failed to extract host header: {}", err);

                let response = Response::builder()
                    .status(400)
                    .body(Body::empty())
                    .unwrap();

                return Ok(response)
            },
        };

        let server = self.app.config.servers.iter()
            .find(|server|
                server.listen == self.listen_addr &&
                Ascii::new(&server.name) == host_name
            );
        let server = match server {
            Some(server) => server,
            None => {
                eprintln!("server for host '{}' not defined", host_name);

                let response = Response::builder()
                    .status(400)
                    .body(Body::empty())
                    .unwrap();

                return Ok(response)
            },
        };

        println!("selected server '{}'", server.name);

        let is_public_route = server.is_public_route(request.uri());

        let token_info = if is_public_route {
            None
        } else {
            let token_info = auth::verify_access_token(&self.app.oidc, &request).await
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

        let upstream_authority = server.upstream.parse()
            .context("failed to parse upstream_host as authority")?;
        let upstream_scheme = match server.upstream_tls {
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

        let mut upstream_request = create_upstream_request(request, &self.client_addr);

        // let is_authenticated_str = if user_info.is_some() { "true" } else { "false" };
        // upstream_request.headers_mut().insert("X-User-Authenticated", HeaderValue::from_static(is_authenticated_str));

        if let Some(token_info) = token_info {
            enrich_request_with_token_info(&mut upstream_request, &token_info)?;
        }

        let mut upstream_response = self.app.http.execute(upstream_request).await
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

    fn extract_host_name<'a>(&'a self, request: &'a Request<Body>) -> Result<Ascii<&'a str>> {
        // TODO: maybe ensure that sni hostname matches request hostname

        if let Some(sni_hostname) = &self.sni_hostname {
            return Ok(Ascii::new(sni_hostname));
        }

        let host = request.headers().get(HOST)
            .context("Host header does is not set")?;
        let host = host.to_str()
            .context("Host header is invalid UTF-8")?;
        let host = host.split_once(":")
            .map(|(host, _port)| host)
            .unwrap_or(host);

        Ok(Ascii::new(host))
    }
}

struct App {
    listener_manager: ListenerManager,
    tls_manager: TlsManager,
    oidc: auth::Client,
    http: Client,
    config: Config,
}

impl App {
    async fn new(config: Config) -> Result<Self> {
        let oidc = auth::create_oidc_client(&config).await
            .context("failed to create oidc client")?;

        Ok(Self {
            listener_manager: ListenerManager::new(),
            tls_manager: TlsManager::new(),
            oidc,
            http: Client::new(),
            config,
        })
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
        headers.insert(X_USER_NAME, username.parse()?);
    }

    match &token_info.extra_fields().0 {
        Token::Keybase(token) => {
            for role in &token.realm_access.roles {
                let role = match role.parse::<HeaderValue>() {
                    Ok(role) => role,
                    Err(_) => {
                        eprintln!("Role is not a valid header value: {}", role);
                        continue
                    },
                };
                headers.append(X_USER_ROLE, role);
            }
        },
    }

    Ok(())
}
