use super::*;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{symlink, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier, Mutex,
    },
};

const NOW: u64 = 1_700_000_000;
const CLIENT: &str = "oaiapp_synthetic";
const SUBJECT: &str = "synthetic-subject";
const CALLBACK: &str = "http://127.0.0.1:43123/auth/callback";
const MODULUS: &str = "nyC5nmk_goSg8nlobdZUq6OF00Pz_fCgXcK4BYBpaoyww3CkJYN90B3BG5xYTXzv9ZUf_3zYlZbjqQDhf2y9A31Gmj3ygz51KsZjOIjZ6FXK-i8fC8QTh7uVcYo9cixgP7OrdfL2oj2p2xsp6QbenQHst3bOb3h5jmGVPe40X1mjHisTWSS6R0E8HjTga-bj2smyBsulEYPPmblD0dolHbHBBmXy47O94q689SlCnO2G_S3Zy09iV4Giz5_R3c-6CJj1VgwpemAxlqh6Sdh4YCwBYVsJ74iAPd9OsbtdtslXfkSAAoE2ZSTsdq9oAHbWhEyvxn32kDPKFAhzU2u29Q";
// Deliberately public synthetic test key; never referenced by production code.
const PRIVATE_PKCS8: &str = "MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCfILmeaT+ChKDyeWht1lSro4XTQ/P98KBdwrgFgGlqjLDDcKQlg33QHcEbnFhNfO/1lR//fNiVluOpAOF/bL0DfUaaPfKDPnUqxmM4iNnoVcr6Lx8LxBOHu5Vxij1yLGA/s6t18vaiPanbGynpBt6dAey3ds5veHmOYZU97jRfWaMeKxNZJLpHQTweNOBr5uPaybIGy6URg8+ZuUPR2iUdscEGZfLjs73irrz1KUKc7Yb9LdnLT2JXgaLPn9Hdz7oImPVWDCl6YDGWqHpJ2HhgLAFhWwnviIA9306xu122yVd+RIACgTZlJOx2r2gAdtaETK/GffaQM8oUCHNTa7b1AgMBAAECggEAQ6GXAprc4U4ZUbUEC1rC1chEQoxpEQ7KOc9egwgWxbBSGF6xhTgjquZagq+qYBSvsALXfLDFJJ7i15uGO7AtjWQbM8cMYXeUOL/kv8x6QOLlx/trCIcnfdLeK0mD4Bi4N5rXTuQaqs3meTdMUrwWtUhyIW+fQYVEtJ0I3y90lulIdmnBbh8gRbxr2RBOgLDNKHrralJ9zyvA5ZK0oADZZGowlV/T0nRZatf36EdXyD/26PMjZz3puYy4tzZ+r2ONRmnlfOqhi7QipjmOHs/XuSrlL5ePylOxuWzgOReu4/dBRidIIDoeR7Q094juxc1JmbJwoxR7NBdUwPx4G44B6QKBgQDLepy9WZGW+7e9m/l68LFmBcwJlwJX5E5q8ufWJzijSu2tegxddYg9Mtxj/FZ1VAmTjIW5iHHp+5/NOdhqEmE7NVAeJgBPTAFEwSDLrYVNLPMgCCJD5zt3UuRlP7ba2BNSzNYmLaNb4xqjTA5iHcbPssg55QjoyLo2r+qAnfAzPwKBgQDIM3//798tw78BuJW6NguoD6CX06LFw0LidhLm1M2Swm216qTA2FJmNNYXKei7BGcvQ34oXSzoCCZp2fHsRYnEfeQz0sI/jWgKBU1DJg7pwKA32QDmODU/pdwk9xUljM0q7qL01FAb0bz3f+boJ+AQKmRTkRUw/jXU+Tw/zQvsywKBgG/j/qjIxhxmUsV7LZcDICnf6jEjzvAR5WU+KWpDqwtwOn54bLA/3yj3/OUmb0yZZ6KxVR+O/kUSTu+FbZH2hW0PhfzuHKF+Q/xQzv3xbynfG9pzXpREZRjahil+/cgdpZKUfPYFhp0SpyVAv3AIu7iesSfU4C9RocRMvAD52In1AoGACNpOJLEku09WmOripBNTvKDERzJyljAmOwFhoJd0Z7L1SwAPPP/T1PUtKmUOB4cQUQS5BWvpWnFR0YhzrDD/AJNYjfoZ4NmN6jOF0RxTuH5uF0ofQg7mO+RHsdLDOp/ZSra2ugDRcgl6Q6+CNG3e+JgHS7K0ry/ujXZf9apJhrUCgYARY3TqOZn2bhWW3EBqwyrH8LaGA+zAf2UBsLkA4NIFmDvogIfYtOOK9uqvAGCKw/svcwxEbLFgtI8aZG+J6p4GXU/4t6sd2TlyZocKpPkf06eHCACEKfNfwNp6aJYTGpKSXchwgdI3s/YQq7nR+7QD1mUH2voYmbvfbJ35UeVsWg==";

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_unix(&self) -> u64 {
        self.0
    }
}

struct CounterEntropy(u8);
impl Entropy for CounterEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), AuthError> {
        for byte in out {
            *byte = self.0;
            self.0 = self.0.wrapping_add(1);
        }
        Ok(())
    }
}

fn host() -> String {
    "urn:uuid:07070707-0707-4707-8707-070707070707".to_owned()
}
fn nonce() -> String {
    URL_SAFE_NO_PAD.encode([9_u8; 32])
}

fn jwks_value() -> Value {
    // Preserve the exact supplied modulus. Its fixed fixture spelling is checked
    // independently of JWT construction so signatures exercise the public key.
    let modulus = MODULUS;
    json!({"keys": [{"kty": "RSA", "kid": "synthetic-only-1", "alg": "RS256", "use": "sig", "n": modulus, "e": "AQAB"}]})
}

fn verifier() -> IdTokenVerifier {
    IdTokenVerifier::from_jwks(&serde_json::to_vec(&jwks_value()).unwrap()).unwrap()
}

fn claims(nonce: Option<&str>) -> Value {
    let mut claims = json!({
        "iss": ISSUER, "sub": SUBJECT, "aud": CLIENT,
        "iat": NOW, "nbf": NOW, "exp": NOW + 3600,
    });
    if let Some(nonce) = nonce {
        claims["nonce"] = json!(nonce);
    }
    claims
}

fn sign(claims: &Value) -> String {
    let der = STANDARD.decode(PRIVATE_PKCS8).unwrap();
    // The supplied fixed PKCS8 fixture contains its PKCS1 private key at byte 26.
    // This avoids the optional PEM feature and any extra ASN.1 dependency.
    assert_eq!(
        &der[..26],
        &[
            0x30, 0x82, 0x04, 0xbc, 0x02, 0x01, 0x00, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48,
            0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00, 0x04, 0x82, 0x04, 0xa6,
        ]
    );
    let key = EncodingKey::from_rsa_der(&der[26..]);
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("synthetic-only-1".to_owned());
    encode(&header, claims, &key).unwrap()
}

fn body(id_token: String, access: &str, refresh: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "access_token": access, "refresh_token": refresh, "id_token": id_token,
        "token_type": "Bearer", "expires_in": 3600, "scope": SCOPES,
    }))
    .unwrap()
}

fn make_record(host: &str, access: &str, refresh: &str) -> CredentialRecord {
    let nonce = nonce();
    let payload = parse_token_response(TokenResponse {
        status: 200,
        body: body(sign(&claims(Some(&nonce))), access, refresh),
    })
    .unwrap();
    finish_tokens(
        payload,
        &verifier(),
        &FixedClock(NOW),
        GrantBinding {
            client_id: CLIENT,
            host_id: host,
            nonce: &nonce,
            selected: None,
            refresh: false,
        },
    )
    .unwrap()
}

