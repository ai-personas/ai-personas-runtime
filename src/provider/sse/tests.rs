use super::*;
use crate::provider as p;
use serde_json::json;
use std::sync::Arc;

fn limits() -> Limits {
    Limits {
        timeout_ms: 1000,
        max_prompt_bytes: 4096,
        max_schema_bytes: 4096,
        max_schema_nodes: 1024,
        max_schema_depth: 16,
        max_request_bytes: 16384,
        max_response_bytes: 4096,
        max_stream_bytes: 2_000_000,
        max_event_bytes: 8192,
        max_events: 4096,
    }
}
fn frame(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}
fn created() -> Value {
    json!({"type":"response.created", "sequence_number":0,
        "response":{"id":"resp_synthetic", "model":"fixture-model", "status":"in_progress", "output":[]}})
}
fn message(id: &str, phase: Option<&str>, text: &str) -> Value {
    json!({"id":id,"type":"message","role":"assistant","status":"completed","phase":phase,
        "content":[{"type":"output_text","text":text}]})
}
fn completed(output: Vec<Value>) -> Value {
    json!({"type":"response.completed", "response":{"id":"resp_synthetic", "model":"fixture-model",
        "status":"completed", "output":output}})
}
fn started() -> Collector {
    let mut c = Collector::new("fixture-model", &limits());
    c.push(&frame(created())).unwrap();
    c
}
fn assert_rule(c: &Collector, rule: &str) {
    assert_eq!(c.diagnostic.as_ref().unwrap().rule, rule);
}

#[test]
fn legacy_and_explicit_final_carriers_are_supported() {
    for mode in [
        p::AccessMode::PlatformApiKey,
        p::AccessMode::SiwcOAuth,
        p::AccessMode::ChatgptOAuth,
    ] {
        for output in [
            vec![message("m1", None, "exact final")],
            vec![
                message("m1", Some("commentary"), "never a decision"),
                message("m2", Some("final_answer"), "exact final"),
            ],
        ] {
            let mut c = started().with_mode(mode);
            for byte in frame(completed(output)) {
                c.push(&[byte]).unwrap();
            }
            assert!(c.completed);
            assert_eq!(c.finish().unwrap(), "exact final");
            assert!(c.diagnostic.is_none());
        }
    }
}

#[test]
fn ambiguous_and_commentary_only_outputs_never_become_decisions() {
    for (output, rule) in [
        (
            vec![
                message("m1", Some("final_answer"), "one"),
                message("m2", Some("final_answer"), "two"),
            ],
            "ambiguous_response_carrier",
        ),
        (
            vec![
                message("m1", Some("commentary"), "one"),
                message("m2", None, "two"),
            ],
            "ambiguous_unphased_carrier",
        ),
        (
            vec![message("m1", Some("commentary"), "one")],
            "terminal_carrier_missing",
        ),
    ] {
        let mut c = started();
        assert_eq!(c.push(&frame(completed(output))), Err(Failure::Protocol));
        assert!(!c.completed);
        assert_rule(&c, rule);
        assert_eq!(c.finish(), Err(Failure::Protocol));
    }
}

#[test]
fn phase_and_delta_consistency_remain_required() {
    for (first, terminal, rule) in [
        (
            json!({"type":"response.output_item.added","output_index":0,
            "item":{"id":"m1","type":"message","role":"assistant","status":"in_progress","content":[],"phase":"commentary"}}),
            message("m1", Some("final_answer"), "exact"),
            "message_phase_changed",
        ),
        (
            json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"m1","delta":"different"}),
            message("m1", Some("final_answer"), "exact"),
            "carrier_text_mismatch",
        ),
    ] {
        let mut c = started();
        c.push(&frame(first)).unwrap();
        assert_eq!(
            c.push(&frame(completed(vec![terminal]))),
            Err(Failure::Protocol)
        );
        assert_rule(&c, rule);
    }
}

