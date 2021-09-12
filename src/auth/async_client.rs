// This is a modified async_client that reuses the created client.
// Original: https://github.com/ramosbugs/oauth2-rs/blob/main/src/reqwest.rs
// TODO: Open issue for this

use lazy_static::lazy_static;
use oauth2::{HttpRequest, HttpResponse, reqwest::Error};
pub use reqwest;
use reqwest::Client;

///
/// Asynchronous HTTP client.
///
pub async fn async_http_client(
    request: HttpRequest,
) -> Result<HttpResponse, Error<reqwest::Error>> {
    lazy_static! {
        static ref CLIENT: Client = {
            let builder = Client::builder();

            // Following redirects opens the client up to SSRF vulnerabilities.
            // but this is not possible to prevent on wasm targets
            #[cfg(not(target_arch = "wasm32"))]
            let builder = builder.redirect(reqwest::redirect::Policy::none());

            builder.build().unwrap()
        };
    };

    let mut request_builder = CLIENT
        .request(request.method, request.url.as_str())
        .body(request.body);
    for (name, value) in &request.headers {
        request_builder = request_builder.header(name.as_str(), value.as_bytes());
    }
    let request = request_builder.build().map_err(Error::Reqwest)?;

    let response = CLIENT.execute(request).await.map_err(Error::Reqwest)?;

    let status_code = response.status();
    let headers = response.headers().to_owned();
    let chunks = response.bytes().await.map_err(Error::Reqwest)?;
    Ok(HttpResponse {
        status_code,
        headers,
        body: chunks.to_vec(),
    })
}
