use super::*;

pub(super) fn observed_overload() -> crate::provider::StreamDiagnostic {
    crate::provider::StreamDiagnostic {
        stage:"event",event_index:Some(4),event_type:Some("error"),event_type_sha256:None,
        remote_error_type:Some("service_unavailable_error"),
        remote_error_code:Some("server_is_overloaded"),remote_error_type_sha256:None,
        remote_error_code_sha256:None,incomplete_reason:None,incomplete_reason_sha256:None,
        item_status:None,rule:"provider_error",field:None,
    }
}

#[test]
fn current_and_historical_explicit_overload_diagnostics_allow_recovery() {
    let mut diagnostic=observed_overload();
    assert!(temporary_overload(&diagnostic));
    diagnostic.rule="remote_or_unsupported_output";
    assert!(temporary_overload(&diagnostic));
}

#[test]
fn quota_authentication_and_unknown_or_malformed_errors_are_not_overload() {
    for error_type in [None,Some("authentication_error"),Some("rate_limit_error"),Some("unknown")] {
        let mut diagnostic=observed_overload();diagnostic.remote_error_type=error_type;
        assert!(!temporary_overload(&diagnostic));
    }
    for error_code in [None,Some("insufficient_quota"),Some("invalid_api_key"),Some("unknown")] {
        let mut diagnostic=observed_overload();diagnostic.remote_error_code=error_code;
        assert!(!temporary_overload(&diagnostic));
    }
    let mut diagnostic=observed_overload();diagnostic.rule="malformed_error";
    assert!(!temporary_overload(&diagnostic));
    diagnostic=observed_overload();diagnostic.stage="parse";
    assert!(!temporary_overload(&diagnostic));
    diagnostic=observed_overload();diagnostic.event_type=Some("response.failed");
    assert!(!temporary_overload(&diagnostic));
}
