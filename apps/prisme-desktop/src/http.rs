//! What Prisme fetches from the internet: its separation models, its controller profiles.

use std::time::Duration;

/// A client giving up on a server silent for `timeout`.
pub fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client =
        reqwest::Client::builder().user_agent("Prisme").connect_timeout(timeout).read_timeout(timeout);
    // Android's own certificates are reached through Java only: Mozilla's are shipped instead
    #[cfg(target_os = "android")]
    let client = client.tls_certs_only(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .filter_map(|der| reqwest::Certificate::from_der(der).ok()),
    );
    client.build().map_err(|e| e.to_string())
}

/// The body of `url`, if the server gives it.
pub async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let response = response.error_for_status().map_err(|e| e.to_string())?;
    Ok(response.bytes().await.map_err(|e| e.to_string())?.to_vec())
}