struct Fake {
    next: Mutex<Option<Result<TokenResponse, AuthError>>>,
    requests: Mutex<Vec<TokenRequest>>,
}

impl Fake {
    fn new(body: Vec<u8>) -> Self {
        Self {
            next: Mutex::new(Some(Ok(TokenResponse { status: 200, body }))),
            requests: Mutex::new(Vec::new()),
        }
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl TokenTransport for Fake {
    fn send<'a>(
        &'a self,
        request: TokenRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TokenResponse, AuthError>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            self.next
                .lock()
                .unwrap()
                .take()
                .expect("transport must be called once")
        })
    }
}

fn pending() -> PendingAuthorization {
    AuthFlow::new(host(), CALLBACK.to_owned())
        .unwrap()
        .begin(&mut CounterEntropy(1), &FixedClock(NOW), None)
        .unwrap()
}

fn callback(pending: &PendingAuthorization, client: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("state", &pending.state)
        .append_pair("code", "synthetic-code");
    if let Some(client) = client {
        query.append_pair("client_id", client);
    }
    format!("{}?{}", pending.callback_uri, query.finish())
}

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("personas-auth-test-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn vault_fixture() -> (tempfile::TempDir, Vault, String) {
    let directory = private_dir();
    let vault = Vault::open(&directory.path().join("account.json")).unwrap();
    let host = vault.host_id(&mut CounterEntropy(42)).unwrap();
    (directory, vault, host)
}

#[test]
fn host_ids_require_canonical_uuid_v4_urns() {
    let mut entropy = CounterEntropy(0xf0);
    let id = new_host_id(&mut entropy).unwrap();
    assert_eq!(id, "urn:uuid:f0f1f2f3-f4f5-46f7-b8f9-fafbfcfdfeff");
    assert_eq!(entropy.0, 0);
    assert!(valid_host_id(&id));
    let mut record = make_record(&id, "access", "refresh");
    let mut receipt = registration_for(&id, CLIENT);
    for invalid in [
        URL_SAFE_NO_PAD.encode([7_u8; 32]),
        id.replace('f', "F"),
        id.replace("-46", "-56"),
        id.replace("-b8", "-c8"),
        id.replace('-', ""),
        id[9..].to_owned(),
        format!("{id}\n"),
    ] {
        assert!(!valid_host_id(&invalid));
        assert_eq!(
            AuthFlow::new(invalid.clone(), CALLBACK.to_owned()).unwrap_err(),
            AuthError::Binding
        );
        record.data.ext_agent_host_id = invalid.clone();
        receipt.data.host_id = invalid;
        assert_eq!(record.data.validate(), Err(AuthError::Binding));
        assert_eq!(receipt.data.validate(), Err(AuthError::Binding));
    }
}

#[test]
fn invalid_existing_host_file_is_preserved() {
    let directory = private_dir();
    let path = directory.path().join(".personas-host-id");
    let legacy = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    fs::write(&path, &legacy).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    let mut entropy = CounterEntropy(99);
    for _ in 0..2 {
        let vault = Vault::open(&directory.path().join("account.json")).unwrap();
        assert_eq!(vault.host_id(&mut entropy), Err(AuthError::Vault));
        assert_eq!(entropy.0, 99);
        assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    }
}

#[test]
fn s256_and_independent_fresh_random_values() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let flow = AuthFlow::new(host(), CALLBACK.to_owned()).unwrap();
    let mut entropy = CounterEntropy(0);
    let first = flow.begin(&mut entropy, &FixedClock(NOW), None).unwrap();
    let second = flow.begin(&mut entropy, &FixedClock(NOW), None).unwrap();
    let values = [
        &first.state,
        &first.nonce,
        &first.verifier,
        &second.state,
        &second.nonce,
        &second.verifier,
    ];
    assert!(values.iter().all(|value| valid_opaque(value)));
    assert_eq!(values.into_iter().collect::<BTreeSet<_>>().len(), 6);
    let parsed = url::Url::parse(first.authorization_url()).unwrap();
    let query: BTreeMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], DYNAMIC_CLIENT);
    assert_eq!(query["agent_name_hint"], "AI Personas");
    assert_eq!(query["resource"], RESOURCE);
    assert_eq!(query["scope"], SCOPES);
    assert_eq!(query["code_challenge"], pkce_challenge(&first.verifier));
    assert!(!query.contains_key("id_token_hint") && !query.contains_key("login_hint"));
    assert!(!format!("{first:?}").contains(first.authorization_url()));
}

#[test]
fn callback_registration_is_exact_and_bounded() {
    for uri in [
        "https://127.0.0.1:43123/auth/callback",
        "http://localhost:43123/auth/callback",
        "http://127.0.0.1/auth/callback",
        "http://127.0.0.1:0/auth/callback",
        "http://127.0.0.1:65536/auth/callback",
        "http://127.0.0.1:043123/auth/callback",
        "http://user@127.0.0.1:43123/auth/callback",
        "http://127.0.0.1:43123/callback",
        "http://127.0.0.1:43123/auth/callback?x=1",
        "http://127.0.0.1:43123/auth/callback#x",
        "http://127.0.0.1:43123/a/../auth/callback",
    ] {
        assert!(AuthFlow::new(host(), uri.to_owned()).is_err());
    }
    assert!(AuthFlow::new(host(), "http://127.0.0.1:80/auth/callback".to_owned()).is_ok());
    assert!(AuthFlow::new("x".repeat(257), CALLBACK.to_owned()).is_err());
}

#[tokio::test]
async fn initial_exchange_binds_exact_redirect_and_sends_once() {
    let attempt = pending();
    let callback = callback(&attempt, Some(CLIENT));
    let transport = Fake::new(body(
        sign(&claims(Some(&attempt.nonce))),
        "access-one",
        "refresh-one",
    ));
    let record = attempt
        .exchange(&callback, &transport, &verifier(), &FixedClock(NOW))
        .await
        .unwrap();
    // The attempt has moved into exchange and cannot be exchanged a second time.
    assert_eq!(record.metadata().subject, SUBJECT);
    assert_eq!(record.metadata().client_id, CLIENT);
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].endpoint, TOKEN_ENDPOINT);
    assert_eq!(requests[0].timeout, Duration::from_secs(30));
    assert_eq!(requests[0].max_response_bytes, 65536);
    let form: BTreeMap<_, _> = requests[0].form.iter().cloned().collect();
    assert_eq!(form["redirect_uri"], CALLBACK);
    assert_eq!(form["client_id"], CLIENT);
    assert_eq!(form["resource"], RESOURCE);
    assert_eq!(form["grant_type"], "authorization_code");
    assert!(!form.contains_key("client_secret"));
}

#[tokio::test]
async fn callback_tampering_never_exchanges() {
    for case in 0..12 {
        let attempt = pending();
        let valid = callback(&attempt, Some(CLIENT));
        let tampered = match case {
            0 => valid.replace(&attempt.state, "wrong-state"),
            1 => valid.replace(":43123/", ":43124/"),
            2 => callback(&attempt, None),
            3 => callback(&attempt, Some(DYNAMIC_CLIENT)),
            4 => format!("{valid}&state={}", attempt.state),
            5 => format!("{valid}&%73tate={}", attempt.state),
            6 => format!("{valid}&error=access_denied"),
            7 => format!("{valid}#fragment"),
            8 => format!("{valid}&unknown=value"),
            9 => valid.replace("synthetic-code", "%GG"),
            10 => valid.replace("synthetic-code", ""),
            _ => valid.replace("synthetic-code", "%FF"),
        };
        let transport = Fake::new(Vec::new());
        assert!(attempt
            .validate_callback_registration(&tampered, &FixedClock(NOW))
            .is_err());
        assert!(attempt
            .exchange(&tampered, &transport, &verifier(), &FixedClock(NOW))
            .await
            .is_err());
        assert_eq!(transport.count(), 0);
    }
}