#[test]
fn phased_thin_completion_needs_complete_item_lifecycles() {
    for omit_part_done in [false, true] {
        let mut c = started().with_mode(p::AccessMode::SiwcOAuth);
        let mut sequence = 1;
        for (index, phase, text) in [(0, "commentary", "comment"), (1, "final_answer", "final")] {
            let id = format!("m{index}");
            let item = message(&id, Some(phase), text);
            let mut added = item.clone();
            added["status"] = json!("in_progress");
            added["content"] = json!([]);
            let events = vec![
                json!({"type":"response.output_item.added","output_index":index,"item":added}),
                json!({"type":"response.content_part.added","output_index":index,"content_index":0,"item_id":id,"part":{"type":"output_text","text":""}}),
                json!({"type":"response.output_text.done","output_index":index,"content_index":0,"item_id":id,"text":text}),
                json!({"type":"response.content_part.done","output_index":index,"content_index":0,"item_id":id,"part":{"type":"output_text","text":text}}),
                json!({"type":"response.output_item.done","output_index":index,"item":item}),
            ];
            for mut event in events {
                if omit_part_done && event["type"] == "response.content_part.done" {
                    continue;
                }
                event["sequence_number"] = json!(sequence);
                sequence += 1;
                c.push(&frame(event)).unwrap();
            }
        }
        let mut terminal = completed(vec![]);
        terminal["sequence_number"] = json!(sequence);
        if omit_part_done {
            assert_eq!(c.push(&frame(terminal)), Err(Failure::Protocol));
            assert_rule(&c, "thin_message_lifecycle_missing");
        } else {
            c.push(&frame(terminal)).unwrap();
            assert_eq!(c.finish().unwrap(), "final");
        }
    }
}

#[test]
fn incomplete_items_are_remote_failures_without_promoting_partial_text() {
    for kind in ["message", "reasoning"] {
        let mut item = message("m1", None, "private partial");
        item["type"] = json!(kind);
        item["status"] = json!("incomplete");
        let mut c = started();
        assert_eq!(
            c.push(&frame(
                json!({"type":"response.output_item.done","output_index":0,"item":item})
            )),
            Err(Failure::RemoteFailure)
        );
        assert_rule(
            &c,
            if kind == "message" {
                "message_incomplete"
            } else {
                "reasoning_incomplete"
            },
        );
        assert!(!c.completed);
        assert_eq!(c.finish(), Err(Failure::RemoteFailure));
        assert!(!serde_json::to_string(&c.diagnostic)
            .unwrap()
            .contains("private partial"));
    }
}

#[test]
fn remote_terminal_status_and_usage_are_checked() {
    for status in ["incomplete", "failed"] {
        let mut c = started();
        assert_eq!(
            c.push(&frame(
                json!({"type":format!("response.{status}"), "response":{
            "id":"resp_synthetic","model":"fixture-model","status":status,
            "usage":{"input_tokens":3,"output_tokens":4,"total_tokens":7}}})
            )),
            Err(Failure::RemoteFailure)
        );
        assert_rule(
            &c,
            if status == "failed" {
                "provider_failed"
            } else {
                "provider_incomplete"
            },
        );
        assert_eq!(c.usage.total_tokens, Some(7));
    }
}

