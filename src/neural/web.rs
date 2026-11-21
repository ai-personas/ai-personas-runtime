//! Persona-chosen retrieval of an exact source. HTTP status, the server's
//! declaration and a recognizable file header are independent observations.
use super::{failure, now, Result};
use reqwest::{header, redirect, Client};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fetch {
    /// HTTP(S) URL to retrieve. No model-provider credentials are attached.
    pub url: String,
    /// Optionally compare the returned bytes' recognizable header with this format.
    /// A header match does not verify a document's contents.
    #[serde(default)]
    pub expect: Option<Signature>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Signature {
    Pdf,
    Html,
    Zip,
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl Signature {
    fn identify(bytes: &[u8]) -> Option<Self> {
        // Read bytes, never the URL/filename or a server's Content-Type claim.
        let text = bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(bytes)
            .trim_ascii_start();
        if [b"<!doctype html".as_slice(), b"<html"].iter().any(|tag| {
            text.get(..tag.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(tag))
                && text
                    .get(tag.len())
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>')
        }) {
            return Some(Self::Html);
        }
        if bytes.starts_with(b"%PDF-") {
            return Some(Self::Pdf);
        }
        if [b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"]
            .iter()
            .any(|tag| bytes.starts_with(*tag))
        {
            return Some(Self::Zip);
        }
        match super::context::image_media(bytes)? {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/gif" => Some(Self::Gif),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }

    fn media(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Html => "text/html",
            Self::Zip => "application/zip",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

impl Fetch {
    pub fn validate(&self) -> Result<url::Url> {
        crate::http_fetch::parse_url(&self.url).map_err(failure)
    }
}

pub(super) struct Body {
    pub media: &'static str,
    pub bytes: Vec<u8>,
}

pub(super) async fn fetch(
    input: Fetch,
    mut stopped: impl FnMut() -> bool,
) -> Result<(Value, Option<Body>)> {
    let url = input.validate()?;
    let client = Client::builder()
        .retry(reqwest::retry::never())
        .redirect(redirect::Policy::custom(|r| {
            if r.previous().contains(r.url()) {
                r.stop()
            } else {
                r.follow()
            }
        }))
        .build()?;
    let mut receipt = json!({"requested_url":input.url, "expected_signature":input.expect,
        "started":now(), "success":false, "body_complete":false, "received_bytes":0,
        "request_attempted":false, "automatic_retry":false});
    let operation = async {
        receipt["request_attempted"] = json!(true);
        let mut response = client
            .get(url)
            .header(header::ACCEPT_ENCODING, "identity")
            .header(header::USER_AGENT, "ai-personas/web.fetch")
            .send()
            .await?;
        let status = response.status();
        let field = |name| {
            response
                .headers()
                .get(name)
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        };
        let encoding = field(header::CONTENT_ENCODING);
        let identity = encoding
            .as_deref()
            .is_none_or(|v| v.trim().eq_ignore_ascii_case("identity"));
        receipt["final_url"] = json!(response.url().as_str());
        receipt["http_status"] = json!(status.as_u16());
        receipt["http_success"] = json!(status.is_success());
        receipt["content_type"] = json!(field(header::CONTENT_TYPE));
        receipt["content_encoding"] = json!(encoding);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.try_reserve(chunk.len())?;
            bytes.extend_from_slice(&chunk);
            receipt["received_bytes"] = json!(bytes.len());
        }
        let signature = if identity {
            Signature::identify(&bytes)
        } else {
            None
        };
        let matched = input.expect.map(|expected| signature == Some(expected));
        receipt["body_complete"] = json!(true);
        receipt["observed_signature"] = json!(signature);
        receipt["signature_matches"] = json!(matched);
        receipt["success"] = json!(status.is_success() && matched != Some(false));
        if !status.is_success() {
            receipt["failure"] = json!("http_status");
        } else if matched == Some(false) {
            receipt["failure"] = json!("expected_header_not_observed");
        }
        let media = signature.map(Signature::media).unwrap_or_else(|| {
            if identity && std::str::from_utf8(&bytes).is_ok() {
                "text/plain; charset=utf-8"
            } else {
                "application/octet-stream"
            }
        });
        Ok::<_, super::Error>(Body { media, bytes })
    };
    let cancellation = async {
        while !stopped() {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    let result = tokio::select! {
        biased;
        _ = cancellation => None,
        outcome = operation => Some(outcome),
    };
    receipt["finished"] = json!(now());
    let body = match result {
        Some(Ok(body)) => Some(body),
        Some(Err(error)) => {
            receipt["failure"] = json!("retrieval_failed");
            receipt["detail"] = json!(error.to_string());
            None
        }
        None => {
            receipt["failure"] = json!("cancelled");
            None
        }
    };
    Ok((receipt, body))
}