#[tokio::test]
async fn denial_requires_state_and_attempt_lifetime_is_finite() {
    let attempt = pending();
    let denied = format!(
        "{CALLBACK}?state={}&error=access_denied&error_description=SECRET",
        attempt.state
    );
    let transport = Fake::new(Vec::new());
    let error = attempt
        .exchange(&denied, &transport, &verifier(), &FixedClock(NOW))
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::Denied);
    assert!(!format!("{error:?} {error}").contains("SECRET"));
    let attempt = pending();
    assert_eq!(
        attempt
            .exchange(
                &format!("{CALLBACK}?state=wrong&error=access_denied"),
                &transport,
                &verifier(),
                &FixedClock(NOW)
            )
            .await
            .unwrap_err(),
        AuthError::State
    );
    let attempt = pending();
    let callback = callback(&attempt, Some(CLIENT));
    assert_eq!(
        attempt
            .exchange(&callback, &transport, &verifier(), &FixedClock(NOW + 600))
            .await
            .unwrap_err(),
        AuthError::Expired
    );
    assert_eq!(transport.count(), 0);
}

#[tokio::test]
async fn reauthorization_uses_registration_and_accepts_only_same_account() {
    let selected = make_record(&host(), "old-access", "old-refresh");
    let flow = AuthFlow::new(host(), "http://127.0.0.1:43124/auth/callback".to_owned()).unwrap();
    for changed_client in [false, true] {
        let attempt = flow
            .begin(&mut CounterEntropy(3), &FixedClock(NOW), Some(&selected))
            .unwrap();
        let query: BTreeMap<_, _> = url::Url::parse(attempt.authorization_url())
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(query["client_id"], CLIENT);
        assert!(
            !query.contains_key("agent_name_hint")
                && !query.contains_key("id_token_hint")
                && !query.contains_key("login_hint")
        );
        let callback = callback(&attempt, changed_client.then_some("oaiapp_other"));
        let transport = Fake::new(body(
            sign(&claims(Some(&attempt.nonce))),
            "new-access",
            "new-refresh",
        ));
        let result = attempt
            .exchange(&callback, &transport, &verifier(), &FixedClock(NOW))
            .await;
        if changed_client {
            assert_eq!(result.unwrap_err(), AuthError::Binding);
            assert_eq!(transport.count(), 0);
        } else {
            assert_eq!(
                result.unwrap().account_binding(),
                selected.account_binding()
            );
        }
    }
    let attempt = flow
        .begin(&mut CounterEntropy(4), &FixedClock(NOW), Some(&selected))
        .unwrap();
    let callback = callback(&attempt, Some(CLIENT));
    let mut changed = claims(Some(&attempt.nonce));
    changed["sub"] = json!("another-subject");
    let transport = Fake::new(body(sign(&changed), "other-access", "other-refresh"));
    assert_eq!(
        attempt
            .exchange(&callback, &transport, &verifier(), &FixedClock(NOW))
            .await
            .unwrap_err(),
        AuthError::Binding
    );
    let other_host = AuthFlow::new(
        new_host_id(&mut CounterEntropy(8)).unwrap(),
        CALLBACK.to_owned(),
    )
    .unwrap();
    assert_eq!(
        other_host
            .begin(&mut CounterEntropy(0), &FixedClock(NOW), Some(&selected))
            .unwrap_err(),
        AuthError::Binding
    );
    assert_eq!(selected.data.access_token, "old-access");
}

#[test]
fn signed_identity_checks_use_injected_time_and_reject_bad_claims() {
    let verifier = verifier();
    let nonce = nonce();
    assert!(verifier
        .verify(&sign(&claims(Some(&nonce))), CLIENT, &nonce, false, NOW)
        .is_ok());
    let mut one_audience = claims(Some(&nonce));
    one_audience["aud"] = json!([CLIENT]);
    assert!(verifier
        .verify(&sign(&one_audience), CLIENT, &nonce, false, NOW)
        .is_ok());
    for case in 0..10 {
        let mut invalid = claims(Some(&nonce));
        match case {
            0 => invalid["iss"] = json!("https://wrong.invalid"),
            1 => invalid["aud"] = json!("wrong-client"),
            2 => invalid["aud"] = json!([CLIENT, CLIENT]),
            3 => invalid["sub"] = json!(""),
            4 => invalid["nonce"] = json!("wrong-nonce"),
            5 => {
                invalid.as_object_mut().unwrap().remove("nonce");
            }
            6 => invalid["exp"] = json!(NOW),
            7 => invalid["iat"] = json!(NOW + 1),
            8 => invalid["nbf"] = json!(NOW + 1),
            _ => {
                invalid.as_object_mut().unwrap().remove("exp");
            }
        }
        assert!(verifier
            .verify(&sign(&invalid), CLIENT, &nonce, false, NOW)
            .is_err());
    }
    let jwt = sign(&claims(Some(&nonce)));
    let mut parts: Vec<String> = jwt.split('.').map(str::to_owned).collect();
    let mut signature = URL_SAFE_NO_PAD.decode(&parts[2]).unwrap();
    signature[0] ^= 1;
    parts[2] = URL_SAFE_NO_PAD.encode(signature);
    assert!(verifier
        .verify(&parts.join("."), CLIENT, &nonce, false, NOW)
        .is_err());
    assert!(verifier
        .verify(&sign(&claims(None)), CLIENT, &nonce, true, NOW)
        .is_ok());
    assert!(verifier
        .verify(&sign(&claims(Some("different"))), CLIENT, &nonce, true, NOW)
        .is_err());
}

#[test]
fn jwks_and_headers_reject_confusion_and_discovery() {
    for case in 0..8 {
        let mut jwks = jwks_value();
        match case {
            0 => jwks["keys"][0]["kty"] = json!("oct"),
            1 => jwks["keys"][0]["alg"] = json!("HS256"),
            2 => jwks["keys"][0]["use"] = json!("enc"),
            3 => jwks["keys"][0]["n"] = json!(URL_SAFE_NO_PAD.encode([0xff_u8; 128])),
            4 => jwks["keys"][0]["e"] = json!("Ag"),
            5 => {
                jwks["keys"][0].as_object_mut().unwrap().remove("kid");
            }
            6 => {
                let key = jwks["keys"][0].clone();
                jwks["keys"].as_array_mut().unwrap().push(key);
            }
            _ => {
                let key = jwks["keys"][0].clone();
                jwks["keys"] = json!(vec![key; 33]);
            }
        }
        assert!(IdTokenVerifier::from_jwks(&serde_json::to_vec(&jwks).unwrap()).is_err());
    }
    assert!(IdTokenVerifier::from_jwks(&vec![b' '; MAX_BODY + 1]).is_err());
    let nonce = nonce();
    let jwt = sign(&claims(Some(&nonce)));
    for header in [
        json!({"alg":"HS256", "kid":"synthetic-only-1"}),
        json!({"alg":"RS256"}),
        json!({"alg":"RS256", "kid":"unknown"}),
        json!({"alg":"RS256", "kid":"synthetic-only-1", "jku":"https://wrong.invalid"}),
        json!({"alg":"RS256", "kid":"synthetic-only-1", "x5u":null}),
        json!({"alg":"RS256", "kid":"synthetic-only-1", "crit":[]}),
    ] {
        let mut parts: Vec<_> = jwt.split('.').map(str::to_owned).collect();
        parts[0] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        assert!(verifier()
            .verify(&parts.join("."), CLIENT, &nonce, false, NOW)
            .is_err());
    }
}

