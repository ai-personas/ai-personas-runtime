//! HTTP failure metadata and credential-redacted structured explanations.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub body_bytes_observed: usize,
    pub body_sha256: String,
    pub body_complete: bool,
    pub json_observed: bool,
    pub error_type: Option<&'static str>,
    pub error_code: Option<&'static str>,
    pub unsupported_parameter: Option<&'static str>,
    /// Structured provider explanation, with request credentials removed.
    pub message: Option<String>,
    pub message_redacted: bool,
}

pub(super) struct Capture {
    bytes: Vec<u8>,
    hash: Sha256,
    secrets: [Secret; 2],
}

impl Capture {
    pub fn new(secrets: [Secret; 2]) -> Self {
        Self { bytes: Vec::new(), hash: Sha256::new(), secrets }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.hash.update(bytes);
        self.bytes.extend_from_slice(bytes);
    }

    pub fn diagnostic(&self, complete: bool) -> Diagnostic {
        let mut result = Diagnostic {
            body_bytes_observed: self.bytes.len(),
            body_sha256: hex::encode(self.hash.clone().finalize()),
            body_complete: complete,
            json_observed: false,
            error_type: None,
            error_code: None,
            unsupported_parameter: None,
            message: None,
            message_redacted: false,
        };
        if !complete { return result; }
        let Ok(value) = serde_json::from_slice::<Value>(&self.bytes) else { return result; };
        result.json_observed = true;
        let error = &value["error"];
        if let Some(message) = error["message"].as_str().or_else(|| value["detail"].as_str()) {
            let mut message = message.to_owned();
            for secret in &self.secrets {
                if message.contains(secret.expose()) {
                    message = message.replace(secret.expose(), "[REDACTED]");
                    result.message_redacted = true;
                }
            }
            result.message = Some(message);
        }
        result.error_type = known(error["type"].as_str(), &[
            "invalid_request_error", "authentication_error", "permission_error",
            "rate_limit_error", "server_error", "service_unavailable_error",
        ]);
        result.error_code = known(error["code"].as_str(), &[
            "unsupported_parameter", "unknown_parameter", "invalid_value", "invalid_api_key",
            "model_not_found", "context_length_exceeded", "rate_limit_exceeded",
            "insufficient_quota", "server_error",
        ]);
        const PARAMETERS: &[&str] = &[
            "model", "store", "stream", "instructions", "input", "reasoning", "text",
            "prompt_cache_options", "prompt_cache_breakpoint", "prompt_cache_key",
            "prompt_cache_retention", "max_output_tokens", "temperature", "top_p",
        ];
        if matches!(result.error_code, Some("unsupported_parameter" | "unknown_parameter")) {
            result.unsupported_parameter = known(error["param"].as_str(), PARAMETERS);
        }
        // The traditional ChatGPT route may use a JSON detail string instead
        // of the public API's error object. Recognize only complete fixed forms.
        for message in [value["detail"].as_str(), error["message"].as_str()].into_iter().flatten() {
            for parameter in PARAMETERS {
                for prefix in ["Unsupported parameter: ", "Unknown parameter: ", "Unrecognized request argument supplied: "] {
                    if [format!("{prefix}{parameter}"), format!("{prefix}'{parameter}'"),
                        format!("{prefix}{parameter}."), format!("{prefix}'{parameter}'.")]
                        .iter().any(|form| form == message)
                    {
                        result.unsupported_parameter = Some(parameter);
                    }
                }
                if message == format!("{parameter} is not supported on this model") {
                    result.unsupported_parameter = Some(parameter);
                }
            }
        }
        result
    }
}

fn known(value: Option<&str>, labels: &[&'static str]) -> Option<&'static str> {
    value.map(|value| labels.iter().copied().find(|label| *label == value).unwrap_or("unrecognized"))
}
