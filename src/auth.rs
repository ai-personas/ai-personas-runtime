//! Host-side Sign in with ChatGPT authentication.
//!
//! The caller supplies trusted JWKS, a nonblocking HTTP transport, a clock, and
//! entropy. This module neither discovers credentials nor performs HTTP itself.
//! Persist `Vault::host_id` before opening an authorization URL. Start the
//! loopback listener first and keep its selected port for the entire attempt.
//!
//! Authentication establishes identity and a direct grant only. Model, effort,
//! and schema capability verification remains the runtime's responsibility.
//! Authorization URLs and HTTP forms are sensitive; callers must not log them.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

macro_rules! opaque_debug {
    ($($ty:ty),+ $(,)?) => {$(
        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($ty), "([REDACTED])"))
            }
        }
    )+};
}

#[cfg(unix)]
pub(crate) mod codex_adapter;
#[cfg(unix)]
pub(crate) mod codex_jsonrpc;
#[cfg(unix)]
pub(crate) mod codex_process;
#[cfg(unix)]
mod vault;
#[cfg(unix)]
pub use vault::{LockedVault, Vault};
#[cfg(unix)]
pub(crate) mod native;
#[cfg(unix)]
pub mod onboarding;

/// Fixed production issuer; JWKS provisioning is a trusted injection boundary.
pub const ISSUER: &str = "https://auth.openai.com";
/// Fixed production authorization endpoint.
pub const AUTHORIZE_ENDPOINT: &str = "https://auth.openai.com/api/accounts/authorize";
/// Fixed production token endpoint.
pub const TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// Fixed direct-grant resource.
pub const RESOURCE: &str = "https://api.openai.com/v1";
pub const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
pub const ACCESS_TOKEN_ENV: &str = "PERSONAS_SIWC_ACCESS_TOKEN";
pub const ACCOUNT_BINDING_ENV: &str = "PERSONAS_SIWC_ACCOUNT_BINDING";

const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const DIRECT_SCOPE: &str = "chatgpt.tokens.use.direct";
const CALLBACK_REGISTRATION: &str = "http://127.0.0.1/auth/callback";

/// Static errors deliberately exclude provider messages and sensitive values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("authentication transport failed")]
    Transport,
    #[error("authentication request timed out")]
    Timeout,
    #[error("invalid authentication protocol message")]
    Protocol,
    #[error("identity validation failed")]
    Identity,
    #[error("required direct grant is absent or invalid")]
    Scope,
    #[error("authentication state or credential expired")]
    Expired,
    #[error("account or host binding mismatch")]
    Binding,
    #[error("credential vault operation failed")]
    Vault,
    #[error("entropy unavailable")]
    Entropy,
    #[error("invalid authorization state")]
    State,
    #[error("authorization denied")]
    Denied,
    #[error("credential vault is busy")]
    Busy,
    #[error("refresh is not permitted yet")]
    TooEarly,
    /// Replacement happened, but its directory durability could not be confirmed.
    #[error("credential commit durability is uncertain")]
    CommitUncertain,
}

pub trait Clock {
    fn now_unix(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs())
    }
}

pub trait Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), AuthError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), AuthError> {
        getrandom::fill(out).map_err(|_| AuthError::Entropy)
    }
}

/// Generate a canonical lowercase UUIDv4 URN. This function does not persist it.
pub fn new_host_id(entropy: &mut dyn Entropy) -> Result<String, AuthError> {
    let mut bytes = [0_u8; 16];
    entropy.fill(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = hex::encode(bytes);
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &encoded[..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..]
    ))
}