#[test]
fn token_payload_scope_type_lifetime_and_duplicates_fail_closed() {
    let nonce = nonce();
    let valid: Value =
        serde_json::from_slice(&body(sign(&claims(Some(&nonce))), "access", "refresh")).unwrap();
    for case in 0..10 {
        let mut value = valid.clone();
        match case {
            0 => value["scope"] = json!("openid email resource.invoke"),
            1 => value["token_type"] = json!("bearer"),
            2 => value["access_token"] = json!(""),
            3 => value["refresh_token"] = json!(""),
            4 => value["expires_in"] = json!(0),
            5 => value["expires_in"] = json!(3601),
            6 => value["earliest_refresh_at"] = json!(NOW - 1),
            7 => value["earliest_refresh_at"] = json!(NOW + 3601),
            8 => value["scope"] = json!("openid email chatgpt.tokens.use.direct"),
            _ => value["scope"] = json!(format!("{SCOPES} {DIRECT_SCOPE}")),
        }
        let payload = serde_json::from_value(value).unwrap();
        assert!(finish_tokens(
            payload,
            &verifier(),
            &FixedClock(NOW),
            GrantBinding {
                client_id: CLIENT,
                host_id: &host(),
                nonce: &nonce,
                selected: None,
                refresh: false,
            }
        )
        .is_err());
    }
    let payload = serde_json::from_value(valid.clone()).unwrap();
    assert!(finish_tokens(
        payload,
        &verifier(),
        &FixedClock(u64::MAX - 1),
        GrantBinding {
            client_id: CLIENT,
            host_id: &host(),
            nonce: &nonce,
            selected: None,
            refresh: false,
        }
    )
    .is_err());
    let serialized = serde_json::to_string(&valid).unwrap();
    let duplicate = format!("{{\"access_token\":\"duplicate\",{}", &serialized[1..]);
    assert!(parse_token_response(TokenResponse {
        status: 200,
        body: duplicate.into_bytes()
    })
    .is_err());
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("refresh_token");
    assert!(parse_token_response(TokenResponse {
        status: 200,
        body: serde_json::to_vec(&missing).unwrap()
    })
    .is_err());
}

struct Never(AtomicUsize);
impl TokenTransport for Never {
    fn send<'a>(
        &'a self,
        _request: TokenRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TokenResponse, AuthError>> + Send + 'a>,
    > {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn transport_is_bounded_and_errors_are_redacted() {
    let never = Never(AtomicUsize::new(0));
    assert!(matches!(
        send_tokens(&never, Vec::new(), Duration::from_millis(1)).await,
        Err(AuthError::Timeout)
    ));
    assert_eq!(never.0.load(Ordering::SeqCst), 1);
    let fake = Fake::new(Vec::new());
    assert!(send_tokens(
        &fake,
        vec![("code".to_owned(), "a".repeat(MAX_REQUEST))],
        TOKEN_TIMEOUT
    )
    .await
    .is_err());
    assert_eq!(fake.count(), 0);
    for response in [
        TokenResponse {
            status: 200,
            body: vec![b'x'; MAX_BODY + 1],
        },
        TokenResponse {
            status: 201,
            body: b"SECRET-RESPONSE".to_vec(),
        },
        TokenResponse {
            status: 400,
            body: b"SECRET-RESPONSE".to_vec(),
        },
        TokenResponse {
            status: 200,
            body: b"SECRET-RESPONSE".to_vec(),
        },
    ] {
        assert!(!format!("{response:?}").contains("SECRET-RESPONSE"));
        let error = match parse_token_response(response) {
            Err(error) => error,
            Ok(_) => panic!("invalid response accepted"),
        };
        assert!(!format!("{error:?} {error}").contains("SECRET-RESPONSE"));
    }
    let failed = Fake {
        next: Mutex::new(Some(Err(AuthError::Transport))),
        requests: Mutex::new(Vec::new()),
    };
    assert!(matches!(
        send_tokens(&failed, Vec::new(), TOKEN_TIMEOUT).await,
        Err(AuthError::Transport)
    ));
    assert_eq!(failed.count(), 1);
    let request = TokenRequest {
        endpoint: TOKEN_ENDPOINT,
        form: form(&[("code", "SECRET-CODE")]),
        timeout: TOKEN_TIMEOUT,
        max_response_bytes: MAX_BODY,
    };
    assert!(!format!("{request:?}").contains("SECRET-CODE"));
    assert!(!format!("{request:?}").contains(TOKEN_ENDPOINT));
}

#[test]
fn vault_permissions_atomic_failure_and_binding_preserve_old_record() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    let first = make_record(&host, "access-old", "refresh-old");
    guard.save(&first).unwrap();
    let path = directory.path().join("account.json");
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    let replacement = make_record(&host, "access-new", "refresh-new");
    let original_bytes = fs::read(&path).unwrap();
    assert_eq!(
        guard.fail_before_commit(&replacement),
        Err(AuthError::Vault)
    );
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    assert_eq!(
        guard.load().unwrap().unwrap().data.refresh_token,
        "refresh-old"
    );
    assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".personas-tmp-")));
    let mut different = make_record(&host, "other-access", "other-refresh");
    different.data.subject = "different-subject".to_owned();
    assert_eq!(guard.save(&different), Err(AuthError::Binding));
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    // The failed commit consumed this verified authorization's one-use save
    // authority. A retry must not silently restore it or change the old record.
    assert_eq!(guard.save(&replacement), Err(AuthError::State));
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    let replacement = make_record(&host, "access-new", "refresh-new");
    guard.save(&replacement).unwrap();
    let loaded = guard.load().unwrap().unwrap();
    assert_eq!(loaded.data.access_token, "access-new");
    assert_eq!(loaded.data.refresh_token, "refresh-new");
    assert_eq!(loaded.data.id_token, replacement.data.id_token);
    assert_eq!(loaded.expires_at_unix(), replacement.expires_at_unix());
    assert!(!format!("{vault:?} {guard:?} {loaded:?}").contains("refresh-new"));
}

#[test]
fn vault_rejects_unsafe_paths_files_and_lockfiles() {
    let directory = private_dir();
    assert!(Vault::open(std::path::Path::new("relative.json")).is_err());
    for name in [
        ".personas-host-id",
        "account.lock",
        "account.registration",
        "account.refresh",
        ".personas-tmp-collision",
    ] {
        assert!(Vault::open(&directory.path().join(name)).is_err());
    }
    assert!(Vault::open(&directory.path().join("child/../account.json")).is_err());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Vault::open(&directory.path().join("account.json")).is_err());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let child = directory.path().join("private");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&child, directory.path().join("alias")).unwrap();
    assert!(Vault::open(&directory.path().join("alias/account.json")).is_err());
    let victim = directory.path().join("victim");
    fs::write(&victim, b"untouched").unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
    let target = child.join("account.json");
    symlink(&victim, &target).unwrap();
    assert!(Vault::open(&target).is_err());
    fs::remove_file(&target).unwrap();
    fs::write(&target, b"{}").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Vault::open(&target).is_err());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&target, child.join("alias-file")).unwrap();
    assert!(Vault::open(&target).is_err());
    fs::remove_file(child.join("alias-file")).unwrap();
    fs::remove_file(&target).unwrap();
    let vault = Vault::open(&target).unwrap();
    symlink(&victim, child.join("account.json.lock")).unwrap();
    assert!(vault.lock().is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
}

#[test]
fn target_symlink_inserted_after_lock_is_rejected_without_touching_victim() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    let record = make_record(&host, "access", "refresh");
    let victim = directory.path().join("victim");
    fs::write(&victim, b"untouched").unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&victim, directory.path().join("account.json")).unwrap();
    assert!(guard.save(&record).is_err());
    assert_eq!(fs::read(victim).unwrap(), b"untouched");
}

#[test]
fn locks_serialize_and_guard_owns_its_directory() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    assert_eq!(vault.lock().unwrap_err(), AuthError::Busy);
    assert_eq!(
        fs::metadata(directory.path().join("account.json.lock"))
            .unwrap()
            .mode()
            & 0o7777,
        0o600
    );
    drop(vault);
    guard
        .save(&make_record(&host, "access", "refresh"))
        .unwrap();
    drop(guard);
    let reopened = Vault::open(&directory.path().join("account.json")).unwrap();
    assert!(reopened.lock().unwrap().load().unwrap().is_some());
}