#[test]
fn late_unknown_event_has_exact_position_and_no_payload_excerpt() {
    let mut c = started();
    c.push(&frame(
        json!({"type":"response.output_item.added","output_index":0,
        "item":{"id":"r1","type":"reasoning","status":"in_progress","summary":[]}}),
    ))
    .unwrap();
    for _ in 0..2810 {
        c.push(&frame(json!({"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"synthetic private text"}))).unwrap();
    }
    let bad = frame(
        json!({"type":"secret-attacker-event-value", "payload":"synthetic credential marker"}),
    );
    let mut failed = false;
    for chunk in bad.chunks(17) {
        if c.push(chunk).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    let d = c.diagnostic.as_ref().unwrap();
    assert_eq!(d.event_index, Some(2813));
    assert_eq!(d.event_type, Some("unrecognized"));
    assert_eq!(
        d.event_type_sha256,
        Some(p::digest(b"secret-attacker-event-value"))
    );
    assert_rule(&c, "unsupported_event_type");
    let safe = serde_json::to_string(d).unwrap();
    for secret in [
        "synthetic private text",
        "secret-attacker-event-value",
        "synthetic credential marker",
    ] {
        assert!(!safe.contains(secret));
    }
    assert!(safe.len() < 512);
    let original = safe;
    assert_eq!(
        c.push(&frame(completed(vec![message("m1", None, "late")]))),
        Err(Failure::Protocol)
    );
    assert_eq!(c.finish(), Err(Failure::Protocol));
    assert_eq!(
        serde_json::to_string(&c.diagnostic.as_ref().unwrap()).unwrap(),
        original
    );
}

#[test]
fn framing_and_eof_rules_are_distinguished() {
    for (bytes, rule) in [
        (
            b"event: response.completed\ndata: {not json}\n\n".as_slice(),
            "event_json_invalid",
        ),
        (
            b"event: response.completed\n\n".as_slice(),
            "frame_event_without_data",
        ),
    ] {
        let mut c = started();
        assert_eq!(c.push(bytes), Err(Failure::Protocol));
        assert_rule(&c, rule);
        assert_eq!(c.diagnostic.as_ref().unwrap().event_index, Some(2));
        assert_eq!(
            c.diagnostic.as_ref().unwrap().event_type,
            Some("response.completed")
        );
    }
    let mut c = started();
    let mut bytes = frame(completed(vec![message("m1", None, "final")]));
    bytes.pop();
    c.push(&bytes).unwrap();
    assert_eq!(c.finish(), Err(Failure::Protocol));
    assert_rule(&c, "eof_unfinished_or_uncompleted");
    assert_eq!(c.diagnostic.as_ref().unwrap().stage, "stream_eof");
    assert_eq!(c.diagnostic.as_ref().unwrap().event_index, None);
}

#[test]
fn json_limit_duplicate_key_and_trailing_event_rules_are_precise() {
    let nested = format!("data: {}0{}\n\n", "[".repeat(65), "]".repeat(65));
    for (bytes, rule) in [
        (nested.as_bytes(), "event_json_limit"),
        (
            b"data: {\"private-key\":1,\"private-key\":2}\n\n".as_slice(),
            "event_json_duplicate_key",
        ),
    ] {
        let mut c = started();
        assert_eq!(c.push(bytes), Err(Failure::Protocol));
        assert_rule(&c, rule);
        assert!(!serde_json::to_string(&c.diagnostic)
            .unwrap()
            .contains("private-key"));
    }
    let mut c = started();
    c.push(&frame(completed(vec![message("m1", None, "final")])))
        .unwrap();
    assert_eq!(
        c.push(&frame(json!({"type":"response.in_progress"}))),
        Err(Failure::Protocol)
    );
    assert_rule(&c, "event_after_completion");
    let d = c.diagnostic.as_ref().unwrap();
    assert_eq!(d.event_index, Some(3));
    assert_eq!(d.event_type, Some("response.in_progress"));
    assert_eq!(c.finish(), Err(Failure::Protocol));
}

#[test]
fn response_id_is_retained_bounded_and_debug_redacted() {
    for id in ["resp_synthetic", "not-a-supported-response-id"] {
        let mut event = created();
        event["response"]["id"] = json!(id);
        let mut c = Collector::new("fixture-model", &limits());
        c.push(&frame(event)).unwrap();
        assert_eq!(c.response_id_sha256(), Some(p::digest(id.as_bytes())));
        if id.starts_with("resp_") {
            let retained = c.retained_response_id().unwrap();
            assert_eq!(serde_json::to_value(&retained).unwrap(), id);
            assert!(!format!("{retained:?}").contains(id));
        } else {
            assert!(c.retained_response_id().is_none());
        }
    }
    for id in [
        "resp_",
        "resp_secret/with/slashes",
        &format!("resp_{}", "a".repeat(124)),
    ] {
        assert!(p::RetainedResponseId::from_wire(id).is_none());
    }
}

struct FixtureSource;
impl p::CredentialSource for FixtureSource {
    fn load(
        &self,
        _: &p::CredentialReference,
        _: &p::SecretReference,
    ) -> std::result::Result<p::CredentialBundle, Failure> {
        Ok(p::CredentialBundle {
            credential: p::Credential::ApiKey {
                key: p::Secret::new("synthetic-test-key".into()),
                account: p::Secret::new("synthetic-account".into()),
            },
            evidence_account: p::Secret::new("synthetic-account".into()),
        })
    }
}
struct FixtureTransport {
    bytes: Vec<u8>,
    fail_after_body: bool,
}
struct FixtureBody {
    bytes: std::collections::VecDeque<u8>,
    fail_after_body: bool,
}
impl p::HttpBody for FixtureBody {
    fn read(
        &mut self,
        maximum: usize,
    ) -> futures_util::future::BoxFuture<'_, std::result::Result<Option<Vec<u8>>, Failure>> {
        Box::pin(async move {
            if self.bytes.is_empty() {
                return if self.fail_after_body {
                    Err(Failure::Network)
                } else {
                    Ok(None)
                };
            }
            let count = maximum.min(37).min(self.bytes.len());
            Ok(Some(self.bytes.drain(..count).collect()))
        })
    }
}
impl p::HttpTransport for FixtureTransport {
    fn post(
        &self,
        _: p::HttpRequest,
    ) -> futures_util::future::BoxFuture<'_, std::result::Result<p::HttpResponse, Failure>> {
        let body = FixtureBody {
            bytes: self.bytes.clone().into(),
            fail_after_body: self.fail_after_body,
        };
        Box::pin(async {
            Ok(p::HttpResponse {
                status: 200,
                is_unencoded_sse: true,
                body: Box::new(body),
            })
        })
    }
}
async fn dispatch(
    bytes: Vec<u8>,
    fail_after_body: bool,
) -> std::result::Result<p::ProviderOutput, p::ProviderError> {
    dispatch_with_limits(bytes, fail_after_body, limits()).await
}
async fn dispatch_with_limits(
    bytes: Vec<u8>,
    fail_after_body: bool,
    limits: Limits,
) -> std::result::Result<p::ProviderOutput, p::ProviderError> {
    use p::ModelProvider;
    let account = p::SecretReference {
        env: "FIXTURE_ACCOUNT".into(),
    };
    let provider = p::DirectProvider::new(
        p::DirectSettings {
            mode: p::AccessMode::PlatformApiKey,
            model: "fixture-model".into(),
            effort: "high".into(),
            credentials: p::CredentialReference::ApiKey {
                key_env: p::SecretReference {
                    env: "FIXTURE_KEY".into(),
                },
                account_env: account.clone(),
            },
            evidence: p::CapabilityEvidence {
                mode: p::AccessMode::PlatformApiKey,
                model: "fixture-model".into(),
                efforts: ["high".into()].into(),
                account_env: account,
                account_sha256: p::digest(b"synthetic-account"),
                transport_profile: p::AccessMode::PlatformApiKey.profile().into(),
                catalog_evidence_sha256: "a".repeat(64),
                structured_output_evidence_sha256: Some("b".repeat(64)),
            },
            limits,
        },
        Arc::new(FixtureSource),
        Arc::new(FixtureTransport {
            bytes,
            fail_after_body,
        }),
        Arc::new(p::SystemClock),
    )
    .unwrap();
    provider
        .prepare(p::ProviderRequest {
            prompt: "synthetic input".into(),
            invocation_id: "fixture-call".into(),
            output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        })
        .unwrap()
        .dispatch(p::Cancellation::default())
        .await
}

#[tokio::test]
async fn dispatch_failure_retains_diagnostic_and_id_without_releasing_uncertainty() {
    let mut bytes = frame(created());
    bytes.extend(frame(json!({"type":"private-event-name"})));
    let p::ProviderError::Attempt { failure, receipt } = dispatch(bytes, false).await.unwrap_err()
    else {
        panic!("missing attempt receipt")
    };
    assert_eq!(failure, Failure::Protocol);
    assert_eq!(
        receipt.stream_diagnostic.as_ref().unwrap().event_index,
        Some(2)
    );
    assert_eq!(
        serde_json::to_value(&receipt.response_id).unwrap(),
        "resp_synthetic"
    );
    assert!(!receipt.response_completed);
    assert!(!receipt.stream_eof);
    assert!(receipt.remote_effects_and_charges_may_remain);
    assert!(!receipt.remote_stop_confirmed);
    assert_eq!(receipt.usage, Usage::default());
    let serialized = serde_json::to_string(&receipt).unwrap();
    for private in [
        "private-event-name",
        "synthetic-test-key",
        "synthetic input",
    ] {
        assert!(!serialized.contains(private));
    }
    assert!(serialized.len() < 8192); // Far below the existing 64 KiB metadata allowance.
}

#[tokio::test]
async fn dispatch_still_requires_clean_eof_after_completed() {
    let mut bytes = frame(created());
    bytes.extend(frame(completed(vec![
        message("m1", Some("commentary"), "discard"),
        message("m2", Some("final_answer"), "exact final"),
    ])));
    let p::ProviderError::Attempt { failure, receipt } =
        dispatch(bytes.clone(), true).await.unwrap_err()
    else {
        panic!("missing attempt receipt")
    };
    assert_eq!(failure, Failure::Network);
    assert!(receipt.response_completed);
    assert!(!receipt.stream_eof);
    let complete = dispatch(bytes, false).await.unwrap();
    assert_eq!(complete.response, "exact final");
    assert!(complete.attempt.response_completed && complete.attempt.stream_eof);
    assert!(complete.attempt.remote_effects_and_charges_may_remain);
}

#[tokio::test]
async fn unrecognized_effort_is_never_echoed_in_attempt_or_binding_errors() {
    let marker = "synthetic_credential_marker_without_whitespace";
    let mut event = created();
    event["response"]["reasoning"] = json!({"effort":marker});
    let p::ProviderError::Attempt { failure, receipt } =
        dispatch(frame(event), false).await.unwrap_err()
    else {
        panic!("missing receipt")
    };
    assert_eq!(failure, Failure::ReportedEffortMismatch);
    assert_eq!(
        receipt.stream_diagnostic.as_ref().unwrap().rule,
        "unrecognized_reported_effort"
    );
    assert!(receipt.reported_effort.is_none());
    assert_eq!(
        serde_json::to_value(&receipt.response_id).unwrap(),
        "resp_synthetic"
    );
    assert!(!serde_json::to_string(&receipt).unwrap().contains(marker));
    assert!(!format!("{receipt:?}").contains(marker));

    let mut bytes = frame(created());
    bytes.extend(frame(completed(vec![message("m1", None, "final")])));
    let mut output = dispatch(bytes, false).await.unwrap();
    let audit = output.audit.clone();
    output.attempt.reported_effort = Some(marker.into());
    output.attempt.previous_reported_effort = Some(marker.into());
    let intent = p::ProviderIntent {
        model: "fixture-model".into(),
        effort: "high".into(),
        max_prompt_bytes: 4096,
        max_request_bytes: 16384,
        max_response_bytes: 4096,
        max_stream_bytes: 2_000_000,
    };
    let error = output.bound_to(&audit, &intent).unwrap_err();
    assert!(!format!("{error:?}").contains(marker));
    let p::ProviderError::Attempt { receipt, .. } = error else {
        panic!("missing receipt")
    };
    assert!(!serde_json::to_string(&receipt).unwrap().contains(marker));
}

#[test]
fn early_response_observation_never_establishes_protocol_binding() {
    let mut event = created();
    event["response"]["model"] = json!("wrong-model");
    let mut c = Collector::new("fixture-model", &limits());
    assert_eq!(c.push(&frame(event)), Err(Failure::ReportedModelMismatch));
    assert!(c.response_id.is_none());
    assert_eq!(
        serde_json::to_value(c.retained_response_id()).unwrap(),
        "resp_synthetic"
    );
    assert_eq!(c.response_id_sha256(), Some(p::digest(b"resp_synthetic")));
    assert_eq!(
        c.push(&frame(created())),
        Err(Failure::ReportedModelMismatch)
    );
    assert_eq!(c.finish(), Err(Failure::ReportedModelMismatch));
    assert!(!c.completed);

    let mut first = created();
    first["response"]["id"] = json!("legacy-id");
    let mut c = Collector::new("fixture-model", &limits());
    c.push(&frame(first)).unwrap();
    let mut second = created();
    second["sequence_number"] = json!(1);
    assert_eq!(c.push(&frame(second)), Err(Failure::Protocol));
    assert_rule(&c, "duplicate_created");
    assert!(c.retained_response_id().is_none());
    assert_eq!(c.response_id_sha256(), Some(p::digest(b"legacy-id")));
}

#[test]
fn known_effort_reports_preserve_conflict_evidence() {
    for label in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        let mut c = Collector::new("fixture-model", &limits());
        let mut event = created();
        event["response"]["reasoning"] = json!({"effort":label});
        c.push(&frame(event)).unwrap();
        assert_eq!(c.reported_effort.as_deref(), Some(label));
    }
    let mut c = Collector::new("fixture-model", &limits());
    let mut event = created();
    event["response"]["reasoning"] = json!({"effort":"high"});
    c.push(&frame(event.clone())).unwrap();
    event["type"] = json!("response.in_progress");
    event["sequence_number"] = json!(1);
    event["response"]["reasoning"]["effort"] = json!("low");
    assert_eq!(c.push(&frame(event)), Err(Failure::ReportedEffortMismatch));
    assert_eq!(c.previous_reported_effort.as_deref(), Some("high"));
    assert_eq!(c.reported_effort.as_deref(), Some("low"));
}

#[tokio::test]
async fn transport_budget_exhaustion_has_a_safe_rule_without_fake_event_position() {
    let bytes = frame(created());
    let mut bounds = limits();
    bounds.max_stream_bytes = bytes.len();
    bounds.max_event_bytes = bytes.len();
    bounds.max_response_bytes = 1;
    let p::ProviderError::Attempt { failure, receipt } = dispatch_with_limits(bytes, false, bounds)
        .await
        .unwrap_err()
    else {
        panic!("missing receipt")
    };
    assert_eq!(failure, Failure::Limit);
    let d = receipt.stream_diagnostic.unwrap();
    assert_eq!(d.stage, "transport_read");
    assert_eq!(d.rule, "stream_budget_exhausted_before_eof");
    assert_eq!(d.field, Some("max_stream_bytes"));
    assert_eq!(d.event_index, None);
    assert!(!receipt.stream_eof);
    assert!(!receipt.response_completed);
    assert!(receipt.remote_effects_and_charges_may_remain);
}

#[test]
fn collector_limit_rules_distinguish_event_and_response_bounds() {
    let mut bounds = limits();
    bounds.max_event_bytes = 20;
    let mut c = Collector::new("fixture-model", &bounds);
    assert_eq!(c.push(&frame(created())), Err(Failure::Limit));
    assert_rule(&c, "event_bytes_limit");
    let mut c = started();
    c.limits.max_events = 1;
    assert_eq!(c.push(&frame(completed(vec![]))), Err(Failure::Limit));
    assert_rule(&c, "event_count_limit");
    let mut c = started();
    c.limits.max_response_bytes = 1;
    assert_eq!(
        c.push(&frame(completed(vec![message("m1", None, "too long")]))),
        Err(Failure::Limit)
    );
    assert_rule(&c, "message_bytes_limit");
    let mut c = started();
    assert_eq!(c.transport_limit(true), Failure::Limit);
    assert_rule(&c, "transport_chunk_exceeds_requested_bound");
}
