//! HTTP(S) text retrieval without application origin, size or duration quotas.
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use url::{Host, Url};

pub const PROFILE: &str = "https.get.v1";
pub const BODY_ESTIMATE_BYTES: usize = 16 * 1024;

pub fn client_revision() -> String {
    let mut bytes = include_bytes!("http_fetch.rs").to_vec();
    bytes.extend_from_slice(include_bytes!("../Cargo.toml"));
    bytes.extend_from_slice(include_bytes!("../Cargo.lock"));
    crate::store::sha256_bytes(&bytes)
}

pub fn parse_url(input: &str) -> Result<Url, &'static str> {
    let url = Url::parse(input).map_err(|_| "invalid_url")?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err("http_url_required");
    }
    Ok(url)
}

/// Validate the stored spelling of a historical origin declaration.
pub fn valid_origin(origin: &str) -> bool {
    parse_url(&format!("{origin}/")).is_ok_and(|url| url.origin().ascii_serialization() == origin)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub succeeded: bool,
    pub failure: Option<String>,
    pub request_may_have_started: bool,
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
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
            final_url: None,
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
    let port = url.port_or_known_default().ok_or("port_absent")?;
    let mut addresses: Vec<_> = match url.host().ok_or("host_absent")? {
        Host::Domain(host) => tokio::net::lookup_host((host, port))
            .await.map_err(|_| "dns_failed")?.collect(),
        Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
        Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
    };
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return Err("dns_no_addresses");
    }
    outcome.resolved_addresses = addresses.clone();
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| attempt.follow()))
        .retry(reqwest::retry::never());
    if let Some(Host::Domain(host)) = url.host() {
        builder = builder.resolve_to_addrs(host, &addresses);
    }
    let client = builder.build().map_err(|_| "client_unavailable")?;
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
    outcome.final_url = Some(response.url().to_string());
    outcome.peer = response.remote_addr();
    if let Some(header) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let value = header.to_str().map_err(|_| "invalid_content_type")?;
        outcome.content_type = Some(value.to_owned());
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|v| v.as_bytes() != b"identity")
    {
        return Err("encoded_body_not_supported");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "body_read_failed")? {
        body.try_reserve(chunk.len()).map_err(|_| "body_allocation_failed")?;
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
/// fabricated failure. No local duration timer interrupts the request.
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
                    match runtime.block_on(retrieve(&url, &mut outcome)) {
                        Ok(()) => outcome.succeeded = true,
                        Err(code) => outcome.failure = Some(code.into()),
                    }
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