#[test]
fn host_id_is_stable_shared_and_bound_to_saved_records() {
    let (directory, vault, host) = vault_fixture();
    let mut entropy = CounterEntropy(99);
    assert_eq!(vault.host_id(&mut entropy).unwrap(), host);
    assert_eq!(entropy.0, 99);
    let second = Vault::open(&directory.path().join("second.json")).unwrap();
    assert_eq!(second.host_id(&mut entropy).unwrap(), host);
    assert_eq!(
        fs::metadata(directory.path().join(".personas-host-id"))
            .unwrap()
            .mode()
            & 0o7777,
        0o600
    );
    let guard = vault.lock().unwrap();
    guard
        .save(&make_record(&host, "access", "refresh"))
        .unwrap();
    fs::write(
        directory.path().join(".personas-host-id"),
        new_host_id(&mut CounterEntropy(3)).unwrap(),
    )
    .unwrap();
    assert_eq!(guard.load().unwrap_err(), AuthError::Binding);
    assert_eq!(guard.host_id(&mut entropy), Err(AuthError::Binding));
    fs::remove_file(directory.path().join(".personas-host-id")).unwrap();
    assert_eq!(guard.host_id(&mut entropy), Err(AuthError::Binding));
    assert_eq!(entropy.0, 99);
}

#[cfg(target_os = "linux")]
#[test]
fn concurrent_host_creation_reads_one_complete_winner() {
    let directory = private_dir();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8_u8)
        .map(|index| {
            let path = directory.path().join(format!("account-{index}.json"));
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let vault = Vault::open(&path).unwrap();
                barrier.wait();
                vault.host_id(&mut CounterEntropy(index)).unwrap()
            })
        })
        .collect();
    let ids: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(ids.iter().all(|id| id == &ids[0] && valid_host_id(id)));
    let metadata = fs::metadata(directory.path().join(".personas-host-id")).unwrap();
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(
        fs::read_to_string(directory.path().join(".personas-host-id")).unwrap(),
        ids[0]
    );
}

#[test]
fn loading_rejects_missing_identity_bad_version_and_oversize() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    let record = make_record(&host, "access", "refresh");
    guard.save(&record).unwrap();
    let path = directory.path().join("account.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (field, value) in [
        ("subject", json!("")),
        ("version", json!(2)),
        ("client_id", json!(DYNAMIC_CLIENT)),
        ("scopes", json!(["openid"])),
        ("scopes", json!([DIRECT_SCOPE])),
        ("scopes", json!(["resource.invoke"])),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(guard.load().is_err());
    }
    fs::write(&path, vec![b' '; MAX_BODY + 1]).unwrap();
    assert!(guard.load().is_err());
}

struct LockProbe {
    path: PathBuf,
    body: Vec<u8>,
    calls: AtomicUsize,
}
impl TokenTransport for LockProbe {
    fn send<'a>(
        &'a self,
        request: TokenRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TokenResponse, AuthError>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                Vault::open(&self.path).unwrap().lock().unwrap_err(),
                AuthError::Busy
            );
            let recovery = self.path.with_file_name("account.json.refresh");
            let fenced: StoredCredential =
                serde_json::from_slice(&fs::read(&recovery).unwrap()).unwrap();
            assert!(fenced.refresh_uncertain.is_some());
            assert_eq!(fenced.access_token, "access-old");
            assert_eq!(fenced.refresh_token, "refresh-old");
            assert_eq!(fs::metadata(recovery).unwrap().mode() & 0o7777, 0o600);
            let form: BTreeMap<_, _> = request.form.into_iter().collect();
            assert_eq!(form["grant_type"], "refresh_token");
            assert_eq!(form["client_id"], CLIENT);
            assert_eq!(form["refresh_token"], "refresh-old");
            assert_eq!(form["resource"], RESOURCE);
            assert!(!form.contains_key("scope"));
            Ok(TokenResponse {
                status: 200,
                body: self.body.clone(),
            })
        })
    }
}

#[tokio::test]
async fn refresh_holds_lock_and_rotates_all_credentials_atomically() {
    let (directory, vault, host) = vault_fixture();
    let mut guard = vault.lock().unwrap();
    guard
        .save(&make_record(&host, "access-old", "refresh-old"))
        .unwrap();
    let new_id = sign(&claims(None));
    let transport = LockProbe {
        path: directory.path().join("account.json"),
        body: body(new_id.clone(), "access-new", "refresh-new"),
        calls: AtomicUsize::new(0),
    };
    let refreshed = guard
        .refresh(&transport, &verifier(), &FixedClock(NOW + 10))
        .await
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let loaded = guard.load().unwrap().unwrap();
    assert_eq!(loaded.data.access_token, "access-new");
    assert_eq!(loaded.data.refresh_token, "refresh-new");
    assert_eq!(loaded.data.id_token, new_id);
    assert_eq!(loaded.data.nonce, nonce());
    assert_eq!(loaded.expires_at_unix(), NOW + 3610);
    assert_eq!(loaded.account_binding(), refreshed.account_binding());
    assert!(loaded.data.refresh_uncertain.is_none());
    assert!(!directory.path().join("account.json.refresh").exists());
    assert!(loaded.handoff(&FixedClock(NOW + 10)).is_ok());
    assert!(refreshed.handoff(&FixedClock(NOW + 10)).is_ok());
}

