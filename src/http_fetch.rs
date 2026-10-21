//! Direct, unauthenticated HTTPS text retrieval. No model or shell participates.
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::{Host, Url};

pub const PROFILE: &str = "https.get.v1";
pub const URL_BYTES: usize = 2048;
pub const BODY_BYTES: usize = 16 * 1024;
pub const TIMEOUT_SECONDS: u64 = 20;

pub fn client_revision() -> String {
    let mut bytes = include_bytes!("http_fetch.rs").to_vec();
    bytes.extend_from_slice(include_bytes!("../Cargo.toml"));
    bytes.extend_from_slice(include_bytes!("../Cargo.lock"));
    crate::store::sha256_bytes(&bytes)
}

pub fn parse_url(input: &str) -> Result<Url, &'static str> {
    if input.len() > URL_BYTES || input.bytes().any(|c| c.is_ascii_control()) {
        return Err("invalid_url");
    }
    let url = Url::parse(input).map_err(|_| "invalid_url")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(url.host(), Some(Host::Domain(_)))
        || url.as_str() != input
    {
        return Err("canonical_https_url_required");
    }
    let host = url.host_str().ok_or("host_absent")?;
    if host.len() > 253
        || !host.contains('.')
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("public_dns_hostname_required");
    }
    Ok(url)
}

/// Canonical origins contain no path, query, credentials, explicit port or slash.
pub fn valid_origin(origin: &str) -> bool {
    parse_url(&format!("{origin}/")).is_ok_and(|url| url.origin().ascii_serialization() == origin)
}

// Conservatively exclude every IANA special-purpose IPv4 allocation, multicast
// and reserved space. IPv6 permits 2000::/3 except its special-purpose blocks.
// This deliberately rejects some globally reachable special-use exceptions.
// Registries: https://www.iana.org/assignments/iana-ipv4-special-registry/
//             https://www.iana.org/assignments/iana-ipv6-special-registry/
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            ![
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc01fc400, 24),
                (0xc034c100, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc0af3000, 24),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .iter()
            .any(|(base, bits)| n >> (32 - bits) == base >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && s[1] < 0x0200)
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && s[0] != 0x2002
                && !(s[0] == 0x2620 && s[1] == 0x004f && s[2] == 0x8000)
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub succeeded: bool,
    pub failure: Option<String>,
    pub request_may_have_started: bool,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub resolved_addresses: Vec<SocketAddr>,
    pub peer: Option<SocketAddr>,
    pub started_at_unix: Option<u64>,
    pub finished_at_unix: Option<u64>,
    pub body: Option<String>,
    pub body_sha256: Option<String>,
}

fn clock() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|t| t.as_secs())
}

impl Outcome {
    fn initial() -> Self {
        Self {
            succeeded: false,
            failure: None,
            request_may_have_started: false,
            status: None,
            content_type: None,
            resolved_addresses: Vec::new(),
            peer: None,
            started_at_unix: clock(),
            finished_at_unix: None,
            body: None,
            body_sha256: None,
        }
    }
}

async fn retrieve(url: &str, outcome: &mut Outcome) -> Result<(), &'static str> {
    let url = parse_url(url)?;
    let host = url.host_str().ok_or("host_absent")?;
    let mut addresses: Vec<_> = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| "dns_failed")?
        .take(33)
        .collect();
    if addresses.len() > 32 {
        return Err("dns_address_limit");
    }
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses.iter().any(|a| !public_address(a.ip()))
    {
        return Err("dns_address_not_permitted");
    }
    outcome.resolved_addresses = addresses.clone();
    // Pin only this checked resolution; the TLS name remains the original host.
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .http1_only()
        .pool_max_idle_per_host(0)
        .connection_verbose(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .timeout(Duration::from_secs(TIMEOUT_SECONDS))
        .connect_timeout(Duration::from_secs(8))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| "client_unavailable")?;
    outcome.request_may_have_started = true;
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/*, application/json, application/xml",
        )
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::USER_AGENT, "ai-personas/https.get.v1")
        .send()
        .await
        .map_err(|_| "request_failed")?;
    outcome.status = Some(response.status().as_u16());
    outcome.peer = response.remote_addr();
    if outcome.peer.is_none_or(|peer| !addresses.contains(&peer)) {
        return Err("peer_address_mismatch");
    }
    if let Some(header) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let value = header.to_str().map_err(|_| "invalid_content_type")?;
        if value.len() > 256 {
            return Err("content_type_too_large");
        }
        outcome.content_type = Some(value.to_owned());
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|v| v.as_bytes() != b"identity")
    {
        return Err("encoded_body_not_supported");
    }
    if response
        .content_length()
        .is_some_and(|n| n > BODY_BYTES as u64)
    {
        return Err("body_too_large");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "body_read_failed")? {
        if chunk.len() > BODY_BYTES - body.len() {
            return Err("body_too_large");
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body).map_err(|_| "body_not_utf8")?;
    outcome.body_sha256 = Some(crate::store::sha256_bytes(body.as_bytes()));
    outcome.body = Some(body);
    if !response.status().is_success() {
        return Err("http_status_not_success");
    }
    Ok(())
}

/// Called only after durable dispatch. A separate runtime permits synchronous
/// Store adoption inside an existing Tokio session. Panic means unknown, not a
/// fabricated failure. Cancellation bounds the async operation, not libc DNS.
pub fn execute(url: &str) -> Option<Outcome> {
    let url = url.to_owned();
    let worker = std::thread::Builder::new()
        .name("personas-https".into())
        .spawn(move || {
            let mut outcome = Outcome::initial();
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    let result = runtime.block_on(async {
                        tokio::time::timeout(
                            Duration::from_secs(TIMEOUT_SECONDS),
                            retrieve(&url, &mut outcome),
                        )
                        .await
                    });
                    match result {
                        Ok(Ok(())) => outcome.succeeded = true,
                        Ok(Err(code)) => outcome.failure = Some(code.into()),
                        Err(_) => outcome.failure = Some("deadline_elapsed".into()),
                    }
                    // A libc resolver may outlive cancellation. Do not wait for its
                    // blocking worker; it cannot send an HTTP request after timeout.
                    runtime.shutdown_background();
                }
                Err(_) => outcome.failure = Some("executor_unavailable".into()),
            }
            outcome.finished_at_unix = clock();
            outcome
        });
    match worker {
        Ok(worker) => worker.join().ok(),
        Err(_) => {
            let mut outcome = Outcome::initial();
            outcome.failure = Some("executor_unavailable".into());
            outcome.finished_at_unix = clock();
            Some(outcome)
        }
    }
}
