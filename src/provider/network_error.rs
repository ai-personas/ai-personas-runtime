//! Per-attempt transport diagnostics without URLs, credentials or free-form text.
use super::*;
use std::error::Error as StdError;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    AwaitingResponseHeaders,
    ReadingResponseBody,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cause {
    /// Fingerprint only: a source error's text can contain request details.
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_os_error: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub stage: Stage,
    pub is_connect: bool,
    pub is_timeout: bool,
    pub is_request: bool,
    pub is_body: bool,
    pub is_decode: bool,
    pub causes: Vec<Cause>,
}

/// Shared only by one HTTP attempt and its response body, never by the transport
/// client or the credential source. Concurrent calls cannot exchange diagnostics.
#[derive(Clone, Default)]
pub struct TransportDiagnostics(Arc<Mutex<Option<Diagnostic>>>);

impl TransportDiagnostics {
    pub(super) fn capture(&self, stage: Stage, error: &reqwest::Error) {
        let mut causes = Vec::new();
        let mut current: Option<&(dyn StdError + 'static)> = Some(error);
        while let Some(cause) = current {
            let io = cause.downcast_ref::<std::io::Error>();
            causes.push(Cause {
                sha256: digest(cause.to_string().as_bytes()),
                io_kind: io.map(|value| format!("{:?}", value.kind())),
                raw_os_error: io.and_then(std::io::Error::raw_os_error),
            });
            current = cause.source();
        }
        let diagnostic = Diagnostic {
            stage,
            is_connect: error.is_connect(),
            is_timeout: error.is_timeout(),
            is_request: error.is_request(),
            is_body: error.is_body(),
            is_decode: error.is_decode(),
            causes,
        };
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert(diagnostic);
    }

    pub(super) fn snapshot(&self) -> Option<Diagnostic> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner()).clone()
    }
}