#[tokio::test]
async fn refresh_failures_fence_replay_and_handoff_but_too_early_changes_nothing() {
    for case in 0..9 {
        let (directory, vault, host) = vault_fixture();
        let mut guard = vault.lock().unwrap();
        let mut record = make_record(&host, "access-old", "refresh-old");
        record.data.earliest_refresh_at = Some(NOW + 60);
        guard.save(&record).unwrap();
        let stale = guard.load().unwrap().unwrap();
        assert_eq!(stale.earliest_refresh_at_unix(), Some(NOW + 60));
        let path = directory.path().join("account.json");
        let original = fs::read(&path).unwrap();
        let unused = Fake::new(Vec::new());
        assert_eq!(
            guard
                .refresh(&unused, &verifier(), &FixedClock(NOW))
                .await
                .unwrap_err(),
            AuthError::TooEarly
        );
        assert_eq!(unused.count(), 0);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!directory.path().join("account.json.refresh").exists());
        assert!(stale.handoff(&FixedClock(NOW)).is_ok());
        let mut identity = claims(None);
        match case {
            0 => identity["sub"] = json!("other-account"),
            1 => identity["aud"] = json!("other-client"),
            2 => identity["nonce"] = json!("wrong-retained-nonce"),
            _ => {}
        }
        let mut response: Value =
            serde_json::from_slice(&body(sign(&identity), "access-new", "refresh-new")).unwrap();
        if case == 3 {
            response["scope"] = json!("openid");
        }
        if case == 4 {
            response["refresh_token"] = json!("");
        }
        if case == 8 {
            response.as_object_mut().unwrap().remove("scope");
        }
        let transport = if case == 5 || case == 6 {
            // The server may already have rotated before either error arrives.
            Fake {
                next: Mutex::new(Some(Err(if case == 5 {
                    AuthError::Transport
                } else {
                    AuthError::Timeout
                }))),
                requests: Mutex::new(Vec::new()),
            }
        } else if case == 7 {
            Fake::new(b"malformed-response".to_vec())
        } else {
            Fake::new(serde_json::to_vec(&response).unwrap())
        };
        assert!(guard
            .refresh(&transport, &verifier(), &FixedClock(NOW + 60))
            .await
            .is_err());
        assert_eq!(transport.count(), 1);
        let recovery_path = directory.path().join("account.json.refresh");
        let fenced_bytes = fs::read(&recovery_path).unwrap();
        let loaded = guard.load().unwrap().unwrap();
        assert!(loaded.data.refresh_uncertain.is_some());
        assert_eq!(loaded.data.access_token, "access-old");
        assert_eq!(loaded.data.refresh_token, "refresh-old");
        assert_eq!(loaded.data.id_token, stale.data.id_token);
        assert_eq!(loaded.metadata(), stale.metadata());
        assert_eq!(
            loaded.handoff(&FixedClock(NOW + 60)).unwrap_err(),
            AuthError::State
        );
        assert_eq!(
            stale.handoff(&FixedClock(NOW + 60)).unwrap_err(),
            AuthError::State
        );
        assert_eq!(
            record.handoff(&FixedClock(NOW + 60)).unwrap_err(),
            AuthError::State
        );
        assert_eq!(guard.save(&stale), Err(AuthError::State));
        assert_eq!(guard.save(&record), Err(AuthError::State));
        assert_eq!(
            guard
                .refresh(&unused, &verifier(), &FixedClock(NOW + 60))
                .await
                .unwrap_err(),
            AuthError::State
        );
        assert_eq!(unused.count(), 0);
        assert_eq!(fs::read(&recovery_path).unwrap(), fenced_bytes);
        drop(guard);
        let reopened = Vault::open(&path).unwrap();
        let mut guard = reopened.lock().unwrap();
        assert_eq!(
            guard
                .refresh(&unused, &verifier(), &FixedClock(NOW + 60))
                .await
                .unwrap_err(),
            AuthError::State
        );
        assert_eq!(unused.count(), 0);
        let selected = guard.load().unwrap().unwrap();
        let flow = AuthFlow::new(host, CALLBACK.to_owned()).unwrap();
        let attempt = flow
            .begin(
                &mut CounterEntropy(17),
                &FixedClock(NOW + 60),
                Some(&selected),
            )
            .unwrap();
        assert_ne!(attempt.nonce, selected.data.nonce);
        let uri = callback(&attempt, None);
        let reauthorization = Fake::new(body(
            sign(&claims(Some(&attempt.nonce))),
            "reauthorized-access",
            "reauthorized-refresh",
        ));
        let replacement = attempt
            .exchange(&uri, &reauthorization, &verifier(), &FixedClock(NOW + 60))
            .await
            .unwrap();
        guard.save(&replacement).unwrap();
        assert!(!recovery_path.exists());
        assert!(guard
            .load()
            .unwrap()
            .unwrap()
            .handoff(&FixedClock(NOW + 60))
            .is_ok());
        assert!(replacement.handoff(&FixedClock(NOW + 60)).is_ok());
        assert_eq!(guard.save(&stale), Err(AuthError::State));
        assert_eq!(
            stale.handoff(&FixedClock(NOW + 60)).unwrap_err(),
            AuthError::State
        );
    }
}

#[tokio::test]
async fn dropping_pending_refresh_leaves_a_durable_fence() {
    let (directory, vault, host) = vault_fixture();
    let mut guard = vault.lock().unwrap();
    guard
        .save(&make_record(&host, "access-old", "refresh-old"))
        .unwrap();
    let stale = guard.load().unwrap().unwrap();
    let never = Never(AtomicUsize::new(0));
    let verifier = verifier();
    let clock = FixedClock(NOW + 1);
    assert!(tokio::time::timeout(
        Duration::from_millis(1),
        guard.refresh(&never, &verifier, &clock)
    )
    .await
    .is_err());
    assert_eq!(never.0.load(Ordering::SeqCst), 1);
    assert_eq!(stale.handoff(&clock).unwrap_err(), AuthError::State);
    drop(guard);
    drop(vault);
    let reopened = Vault::open(&directory.path().join("account.json")).unwrap();
    let mut guard = reopened.lock().unwrap();
    assert!(guard
        .load()
        .unwrap()
        .unwrap()
        .data
        .refresh_uncertain
        .is_some());
    let unused = Fake::new(Vec::new());
    assert_eq!(
        guard.refresh(&unused, &verifier, &clock).await.unwrap_err(),
        AuthError::State
    );
    assert_eq!(unused.count(), 0);
    assert_eq!(guard.save(&stale), Err(AuthError::State));
}

#[tokio::test]
async fn refresh_commit_failures_keep_old_recovery_authoritative() {
    for after_replace in [false, true] {
        let (directory, vault, host) = vault_fixture();
        let mut guard = vault.lock().unwrap();
        guard
            .save(&make_record(&host, "access-old", "refresh-old"))
            .unwrap();
        let stale = guard.load().unwrap().unwrap();
        let transport = Fake::new(body(sign(&claims(None)), "access-new", "refresh-new"));
        let expected = if after_replace {
            AuthError::CommitUncertain
        } else {
            AuthError::Vault
        };
        assert_eq!(
            guard
                .fail_refresh_commit(&transport, &verifier(), &FixedClock(NOW + 1), after_replace)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(transport.count(), 1);
        drop(guard);
        let reopened = Vault::open(&directory.path().join("account.json")).unwrap();
        let mut guard = reopened.lock().unwrap();
        let loaded = guard.load().unwrap().unwrap();
        assert_eq!(loaded.data.access_token, "access-old");
        assert_eq!(loaded.data.refresh_token, "refresh-old");
        assert_eq!(loaded.data.id_token, stale.data.id_token);
        assert!(loaded.data.refresh_uncertain.is_some());
        assert_eq!(
            loaded.handoff(&FixedClock(NOW + 1)).unwrap_err(),
            AuthError::State
        );
        assert_eq!(guard.save(&stale), Err(AuthError::State));
        let unused = Fake::new(Vec::new());
        assert_eq!(
            guard
                .refresh(&unused, &verifier(), &FixedClock(NOW + 1))
                .await
                .unwrap_err(),
            AuthError::State
        );
        assert_eq!(unused.count(), 0);
    }
}

#[tokio::test]
async fn unsafe_refresh_marker_prevents_dispatch() {
    let (directory, vault, host) = vault_fixture();
    let mut guard = vault.lock().unwrap();
    guard
        .save(&make_record(&host, "access-old", "refresh-old"))
        .unwrap();
    let original = fs::read(directory.path().join("account.json")).unwrap();
    let victim = directory.path().join("victim");
    fs::write(&victim, b"untouched").unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&victim, directory.path().join("account.json.refresh")).unwrap();
    let unused = Fake::new(Vec::new());
    assert_eq!(
        guard
            .refresh(&unused, &verifier(), &FixedClock(NOW))
            .await
            .unwrap_err(),
        AuthError::Vault
    );
    assert_eq!(unused.count(), 0);
    assert_eq!(
        fs::read(directory.path().join("account.json")).unwrap(),
        original
    );
    assert_eq!(fs::read(victim).unwrap(), b"untouched");
}

