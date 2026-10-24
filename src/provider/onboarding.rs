//! Sealed SIWC bootstrap wire path. It does not construct DirectSettings with
//! invented evidence. Only the auth onboarding backend can dispatch this path,
//! after its durable claim. No persona, tool, runtime state, or user task is sent.
use super::*;

pub(crate) const CATALOG_URL: &str = "https://api.openai.com/v1/models";
pub(crate) const PROBE_PROFILE: &str = "personas-siwc-carrier-probe-v1";

pub(crate) struct CatalogResponse {
    pub status: u16,
    pub unencoded_json: bool,
    pub body: Box<dyn HttpBody>,
}

/// Trusted host transport. Production always uses the fixed public routes.
pub(crate) trait Transport: HttpTransport {
    fn catalog(
        &self,
        bearer: Secret,
        guard: Arc<IntakeLaunchGuard>,
    ) -> BoxFuture<'_, Result<CatalogResponse, Failure>>;
}

impl Transport for ReqwestTransport {
    fn catalog(
        &self,
        bearer: Secret,
        guard: Arc<IntakeLaunchGuard>,
    ) -> BoxFuture<'_, Result<CatalogResponse, Failure>> {
        Box::pin(async move {
            use reqwest::header::{HeaderValue, CONTENT_ENCODING, CONTENT_TYPE};
            if !secret_valid(&bearer, 16_384) {
                return Err(Failure::InvalidCredential);
            }
            let mut authorization = HeaderValue::from_str(&format!("Bearer {}", bearer.expose()))
                .map_err(|_| Failure::InvalidCredential)?;
            authorization.set_sensitive(true);
            let request = self
                .client
                .get(CATALOG_URL)
                .header("authorization", authorization)
                .header("accept", "application/json")
                .header("accept-encoding", "identity")
                .header("user-agent", USER_AGENT)
                .build()
                .map_err(|_| Failure::Configuration)?;
            guard.begin_launch()?;
            let response = self
                .client
                .execute(request)
                .await
                .map_err(|_| Failure::Network)?;
            let headers = response.headers();
            let content_types = headers.get_all(CONTENT_TYPE);
            let mut types = content_types.iter();
            let json = types.next().and_then(|v| v.to_str().ok()).is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            }) && types.next().is_none();
            let content_encodings = headers.get_all(CONTENT_ENCODING);
            let mut encodings = content_encodings.iter();
            let identity = encodings
                .next()
                .is_none_or(|v| v.as_bytes().eq_ignore_ascii_case(b"identity"))
                && encodings.next().is_none();
            Ok(CatalogResponse {
                status: response.status().as_u16(),
                unencoded_json: json && identity,
                body: Box::new(ReqwestBody { response }),
            })
        })
    }
}

/// Exact mechanical echo specimen. It is never adopted as a persona response.
pub(crate) fn specimen(id: &str) -> Result<String, Failure> {
    use crate::protocol as p;
    let reason = "Isolated host transport fixture; no persona work or maintenance.".to_owned();
    let decision = p::Decision {
        version: p::PROTOCOL_VERSION,
        invocation_id: format!("siwc-probe-{id}"),
        epoch: 0,
        work: p::WorkDisposition::Blocked {
            reason: reason.clone(),
            evidence: Vec::new(),
        },
        fragment_maintenance: p::Maintenance::NoChange {
            reason: reason.clone(),
        },
        asset_maintenance: p::Maintenance::NoChange {
            reason: reason.clone(),
        },
        next_fragments: p::SelectionIntent::Clear {
            reason: reason.clone(),
        },
        next_assets: p::SelectionIntent::Clear {
            reason: reason.clone(),
        },
        continuation: p::Continuation::Stop {
            reason: reason.clone(),
        },
        reply: p::ReplyDisposition::None { reason },
        operations: Vec::new(),
    };
    let bytes = encoded(&decision)?;
    p::parse_decision(&bytes).map_err(|_| Failure::Configuration)?;
    String::from_utf8(bytes).map_err(|_| Failure::Configuration)
}

pub(crate) fn request(
    id: &str,
    model: &str,
    effort: &str,
) -> Result<Vec<u8>, Failure> {
    if !identifier(id) || !identifier(model) || reported_effort_label(effort).is_none() {
        return Err(Failure::Configuration);
    }
    let schema = crate::runtime::provider_schema();
    schema_bytes(&schema)?;
    let prompt = format!("Isolated transport conformance echo, not persona judgment. Do not reason about or execute the specimen. Return only the object required by text.format. Its decision_json string must contain exactly the following UTF-8 JSON bytes:\n{}", specimen(id)?);
    // The shared serializer, instructions and route preserve this historical
    // strict-carrier probe. Native persona calls now request direct JSON objects.
    // This probe establishes only its own response shape and reported effort.
    // max_output_tokens is NOT supported by the SIWC preview.
    encoded(&response_only_body(
        &AccessMode::SiwcOAuth.wire_profile(),
        model,
        effort,
        &prompt,
        &[],
        &schema,
    ))
}