fn valid_host_id(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix("urn:uuid:") else {
        return false;
    };
    let bytes = uuid.as_bytes();
    bytes.len() == 36
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, &byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn random_opaque(entropy: &mut dyn Entropy) -> Result<String, AuthError> {
    let mut bytes = [0_u8; 32];
    entropy.fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn valid_opaque(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == value)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn issued_client(value: &str) -> bool {
    identifier(value) && value != DYNAMIC_CLIENT
}

fn identity_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn secret(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

// Inspect every byte of the fixed-length expected value, without a
// content-dependent early return. Length itself is not confidential.
fn constant_time_equal(expected: &[u8], actual: &[u8]) -> bool {
    let mut difference = expected.len() ^ actual.len();
    for (index, byte) in expected.iter().enumerate() {
        difference |= usize::from(*byte ^ actual.get(index).copied().unwrap_or(0));
        difference = std::hint::black_box(difference);
    }
    difference == 0
}

fn validate_callback_base(uri: &str) -> Result<(), AuthError> {
    let rest = uri
        .strip_prefix("http://127.0.0.1:")
        .ok_or(AuthError::Binding)?;
    let (port_text, path) = rest.split_once('/').ok_or(AuthError::Binding)?;
    if port_text.is_empty() || !port_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AuthError::Binding);
    }
    let port = port_text.parse::<u16>().map_err(|_| AuthError::Binding)?;
    if port == 0 || port.to_string() != port_text || path != "auth/callback" {
        return Err(AuthError::Binding);
    }
    Ok(())
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub struct AuthFlow {
    host_id: String,
    callback_uri: String,
}

impl AuthFlow {
    pub fn new(host_id: String, callback_uri: String) -> Result<Self, AuthError> {
        if !valid_host_id(&host_id) {
            return Err(AuthError::Binding);
        }
        validate_callback_base(&callback_uri)?;
        Ok(Self {
            host_id,
            callback_uri,
        })
    }

    /// The host ID must already be persisted. Selected accounts may be expired;
    /// their issuer, subject, client, host, and callback registration stay bound.
    pub fn begin(
        &self,
        entropy: &mut dyn Entropy,
        clock: &dyn Clock,
        selected: Option<&CredentialRecord>,
    ) -> Result<PendingAuthorization, AuthError> {
        if let Some(record) = selected {
            record.data.validate()?;
            if record.data.ext_agent_host_id != self.host_id
                || record.data.callback_registration != CALLBACK_REGISTRATION
            {
                return Err(AuthError::Binding);
            }
        }
        self.begin_bound(
            entropy,
            clock,
            selected.map(CredentialRecord::metadata),
            selected.map(|record| record.data.client_id.clone()),
        )
    }

    /// Resume a credential-free registration with fresh authorization secrets.
    /// This receipt establishes neither a subject nor a grant. Callers with an
    /// existing credential must use `begin` to retain its exact identity binding.
    pub fn begin_registered(
        &self,
        entropy: &mut dyn Entropy,
        clock: &dyn Clock,
        registration: &PendingRegistration,
    ) -> Result<PendingAuthorization, AuthError> {
        registration.data.validate()?;
        if registration.data.host_id != self.host_id
            || registration.data.callback_registration != CALLBACK_REGISTRATION
        {
            return Err(AuthError::Binding);
        }
        self.begin_bound(
            entropy,
            clock,
            None,
            Some(registration.data.client_id.clone()),
        )
    }

    fn begin_bound(
        &self,
        entropy: &mut dyn Entropy,
        clock: &dyn Clock,
        selected: Option<AccountMetadata>,
        expected_client: Option<String>,
    ) -> Result<PendingAuthorization, AuthError> {
        let started_at = clock.now_unix();
        let state = random_opaque(entropy)?;
        let nonce = random_opaque(entropy)?;
        let verifier = random_opaque(entropy)?;
        let client = expected_client.as_deref().unwrap_or(DYNAMIC_CLIENT);
        let mut url = url::Url::parse(AUTHORIZE_ENDPOINT).map_err(|_| AuthError::Protocol)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", client);
            if expected_client.is_none() {
                query.append_pair("agent_name_hint", "AI Personas");
            }
            query
                .append_pair("ext_agent_host_id", &self.host_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &self.callback_uri)
                .append_pair("scope", SCOPES)
                .append_pair("resource", RESOURCE)
                .append_pair("state", &state)
                .append_pair("nonce", &nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &pkce_challenge(&verifier));
        }
        Ok(PendingAuthorization {
            url: url.into(),
            host_id: self.host_id.clone(),
            callback_uri: self.callback_uri.clone(),
            state,
            nonce,
            verifier,
            selected,
            expected_client,
            started_at,
        })
    }
}

/// A validated callback registration receipt, without authorization secrets.
/// It proves neither signed identity nor a usable grant or model capability.
/// Only callback validation and protected vault loading can construct one.
pub struct PendingRegistration {
    data: StoredRegistration,
}

impl PendingRegistration {
    pub fn client_id(&self) -> &str {
        &self.data.client_id
    }

    pub fn host_id(&self) -> &str {
        &self.data.host_id
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoredRegistration {
    version: u32,
    host_id: String,
    callback_registration: String,
    client_id: String,
}

impl StoredRegistration {
    fn validate(&self) -> Result<(), AuthError> {
        if self.version != 1 {
            return Err(AuthError::Vault);
        }
        if !valid_host_id(&self.host_id)
            || !issued_client(&self.client_id)
            || self.callback_registration != CALLBACK_REGISTRATION
        {
            return Err(AuthError::Binding);
        }
        Ok(())
    }

    fn matches(&self, credential: &StoredCredential) -> bool {
        self.host_id == credential.ext_agent_host_id
            && self.callback_registration == credential.callback_registration
            && self.client_id == credential.client_id
    }
}

/// A single-use authorization attempt. It is intentionally neither Clone nor
/// serializable. `exchange(self, ...)` consumes it even when exchange fails.
pub struct PendingAuthorization {
    url: String,
    host_id: String,
    callback_uri: String,
    state: String,
    nonce: String,
    verifier: String,
    selected: Option<AccountMetadata>,
    expected_client: Option<String>,
    started_at: u64,
}

impl PendingAuthorization {
    pub fn authorization_url(&self) -> &str {
        &self.url
    }

    fn check_time(&self, clock: &dyn Clock) -> Result<(), AuthError> {
        let now = clock.now_unix();
        if now < self.started_at {
            return Err(AuthError::Expired);
        }
        Ok(())
    }

    fn validated_callback(
        &self,
        callback_uri: &str,
        clock: &dyn Clock,
    ) -> Result<(String, String), AuthError> {
        self.check_time(clock)?;
        let query = callback_query(callback_uri, &self.callback_uri)?;
        let state = query.get("state").ok_or(AuthError::State)?;
        if !constant_time_equal(self.state.as_bytes(), state.as_bytes()) {
            return Err(AuthError::State);
        }
        if query.contains_key("error") && query.contains_key("code") {
            return Err(AuthError::Protocol);
        }
        if let Some(error) = query.get("error") {
            return Err(if error == "access_denied" {
                AuthError::Denied
            } else {
                AuthError::Protocol
            });
        }
        if query.contains_key("error_description") {
            return Err(AuthError::Protocol);
        }
        let code = query
            .get("code")
            .filter(|code| secret(code))
            .ok_or(AuthError::Protocol)?;
        let client = match (self.expected_client.as_deref(), query.get("client_id")) {
            (Some(expected), Some(client)) if client == expected => expected,
            (Some(_), Some(_)) => return Err(AuthError::Binding),
            (Some(expected), None) => expected,
            (None, Some(client)) if issued_client(client) => client.as_str(),
            _ => return Err(AuthError::Binding),
        };
        Ok((code.clone(), client.to_owned()))
    }

    /// Validate before exchange so an issued registration can survive a failed
    /// code exchange. The receipt contains no code, state, nonce, or verifier.
    pub fn validate_callback_registration(
        &self,
        callback_uri: &str,
        clock: &dyn Clock,
    ) -> Result<PendingRegistration, AuthError> {
        let (_, client_id) = self.validated_callback(callback_uri, clock)?;
        Ok(PendingRegistration {
            data: StoredRegistration {
                version: 1,
                host_id: self.host_id.clone(),
                callback_registration: CALLBACK_REGISTRATION.to_owned(),
                client_id,
            },
        })
    }

    /// Requires a Tokio runtime. Sends the code at most once, without retry.
    /// The returned record does not replace any selected account automatically.
    pub async fn exchange(
        self,
        callback_uri: &str,
        transport: &dyn TokenTransport,
        verifier: &IdTokenVerifier,
        clock: &dyn Clock,
    ) -> Result<CredentialRecord, AuthError> {
        let (code, client) = self.validated_callback(callback_uri, clock)?;
        let response = send_tokens(
            transport,
            form(&[
                ("grant_type", "authorization_code"),
                ("client_id", &client),
                ("code", &code),
                ("code_verifier", &self.verifier),
                ("redirect_uri", &self.callback_uri),
                ("resource", RESOURCE),
            ]),
        )
        .await?;
        self.check_time(clock)?;
        finish_tokens(
            response,
            verifier,
            clock,
            GrantBinding {
                client_id: &client,
                host_id: &self.host_id,
                nonce: &self.nonce,
                selected: self.selected.as_ref(),
                refresh: false,
            },
        )
    }
}

fn callback_query(uri: &str, expected_base: &str) -> Result<BTreeMap<String, String>, AuthError> {
    if uri.contains('#') {
        return Err(AuthError::Protocol);
    }
    let (base, query) = uri.split_once('?').ok_or(AuthError::Protocol)?;
    if base != expected_base {
        return Err(AuthError::Binding);
    }
    let mut fields = BTreeMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(AuthError::Protocol)?;
        let key = decode_query_component(key)?;
        if !matches!(
            key.as_str(),
            "state" | "code" | "client_id" | "scope" | "error" | "error_description"
        ) {
            return Err(AuthError::Protocol);
        }
        let value = decode_query_component(value)?;
        if fields.insert(key, value).is_some() {
            return Err(AuthError::Protocol);
        }
    }
    Ok(fields)
}

fn decode_query_component(value: &str) -> Result<String, AuthError> {
    fn digit(byte: u8) -> Result<u8, AuthError> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            _ => Err(AuthError::Protocol),
        }
    }
    let mut input = value.bytes();
    let mut output = Vec::with_capacity(value.len());
    while let Some(byte) = input.next() {
        output.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = digit(input.next().ok_or(AuthError::Protocol)?)?;
                let low = digit(input.next().ok_or(AuthError::Protocol)?)?;
                high * 16 + low
            }
            other => other,
        });
    }
    String::from_utf8(output).map_err(|_| AuthError::Protocol)
}