#[test]
fn handoff_changes_only_intended_child_environment_and_checks_expiry() {
    let record = make_record(
        &host(),
        "synthetic-access-secret",
        "synthetic-refresh-secret",
    );
    let before = [
        std::env::var_os(ACCESS_TOKEN_ENV),
        std::env::var_os(ACCOUNT_BINDING_ENV),
    ];
    let handoff = record.handoff(&FixedClock(NOW)).unwrap();
    assert_eq!(
        handoff.capability_state(),
        CapabilityState::VerificationPending
    );
    let mut command = std::process::Command::new("synthetic-child-never-spawned");
    command.arg("safe-argument").env("UNRELATED", "preserved");
    handoff.apply_to(&mut command);
    let environment: BTreeMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(std::ffi::OsStr::to_os_string)))
        .collect();
    assert_eq!(environment.len(), 3);
    assert_eq!(
        environment[std::ffi::OsStr::new("UNRELATED")].as_deref(),
        Some(std::ffi::OsStr::new("preserved"))
    );
    assert_eq!(
        environment[std::ffi::OsStr::new(ACCESS_TOKEN_ENV)].as_deref(),
        Some(std::ffi::OsStr::new("synthetic-access-secret"))
    );
    let binding = record.account_binding();
    assert_eq!(
        environment[std::ffi::OsStr::new(ACCOUNT_BINDING_ENV)].as_deref(),
        Some(std::ffi::OsStr::new(&binding))
    );
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("safe-argument")]
    );
    assert_eq!(
        [
            std::env::var_os(ACCESS_TOKEN_ENV),
            std::env::var_os(ACCOUNT_BINDING_ENV)
        ],
        before
    );
    assert!(!format!("{record:?} {handoff:?}").contains("synthetic-access-secret"));
    assert_eq!(
        record.handoff(&FixedClock(NOW + 3600)).unwrap_err(),
        AuthError::Expired
    );
    assert_eq!(
        record.handoff(&FixedClock(NOW - 1)).unwrap_err(),
        AuthError::Expired
    );
    let canonical = serde_json::to_vec(&[ISSUER, SUBJECT, CLIENT]).unwrap();
    assert_eq!(
        binding,
        format!(
            "personas-siwc-account-v1:{}",
            hex::encode(Sha256::digest(canonical))
        )
    );
}

#[test]
fn binding_is_fixed_size_stable_and_preserves_tuple_boundaries() {
    let mut record = make_record(&host(), "access", "refresh");
    let initial = record.account_binding();
    assert_eq!(initial, record.account_binding());
    let prefix = "personas-siwc-account-v1:";
    assert_eq!(initial.len(), prefix.len() + 64);
    assert!(initial
        .strip_prefix(prefix)
        .unwrap()
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    let mut bindings = BTreeSet::from([initial]);
    for (subject, client) in [
        (SUBJECT, "oaiapp_other"),
        ("other-subject", CLIENT),
        ("ab", "c"),
        ("a", "bc"),
        ("a:b", "c"),
        ("a", "b:c"),
    ] {
        record.data.subject = subject.to_owned();
        record.data.client_id = client.to_owned();
        assert!(bindings.insert(record.account_binding()));
    }
    record.data.subject = "s".repeat(1024);
    record.data.client_id = "c".repeat(256);
    let long = record.account_binding();
    assert_eq!(long.len(), prefix.len() + 64);
    assert!(long.len() <= 512);
    assert_eq!(long, record.account_binding());
    let canonical = serde_json::to_vec(&[
        ISSUER,
        record.data.subject.as_str(),
        record.data.client_id.as_str(),
    ])
    .unwrap();
    assert_eq!(
        long,
        format!("{prefix}{}", hex::encode(Sha256::digest(canonical)))
    );
    record.data.client_id.replace_range(255.., "d");
    assert_ne!(long, record.account_binding());
    record.data.client_id.replace_range(255.., "c");
    record.data.subject.replace_range(1023.., "t");
    assert_ne!(long, record.account_binding());
}

#[test]
fn authorized_party_must_match_even_with_one_correct_audience() {
    let nonce = nonce();
    let mut identity = claims(Some(&nonce));
    identity["azp"] = json!(CLIENT);
    assert!(verifier()
        .verify(&sign(&identity), CLIENT, &nonce, false, NOW)
        .is_ok());
    identity["azp"] = json!("oaiapp_other");
    assert!(matches!(
        verifier().verify(&sign(&identity), CLIENT, &nonce, false, NOW),
        Err(AuthError::Identity)
    ));
    identity["aud"] = json!([CLIENT]);
    assert!(matches!(
        verifier().verify(&sign(&identity), CLIENT, &nonce, false, NOW),
        Err(AuthError::Identity)
    ));
}

fn registration_for(host: &str, client: &str) -> PendingRegistration {
    let attempt = AuthFlow::new(host.to_owned(), CALLBACK.to_owned())
        .unwrap()
        .begin(&mut CounterEntropy(11), &FixedClock(NOW), None)
        .unwrap();
    attempt
        .validate_callback_registration(&callback(&attempt, Some(client)), &FixedClock(NOW))
        .unwrap()
}

#[tokio::test]
async fn failed_code_exchange_retains_only_registration_and_retries_with_fresh_secrets() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    let flow = AuthFlow::new(host.clone(), CALLBACK.to_owned()).unwrap();
    let mut entropy = CounterEntropy(1);
    let attempt = flow.begin(&mut entropy, &FixedClock(NOW), None).unwrap();
    let old_secrets = [
        attempt.state.clone(),
        attempt.nonce.clone(),
        attempt.verifier.clone(),
    ];
    let uri = callback(&attempt, Some(CLIENT));
    let registration = attempt
        .validate_callback_registration(&uri, &FixedClock(NOW))
        .unwrap();
    assert_eq!(registration.client_id(), CLIENT);
    assert_eq!(registration.host_id(), host);
    assert_eq!(
        format!("{registration:?}"),
        "PendingRegistration([REDACTED])"
    );
    guard.retain_pending_registration(&registration).unwrap();
    let path = directory.path().join("account.json.registration");
    let bytes = fs::read(&path).unwrap();
    let stored: Value = serde_json::from_slice(&bytes).unwrap();
    let fields: BTreeSet<_> = stored
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        BTreeSet::from(["version", "host_id", "callback_registration", "client_id"])
    );
    assert_eq!(stored["version"], json!(1));
    assert_eq!(stored["host_id"], json!(host));
    assert_eq!(
        stored["callback_registration"],
        json!(CALLBACK_REGISTRATION)
    );
    assert_eq!(stored["client_id"], json!(CLIENT));
    let text = String::from_utf8(bytes.clone()).unwrap();
    for secret in &old_secrets {
        assert!(!text.contains(secret));
    }
    assert!(!text.contains("synthetic-code"));
    assert!(!text.contains("token"));
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    // SAFETY: geteuid has no arguments or memory-safety preconditions.
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    let failed = Fake {
        next: Mutex::new(Some(Ok(TokenResponse {
            status: 400,
            body: br#"{"error":"invalid_grant"}"#.to_vec(),
        }))),
        requests: Mutex::new(Vec::new()),
    };
    assert_eq!(
        attempt
            .exchange(&uri, &failed, &verifier(), &FixedClock(NOW))
            .await
            .unwrap_err(),
        AuthError::Protocol
    );
    assert_eq!(failed.count(), 1);
    assert!(guard.load().unwrap().is_none());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(guard);
    drop(vault);
    let reopened = Vault::open(&directory.path().join("account.json")).unwrap();
    let guard = reopened.lock().unwrap();
    let retained = guard.pending_registration().unwrap().unwrap();
    assert_eq!(retained.client_id(), CLIENT);
    let retry_flow = AuthFlow::new(
        host.clone(),
        "http://127.0.0.1:43124/auth/callback".to_owned(),
    )
    .unwrap();
    let retry = retry_flow
        .begin_registered(&mut entropy, &FixedClock(NOW + 1), &retained)
        .unwrap();
    assert!(retry.selected.is_none());
    assert_ne!(retry.state, old_secrets[0]);
    assert_ne!(retry.nonce, old_secrets[1]);
    assert_ne!(retry.verifier, old_secrets[2]);
    let query: BTreeMap<_, _> = url::Url::parse(retry.authorization_url())
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(query["client_id"], CLIENT);
    assert_eq!(query["ext_agent_host_id"], host);
    assert!(!query.contains_key("agent_name_hint"));
    let changed = callback(&retry, Some("oaiapp_changed"));
    assert_eq!(
        retry
            .validate_callback_registration(&changed, &FixedClock(NOW + 1))
            .unwrap_err(),
        AuthError::Binding
    );
    let uri = callback(&retry, None);
    let receipt = retry
        .validate_callback_registration(&uri, &FixedClock(NOW + 1))
        .unwrap();
    guard.retain_pending_registration(&receipt).unwrap();
    let transport = Fake::new(body(
        sign(&claims(Some(&retry.nonce))),
        "connected-access",
        "connected-refresh",
    ));
    let record = retry
        .exchange(&uri, &transport, &verifier(), &FixedClock(NOW + 1))
        .await
        .unwrap();
    guard.save(&record).unwrap();
    assert!(!path.exists());
    assert!(guard.pending_registration().unwrap().is_none());
    assert_eq!(guard.load().unwrap().unwrap().metadata().subject, SUBJECT);
    assert_eq!(transport.count(), 1);
}