pub(crate) fn expiry_guard(
    expires_at_unix: u64,
    clock: Arc<dyn Clock>,
) -> Result<Arc<IntakeLaunchGuard>, Failure> {
    let expiry = i64::try_from(expires_at_unix).map_err(|_| Failure::Configuration)?;
    Ok(Arc::new(IntakeLaunchGuard::new(expiry, clock)))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeasuredUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub actual_cost_microusd: Option<u64>,
}
impl From<&Usage> for MeasuredUsage {
    fn from(v: &Usage) -> Self {
        Self {
            input_tokens: v.input_tokens,
            cached_input_tokens: v.cached_input_tokens,
            output_tokens: v.output_tokens,
            total_tokens: v.total_tokens,
            actual_cost_microusd: v.actual_cost_microusd,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireFailure {
    Cancelled,
    Timeout,
    Denied,
    Expired,
    Http,
    Protocol,
    Limit,
    Unrepresentable,
    Transport,
    Binding,
}
fn classify(f: Failure, status: Option<u16>) -> WireFailure {
    match f {
        Failure::Cancelled => WireFailure::Cancelled,
        Failure::Timeout => WireFailure::Timeout,
        Failure::IntakeExpired | Failure::CredentialExpired => WireFailure::Expired,
        Failure::HttpStatus if matches!(status, Some(401 | 403)) => WireFailure::Denied,
        Failure::HttpStatus => WireFailure::Http,
        Failure::Limit => WireFailure::Limit,
        Failure::Unrepresentable => WireFailure::Unrepresentable,
        Failure::Network => WireFailure::Transport,
        Failure::Binding | Failure::ReportedEffortMismatch | Failure::ReportedModelMismatch => {
            WireFailure::Binding
        }
        _ => WireFailure::Protocol,
    }
}

/// Private complete observation, not an externally supplied proof or a grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub failure: Option<WireFailure>,
    pub dispatch_may_have_begun: bool,
    pub http_status: Option<u16>,
    pub stream_bytes: usize,
    pub stream_sha256: String,
    pub event_count: usize,
    pub eof: bool,
    pub completed: bool,
    pub reported_effort: Option<String>,
    pub previous_reported_effort: Option<String>,
    pub response_id_sha256: Option<String>,
    pub usage: MeasuredUsage,
    pub elapsed_ms: u64,
    pub carrier: Option<String>,
}
impl fmt::Debug for Observation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Observation([PRIVATE])")
    }
}

pub(crate) async fn probe(
    transport: &dyn Transport,
    body: Vec<u8>,
    bearer: Secret,
    model: &str,
    guard: Arc<IntakeLaunchGuard>,
    cancellation: Cancellation,
) -> Observation {
    let mut collector = sse::Collector::new(model).with_mode(AccessMode::SiwcOAuth);
    let started = tokio::time::Instant::now();
    let mut http_status = None;
    let mut eof = false;
    let mut hash = Sha256::new();
    let result = {
        let running = async {
            if cancellation.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            let response = transport
                .post_with_intake_guard(
                    HttpRequest {
                        mode: AccessMode::SiwcOAuth,
                        body,
                        bearer,
                        account_header: None,
                    },
                    guard.clone(),
                )
                .await?;
            http_status = Some(response.status);
            if response.status != 200 {
                return Err(Failure::HttpStatus);
            }
            if !response.is_unencoded_sse {
                return Err(Failure::ContentType);
            }
            let mut stream = response.body;
            loop {
                tokio::task::yield_now().await;
                match stream.read().await? {
                    Some(bytes) => {
                        hash.update(&bytes);
                        collector.push(&bytes)?;
                    }
                    None => {
                        eof = true;
                        return collector.finish();
                    }
                }
            }
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(Failure::Cancelled),
            result = running => result,
        }
    };
    let (carrier, failure) = match result {
        Ok(carrier) => (Some(carrier), None),
        Err(f) => (None, Some(classify(f, http_status))),
    };
    Observation {
        failure,
        dispatch_may_have_begun: guard.checked_at_unix().is_some(),
        http_status,
        stream_bytes: collector.observed,
        stream_sha256: hex::encode(hash.finalize()),
        event_count: collector.events,
        eof,
        completed: collector.completed,
        reported_effort: collector.reported_effort.clone(),
        previous_reported_effort: collector.previous_reported_effort.clone(),
        response_id_sha256: collector.response_id_sha256(),
        usage: MeasuredUsage::from(&collector.usage),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        carrier,
    }
}