/// A trusted adapter must return promptly from `send`, perform nonblocking I/O,
/// retain complete bodies, and avoid logging forms or response bodies.
/// No application duration or byte quota applies; the caller can cancel.
pub trait TokenTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        request: TokenRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TokenResponse, AuthError>> + Send + 'a>,
    >;
}

pub struct TokenRequest {
    pub endpoint: &'static str,
    pub form: Vec<(String, String)>,
}

pub struct TokenResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

fn form(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenPayload {
    access_token: String,
    refresh_token: String,
    id_token: String,
    token_type: String,
    expires_in: u64,
    scope: String,
    #[serde(default)]
    earliest_refresh_at: Option<u64>,
}

async fn send_tokens(
    transport: &dyn TokenTransport,
    form: Vec<(String, String)>,
) -> Result<TokenPayload, AuthError> {
    let request = TokenRequest {
        endpoint: TOKEN_ENDPOINT,
        form,
    };
    let response = transport.send(request).await?;
    parse_token_response(response)
}

fn decode_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, AuthError> {
    let value = crate::files::parse_json_unique(
        bytes,
        &crate::files::JsonLimits {
            max_source_bytes: 0,
            max_depth: 0,
            max_nodes: 0,
            max_decoded_bytes: 0,
        },
    )
    .map_err(|_| AuthError::Protocol)?;
    serde_json::from_value(value).map_err(|_| AuthError::Protocol)
}

fn parse_token_response(response: TokenResponse) -> Result<TokenPayload, AuthError> {
    if response.status != 200 {
        return Err(AuthError::Protocol);
    }
    decode_json(&response.body)
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

// Certificate annotations and other non-key JWK metadata may be ignored.
// Serde still rejects duplicate occurrences of the security-relevant fields.
#[derive(Deserialize)]
struct Jwk {
    kty: String,
    kid: String,
    n: String,
    e: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JwtHeader {
    alg: String,
    kid: String,
    typ: Option<String>,
}

fn jwt_header(token: &str) -> Result<JwtHeader, AuthError> {
    if !secret(token) {
        return Err(AuthError::Identity);
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
    {
        return Err(AuthError::Identity);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AuthError::Identity)?;
    let header: JwtHeader = decode_json(&bytes).map_err(|_| AuthError::Identity)?;
    if header.alg != "RS256"
        || !identity_text(&header.kid)
        || header.typ.as_ref().is_some_and(|typ| typ != "JWT")
    {
        return Err(AuthError::Identity);
    }
    Ok(header)
}

/// Signature verifier with a fixed issuer and algorithm. Supplied JWKS must be
/// obtained through the trusted issuer-discovery adapter, never from JWT headers.
pub struct IdTokenVerifier {
    keys: BTreeMap<String, DecodingKey>,
}

impl IdTokenVerifier {
    pub fn from_jwks(bytes: &[u8]) -> Result<Self, AuthError> {
        let jwks: Jwks = decode_json(bytes).map_err(|_| AuthError::Identity)?;
        if jwks.keys.is_empty() {
            return Err(AuthError::Identity);
        }
        let mut keys = BTreeMap::new();
        for key in jwks.keys {
            if key.kty != "RSA"
                || !identity_text(&key.kid)
                || key.alg.as_ref().is_some_and(|alg| alg != "RS256")
                || key.usage.as_ref().is_some_and(|usage| usage != "sig")
                || key
                    .key_ops
                    .as_ref()
                    .is_some_and(|ops| ops.len() != 1 || ops[0] != "verify")
            {
                return Err(AuthError::Identity);
            }
            let n = URL_SAFE_NO_PAD
                .decode(&key.n)
                .map_err(|_| AuthError::Identity)?;
            let e = URL_SAFE_NO_PAD
                .decode(&key.e)
                .map_err(|_| AuthError::Identity)?;
            if n.is_empty()
                || n[0] == 0
                || n.last().is_none_or(|byte| byte & 1 == 0)
                || e.is_empty()
                || e[0] == 0
                || URL_SAFE_NO_PAD.encode(&n) != key.n
                || URL_SAFE_NO_PAD.encode(&e) != key.e
            {
                return Err(AuthError::Identity);
            }
            if (e.len() == 1 && e[0] < 3) || e.last().is_none_or(|byte| byte & 1 == 0) {
                return Err(AuthError::Identity);
            }
            let decoded = DecodingKey::from_rsa_components(&key.n, &key.e)
                .map_err(|_| AuthError::Identity)?;
            if keys.insert(key.kid, decoded).is_some() {
                return Err(AuthError::Identity);
            }
        }
        Ok(Self { keys })
    }

    fn verify(
        &self,
        token: &str,
        client: &str,
        nonce: &str,
        allow_missing_nonce: bool,
        now: u64,
    ) -> Result<IdClaims, AuthError> {
        let header = jwt_header(token)?;
        let key = self.keys.get(&header.kid).ok_or(AuthError::Identity)?;
        let mut validation = Validation::new(Algorithm::RS256);
        // Signature checking remains enabled. Time, issuer, and audience checks
        // below use the injected clock and deliberately reject multiple audiences.
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        validation.required_spec_claims.clear();
        let claims = decode::<IdClaims>(token, key, &validation)
            .map_err(|_| AuthError::Identity)?
            .claims;
        if claims.iss != ISSUER
            || !claims.aud.matches(client)
            || !identity_text(&claims.sub)
            || claims.azp.as_ref().is_some_and(|azp| azp != client)
        {
            return Err(AuthError::Identity);
        }
        if claims.exp <= now {
            return Err(AuthError::Expired);
        }
        if claims.iat > now
            || claims.iat >= claims.exp
            || claims.nbf.is_some_and(|nbf| nbf > now || nbf >= claims.exp)
        {
            return Err(AuthError::Identity);
        }
        match &claims.nonce {
            Some(actual) if constant_time_equal(nonce.as_bytes(), actual.as_bytes()) => {}
            None if allow_missing_nonce => {}
            _ => return Err(AuthError::Identity),
        }
        Ok(claims)
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Array(Vec<String>),
}

impl Audience {
    fn matches(&self, expected: &str) -> bool {
        match self {
            Self::One(value) => value == expected,
            Self::Array(values) => values.len() == 1 && values[0] == expected,
        }
    }
}

#[derive(Clone, Deserialize)]
struct IdClaims {
    iss: String,
    sub: String,
    aud: Audience,
    azp: Option<String>,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    nonce: Option<String>,
}

fn valid_scopes(scopes: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    scopes.iter().all(|scope| {
        !scope.is_empty()
            && scope
                .bytes()
                .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
            && seen.insert(scope.as_str())
    }) && scopes.iter().any(|scope| scope == "resource.invoke")
        && scopes.iter().any(|scope| scope == DIRECT_SCOPE)
}

fn parse_scopes(scope: &str) -> Result<Vec<String>, AuthError> {
    let scopes: Vec<_> = scope.split(' ').map(str::to_owned).collect();
    if !valid_scopes(&scopes) {
        return Err(AuthError::Scope);
    }
    Ok(scopes)
}

struct GrantBinding<'a> {
    client_id: &'a str,
    host_id: &'a str,
    nonce: &'a str,
    selected: Option<&'a AccountMetadata>,
    refresh: bool,
}

fn finish_tokens(
    response: TokenPayload,
    verifier: &IdTokenVerifier,
    clock: &dyn Clock,
    binding: GrantBinding<'_>,
) -> Result<CredentialRecord, AuthError> {
    if response.token_type != "Bearer"
        || !secret(&response.access_token)
        || !secret(&response.refresh_token)
        || response.expires_in == 0
    {
        return Err(AuthError::Protocol);
    }
    let now = clock.now_unix();
    let expires = now
        .checked_add(response.expires_in)
        .ok_or(AuthError::Protocol)?;
    if response
        .earliest_refresh_at
        .is_some_and(|at| at < now || at > expires)
    {
        return Err(AuthError::Protocol);
    }
    let scopes = parse_scopes(&response.scope)?;
    let claims = verifier.verify(
        &response.id_token,
        binding.client_id,
        binding.nonce,
        binding.refresh,
        now,
    )?;
    if let Some(expected) = binding.selected {
        if expected.issuer != claims.iss
            || expected.subject != claims.sub
            || expected.client_id != binding.client_id
            || expected.ext_agent_host_id != binding.host_id
        {
            return Err(AuthError::Binding);
        }
    }
    let data = StoredCredential {
        version: 1,
        issuer: claims.iss,
        subject: claims.sub,
        client_id: binding.client_id.to_owned(),
        ext_agent_host_id: binding.host_id.to_owned(),
        callback_registration: CALLBACK_REGISTRATION.to_owned(),
        nonce: binding.nonce.to_owned(),
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        id_token: response.id_token,
        token_type: response.token_type,
        scopes,
        saved_at_unix: now,
        expires_at_unix: expires,
        earliest_refresh_at: response.earliest_refresh_at,
        refresh_uncertain: None,
    };
    data.validate()?;
    Ok(CredentialRecord::new(data, !binding.refresh))
}

// Only this private disk representation implements serialization. Protected
// local persistence is trusted on load; load performs structural validation,
// while signature validation occurs before every initial or refreshed save.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoredCredential {
    version: u32,
    issuer: String,
    subject: String,
    client_id: String,
    ext_agent_host_id: String,
    callback_registration: String,
    nonce: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    token_type: String,
    scopes: Vec<String>,
    saved_at_unix: u64,
    expires_at_unix: u64,
    earliest_refresh_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_uncertain: Option<[u8; 32]>,
}

impl StoredCredential {
    fn validate(&self) -> Result<(), AuthError> {
        if self.version != 1
            || self.issuer != ISSUER
            || !identity_text(&self.subject)
            || !issued_client(&self.client_id)
        {
            return Err(AuthError::Identity);
        }
        if !valid_host_id(&self.ext_agent_host_id)
            || self.callback_registration != CALLBACK_REGISTRATION
        {
            return Err(AuthError::Binding);
        }
        if !valid_opaque(&self.nonce) {
            return Err(AuthError::State);
        }
        if self.token_type != "Bearer"
            || !secret(&self.access_token)
            || !secret(&self.refresh_token)
        {
            return Err(AuthError::Protocol);
        }
        jwt_header(&self.id_token)?;
        let lifetime = self
            .expires_at_unix
            .checked_sub(self.saved_at_unix)
            .ok_or(AuthError::Protocol)?;
        if lifetime == 0
            || self
                .earliest_refresh_at
                .is_some_and(|at| at < self.saved_at_unix || at > self.expires_at_unix)
        {
            return Err(AuthError::Protocol);
        }
        if !valid_scopes(&self.scopes) {
            return Err(AuthError::Scope);
        }
        Ok(())
    }

    fn same_registration(&self, other: &Self) -> bool {
        self.issuer == other.issuer
            && self.subject == other.subject
            && self.client_id == other.client_id
            && self.ext_agent_host_id == other.ext_agent_host_id
            && self.callback_registration == other.callback_registration
    }
}

pub struct CredentialRecord {
    data: StoredCredential,
    // One-use authority minted only after verifying an authorization response.
    // Loading protected bytes never reconstructs this authority.
    verified_authorization: std::sync::atomic::AtomicBool,
    #[cfg(unix)]
    source: std::sync::Mutex<Option<vault::RecordSource>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountMetadata {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub ext_agent_host_id: String,
    pub expires_at_unix: u64,
    pub scopes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityState {
    VerificationPending,
}

impl CredentialRecord {
    fn new(data: StoredCredential, verified_authorization: bool) -> Self {
        Self {
            data,
            verified_authorization: std::sync::atomic::AtomicBool::new(verified_authorization),
            #[cfg(unix)]
            source: std::sync::Mutex::new(None),
        }
    }

    pub fn metadata(&self) -> AccountMetadata {
        AccountMetadata {
            issuer: self.data.issuer.clone(),
            subject: self.data.subject.clone(),
            client_id: self.data.client_id.clone(),
            ext_agent_host_id: self.data.ext_agent_host_id.clone(),
            expires_at_unix: self.data.expires_at_unix,
            scopes: self.data.scopes.clone(),
        }
    }

    /// Fixed-size binding: lowercase SHA-256 of the canonical JSON tuple.
    /// Email is neither account binding nor workspace identity. Provider evidence
    /// hashes the exact returned binding bytes again.
    pub fn account_binding(&self) -> String {
        let canonical =
            serde_json::json!([self.data.issuer, self.data.subject, self.data.client_id,])
                .to_string();
        format!(
            "personas-siwc-account-v1:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        )
    }

    pub fn expires_at_unix(&self) -> u64 {
        self.data.expires_at_unix
    }

    pub fn earliest_refresh_at_unix(&self) -> Option<u64> {
        self.data.earliest_refresh_at
    }

    pub fn handoff(&self, clock: &dyn Clock) -> Result<RuntimeHandoff, AuthError> {
        self.data.validate()?;
        if self.data.refresh_uncertain.is_some() {
            return Err(AuthError::State);
        }
        #[cfg(unix)]
        if let Some(source) = self.source.lock().map_err(|_| AuthError::Vault)?.as_ref() {
            source.check(&self.data)?;
        }
        let now = clock.now_unix();
        if now < self.data.saved_at_unix || now >= self.data.expires_at_unix {
            return Err(AuthError::Expired);
        }
        Ok(RuntimeHandoff {
            access_token: self.data.access_token.clone(),
            account_binding: self.account_binding(),
        })
    }
}

/// A child-process credential handoff, not a capability-readiness assertion.
/// Supply runtime expiry configuration separately from `AccountMetadata`.
pub struct RuntimeHandoff {
    access_token: String,
    account_binding: String,
}

impl RuntimeHandoff {
    pub fn capability_state(&self) -> CapabilityState {
        CapabilityState::VerificationPending
    }

    /// Sets exactly two child environment overrides; never spawns, changes args,
    /// or modifies global environment. Apply near process launch after checking
    /// expiry. WARNING: formatting the Command afterwards may reveal credentials;
    /// callers must never surface its Debug representation.
    pub fn apply_to(&self, command: &mut std::process::Command) {
        command
            .env(ACCESS_TOKEN_ENV, &self.access_token)
            .env(ACCOUNT_BINDING_ENV, &self.account_binding);
    }
}

opaque_debug!(
    AuthFlow,
    PendingAuthorization,
    PendingRegistration,
    TokenRequest,
    TokenResponse,
    IdTokenVerifier,
    CredentialRecord,
    RuntimeHandoff
);