#[test]
fn invalid_callbacks_never_produce_retained_registration() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    for case in 0..6 {
        let attempt = AuthFlow::new(host.clone(), CALLBACK.to_owned())
            .unwrap()
            .begin(&mut CounterEntropy(1), &FixedClock(NOW), None)
            .unwrap();
        let uri = match case {
            0 => callback(&attempt, Some(CLIENT)).replace(&attempt.state, "wrong-state"),
            1 => format!("{CALLBACK}?state={}&error=access_denied", attempt.state),
            2 => callback(&attempt, None),
            3 => callback(&attempt, Some(DYNAMIC_CLIENT)),
            _ => callback(&attempt, Some(CLIENT)),
        };
        let now = match case {
            4 => NOW + PENDING_LIFETIME,
            5 => NOW - 1,
            _ => NOW,
        };
        let result = attempt.validate_callback_registration(&uri, &FixedClock(now));
        assert!(result.is_err());
        assert!(guard.pending_registration().unwrap().is_none());
        assert!(!directory.path().join("account.json.registration").exists());
    }
}

#[tokio::test]
async fn registered_exchange_rejects_changed_callback_client_without_dispatch() {
    let registration = registration_for(&host(), CLIENT);
    let flow = AuthFlow::new(host(), CALLBACK.to_owned()).unwrap();
    let attempt = flow
        .begin_registered(&mut CounterEntropy(3), &FixedClock(NOW), &registration)
        .unwrap();
    let uri = callback(&attempt, Some("oaiapp_changed"));
    let unused = Fake::new(Vec::new());
    assert_eq!(
        attempt
            .exchange(&uri, &unused, &verifier(), &FixedClock(NOW))
            .await
            .unwrap_err(),
        AuthError::Binding
    );
    assert_eq!(unused.count(), 0);
    let wrong_host = AuthFlow::new(
        new_host_id(&mut CounterEntropy(8)).unwrap(),
        CALLBACK.to_owned(),
    )
    .unwrap();
    assert_eq!(
        wrong_host
            .begin_registered(&mut CounterEntropy(3), &FixedClock(NOW), &registration)
            .unwrap_err(),
        AuthError::Binding
    );
}

#[test]
fn pending_registration_requires_existing_host_and_matching_selected_registration() {
    let directory = private_dir();
    let vault = Vault::open(&directory.path().join("account.json")).unwrap();
    let guard = vault.lock().unwrap();
    let receipt = registration_for(&host(), CLIENT);
    assert_eq!(
        guard.retain_pending_registration(&receipt),
        Err(AuthError::Binding)
    );
    assert!(!directory.path().join("account.json.registration").exists());

    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    let matching = registration_for(&host, CLIENT);
    let other = registration_for(&host, "oaiapp_other");
    guard.retain_pending_registration(&matching).unwrap();
    let path = directory.path().join("account.json.registration");
    let original = fs::read(&path).unwrap();
    assert_eq!(
        guard.retain_pending_registration(&other),
        Err(AuthError::Binding)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_file(directory.path().join(".personas-host-id")).unwrap();
    let mut entropy = CounterEntropy(99);
    assert_eq!(guard.host_id(&mut entropy), Err(AuthError::Binding));
    assert_eq!(entropy.0, 99);
    assert!(guard.pending_registration().is_err());

    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    guard
        .retain_pending_registration(&registration_for(&host, "oaiapp_other"))
        .unwrap();
    assert_eq!(
        guard.save(&make_record(&host, "access", "refresh")),
        Err(AuthError::Binding)
    );
    assert!(!directory.path().join("account.json").exists());
}

#[test]
fn active_credentials_and_pending_registration_must_agree() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    guard
        .save(&make_record(&host, "access", "refresh"))
        .unwrap();
    let original = fs::read(directory.path().join("account.json")).unwrap();
    assert_eq!(
        guard.retain_pending_registration(&registration_for(&host, "oaiapp_other")),
        Err(AuthError::Binding)
    );
    let receipt = registration_for(&host, CLIENT);
    guard.retain_pending_registration(&receipt).unwrap();
    let path = directory.path().join("account.json.registration");
    let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data["client_id"] = json!("oaiapp_other");
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        guard.pending_registration().unwrap_err(),
        AuthError::Binding
    );
    assert_eq!(
        guard.save(&make_record(
            &host,
            "replacement-access",
            "replacement-refresh"
        )),
        Err(AuthError::Binding)
    );
    assert_eq!(
        fs::read(directory.path().join("account.json")).unwrap(),
        original
    );
}

#[test]
fn credential_commit_before_pending_cleanup_is_recoverable() {
    let (directory, vault, host) = vault_fixture();
    let guard = vault.lock().unwrap();
    guard
        .retain_pending_registration(&registration_for(&host, CLIENT))
        .unwrap();
    let record = make_record(&host, "access", "refresh");
    assert_eq!(
        guard.fail_after_commit(&record),
        Err(AuthError::CommitUncertain)
    );
    assert!(directory.path().join("account.json.registration").exists());
    drop(guard);
    let reopened = Vault::open(&directory.path().join("account.json")).unwrap();
    let guard = reopened.lock().unwrap();
    let connected = guard.load().unwrap().unwrap();
    assert_eq!(connected.data.access_token, "access");
    assert!(connected.handoff(&FixedClock(NOW)).is_ok());
    assert_eq!(
        guard.pending_registration().unwrap().unwrap().client_id(),
        CLIENT
    );
    let original = fs::read(directory.path().join("account.json")).unwrap();
    guard.save(&connected).unwrap();
    assert!(guard.pending_registration().unwrap().is_none());
    assert!(!directory.path().join("account.json.registration").exists());
    assert_eq!(
        fs::read(directory.path().join("account.json")).unwrap(),
        original
    );
}

#[test]
fn pending_registration_files_enforce_protection_bounds_and_private_format() {
    for case in 0..8 {
        let (directory, vault, host) = vault_fixture();
        let guard = vault.lock().unwrap();
        let receipt = registration_for(&host, CLIENT);
        guard.retain_pending_registration(&receipt).unwrap();
        let path = directory.path().join("account.json.registration");
        let victim = directory.path().join("victim");
        fs::write(&victim, b"untouched").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        match case {
            0 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            1 => {
                fs::remove_file(&path).unwrap();
                symlink(&victim, &path).unwrap();
            }
            2 => fs::write(&path, vec![b' '; 2049]).unwrap(),
            3 => fs::hard_link(&path, directory.path().join("alias")).unwrap(),
            4..=6 => {
                let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                match case {
                    4 => data["version"] = json!(2),
                    5 => data["client_id"] = json!(DYNAMIC_CLIENT),
                    _ => data["access_token"] = json!("forbidden-token"),
                }
                fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            }
            _ => {
                // Ownership changes are testable only when this test runs as root.
                // SAFETY: geteuid has no arguments or memory-safety preconditions.
                if unsafe { libc::geteuid() } != 0 {
                    continue;
                }
                let file = fs::File::open(&path).unwrap();
                // SAFETY: the File owns a live descriptor; gid -1 preserves its group.
                assert_eq!(
                    unsafe { libc::fchown(std::os::fd::AsRawFd::as_raw_fd(&file), 1, !0) },
                    0
                );
            }
        }
        assert!(guard.pending_registration().is_err());
        assert!(guard.retain_pending_registration(&receipt).is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
    }
}
