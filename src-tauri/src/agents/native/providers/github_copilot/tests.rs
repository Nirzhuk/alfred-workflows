//! Plan 037 fixtures. No socket, no child process, no Copilot seat.

use super::auth::*;
use super::entitlement::*;
use super::events::*;
use super::transport::*;
use crate::agents::native::{NativeEventLimits, NativeEventNormalizer};
use serde_json::json;
use std::time::Duration;

// --- device flow -------------------------------------------------------

fn start_fixture() -> DeviceAuthorizationStart {
    parse_device_start(&json!({
        "device_code": "dc-secret",
        "user_code": "ABCD-1234",
        "verification_uri": "https://github.com/login/device",
        "expires_in": 900,
        "interval": 5
    }))
    .expect("valid device start")
}

#[test]
fn device_start_parses_and_clamps_ttl() {
    let start = parse_device_start(&json!({
        "device_code": "dc",
        "user_code": "AB-12",
        "verification_uri": "https://github.com/login/device",
        "expires_in": 99_999
    }))
    .expect("valid");
    assert_eq!(start.ttl, Duration::from_secs(900));
    assert_eq!(start.interval, Duration::from_secs(5));
}

#[test]
fn device_start_rejects_off_host_verification_uri() {
    assert!(parse_device_start(&json!({
        "device_code": "dc",
        "user_code": "AB-12",
        "verification_uri": "https://github.com.evil.example/login/device",
        "expires_in": 900
    }))
    .is_err());
}

#[test]
fn device_start_never_debug_prints_the_device_code() {
    let printed = format!("{:?}", start_fixture());
    assert!(!printed.contains("dc-secret"));
    assert!(printed.contains("[REDACTED]"));
}

#[test]
fn device_poll_success_denial_expiry_and_backoff() {
    let interval = Duration::from_secs(5);
    assert!(matches!(
        classify_device_poll(
            &json!({"access_token": "gho_abc123", "token_type": "bearer"}),
            interval
        ),
        DevicePollOutcome::Authorized { .. }
    ));
    assert_eq!(
        classify_device_poll(&json!({"error": "access_denied"}), interval),
        DevicePollOutcome::Denied
    );
    assert_eq!(
        classify_device_poll(&json!({"error": "expired_token"}), interval),
        DevicePollOutcome::Expired
    );
    assert_eq!(
        classify_device_poll(&json!({"error": "authorization_pending"}), interval),
        DevicePollOutcome::Pending { retry_in: interval }
    );
    assert_eq!(
        classify_device_poll(&json!({"error": "slow_down"}), interval),
        DevicePollOutcome::SlowDown {
            retry_in: Duration::from_secs(10)
        }
    );
}

#[test]
fn device_poll_rejects_classic_pat_and_non_bearer() {
    assert!(matches!(
        classify_device_poll(
            &json!({"access_token": "ghp_classic", "token_type": "bearer"}),
            Duration::from_secs(5)
        ),
        DevicePollOutcome::Malformed {
            code: "copilot_token_classic_pat_unsupported"
        }
    ));
    assert!(matches!(
        classify_device_poll(
            &json!({"access_token": "gho_abc", "token_type": "mac"}),
            Duration::from_secs(5)
        ),
        DevicePollOutcome::Malformed { .. }
    ));
}

#[test]
fn token_never_debug_prints_its_secret() {
    let token = CopilotAccessToken::parse("gho_supersecretvalue").expect("valid");
    assert_eq!(token.kind(), CopilotTokenKind::OAuthUser);
    let printed = format!("{token:?}");
    assert!(!printed.contains("supersecretvalue"));
}

#[test]
fn account_identity_match_is_case_insensitive_but_never_switches_accounts() {
    assert_eq!(verify_expected_login("OctoCat", "octocat"), Ok(()));
    assert_eq!(
        verify_expected_login("octocat", "hubot"),
        Err("copilot_account_mismatch")
    );
}

#[test]
fn logout_clears_the_provider_local_token() {
    let mut token = CopilotAccessToken::parse("gho_logout_secret").expect("valid");
    token.clear_for_logout();
    assert!(token.expose().is_empty());
}

struct ScriptedHttp {
    responses: std::sync::Mutex<Vec<serde_json::Value>>,
}

struct DeviceStartHttp;

impl DeviceFlowHttp for DeviceStartHttp {
    fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> Result<serde_json::Value, &'static str> {
        assert_eq!(url, "https://github.com/login/device/code");
        assert!(form.contains(&("client_id", "Iv1.alfred")));
        assert!(form.contains(&("scope", "read:user")));
        Ok(json!({
            "device_code": "dc-start",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }))
    }
}

#[test]
fn device_flow_start_uses_only_the_documented_github_endpoint() {
    let start = start_device_flow(&DeviceStartHttp, "Iv1.alfred").expect("start");
    assert_eq!(start.user_code, "ABCD-1234");
}

impl DeviceFlowHttp for ScriptedHttp {
    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> Result<serde_json::Value, &'static str> {
        let mut responses = self.responses.lock().expect("scripted lock");
        if responses.is_empty() {
            return Err("device_poll_malformed");
        }
        Ok(responses.remove(0))
    }
}

#[test]
fn device_flow_backs_off_then_authorizes() {
    let http = ScriptedHttp {
        responses: std::sync::Mutex::new(vec![
            json!({"error": "authorization_pending"}),
            json!({"error": "slow_down"}),
            json!({"access_token": "gho_final", "token_type": "bearer"}),
        ]),
    };
    let start = start_fixture();
    let mut slept = Vec::new();
    let outcome = run_device_flow(
        &http,
        "Iv1.client",
        &start,
        |delay| slept.push(delay),
        || Duration::ZERO,
    );
    assert!(matches!(outcome, DevicePollOutcome::Authorized { .. }));
    assert_eq!(
        slept,
        vec![
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(10)
        ]
    );
}

#[test]
fn device_flow_expires_once_the_ttl_elapses() {
    let http = ScriptedHttp {
        responses: std::sync::Mutex::new(vec![json!({"error": "authorization_pending"})]),
    };
    let start = start_fixture();
    let outcome = run_device_flow(&http, "Iv1.client", &start, |_| {}, || Duration::from_secs(901));
    assert_eq!(outcome, DevicePollOutcome::Expired);
}

// --- entitlement -------------------------------------------------------

fn rejection(code: &str, message: &str) -> CopilotSessionRejection {
    CopilotSessionRejection {
        error_type: String::new(),
        code: code.into(),
        message: message.into(),
        organization: Some("acme".into()),
        retry_after_seconds: Some(60),
    }
}

#[test]
fn github_auth_alone_never_implies_a_copilot_seat() {
    let authenticated = CopilotAccountState::GithubAuthenticated {
        login: "octocat".into(),
    };
    assert!(!authenticated.can_run_turn());
    assert_eq!(authenticated.code(), "github_authenticated");
}

#[test]
fn each_rejection_maps_to_its_own_state() {
    assert!(matches!(
        classify_rejection("octocat", &rejection("copilot_sso_required", "")),
        CopilotAccountState::SsoAuthorizationRequired { .. }
    ));
    assert!(matches!(
        classify_rejection("octocat", &rejection("", "SAML single sign-on required")),
        CopilotAccountState::SsoAuthorizationRequired { .. }
    ));
    assert!(matches!(
        classify_rejection("octocat", &rejection("forbidden_by_org_policy", "")),
        CopilotAccountState::OrganizationPolicyDenied { .. }
    ));
    assert!(matches!(
        classify_rejection("octocat", &rejection("rate_limit_exceeded", "")),
        CopilotAccountState::QuotaExhausted {
            retry_after_seconds: Some(60)
        }
    ));
    assert!(matches!(
        classify_rejection("octocat", &rejection("bad_credentials", "")),
        CopilotAccountState::CredentialExpired
    ));
    assert!(matches!(
        classify_rejection("octocat", &rejection("no_subscription", "")),
        CopilotAccountState::CopilotNotEntitled { .. }
    ));
}

#[test]
fn documented_authorization_error_type_is_org_policy_not_a_seat() {
    let mut denied = rejection("forbidden", "");
    denied.error_type = "authorization".into();
    assert!(matches!(
        classify_rejection("octocat", &denied),
        CopilotAccountState::OrganizationPolicyDenied { .. }
    ));
}

#[test]
fn only_entitled_and_byok_may_run_a_turn() {
    assert!(CopilotAccountState::CopilotEntitled {
        login: "octocat".into(),
        plan: None
    }
    .can_run_turn());
    assert!(CopilotAccountState::ByokConfigured { provider: None }.can_run_turn());
    assert!(!CopilotAccountState::CopilotNotEntitled {
        login: "octocat".into()
    }
    .can_run_turn());
    assert!(!CopilotAccountState::QuotaExhausted {
        retry_after_seconds: None
    }
    .can_run_turn());
}

#[test]
fn billing_visibility_is_separate_from_entitlement() {
    // A live seat with no readable usage is not the same as no seat.
    assert_ne!(
        CopilotBillingVisibility::Unavailable,
        CopilotBillingVisibility::NotApplicableByok
    );
    assert_ne!(
        CopilotBillingVisibility::Reported,
        CopilotBillingVisibility::Unavailable
    );
}

#[test]
fn rejection_parsing_drops_oversized_and_unknown_fields() {
    let huge = "x".repeat(4096);
    let parsed = parse_rejection(&json!({
        "errorType": "quota",
        "errorCode": "quota_exceeded",
        "message": huge,
        "retry_after_seconds": 999_999,
        "somethingElse": "ignored"
    }))
    .expect("parsed");
    assert_eq!(parsed.error_type, "quota");
    assert_eq!(parsed.code, "quota_exceeded");
    assert!(parsed.message.is_empty());
    assert_eq!(parsed.retry_after_seconds, None);
}

// --- event mapping -----------------------------------------------------

fn sdk(event_type: &str, data: serde_json::Value) -> CopilotSdkEvent {
    CopilotSdkEvent {
        event_type: event_type.into(),
        data: data.as_object().cloned().unwrap_or_default(),
    }
}

#[test]
fn reasoning_deltas_are_dropped_not_forwarded() {
    let mut mapper = CopilotEventMapper::new();
    assert_eq!(
        mapper
            .map(&sdk("assistant.reasoning_delta", json!({"delta": "thinking"})))
            .expect("mapped"),
        MappedEvent::Drop
    );
}

#[test]
fn unknown_event_types_are_dropped_rather_than_failing_the_turn() {
    let mut mapper = CopilotEventMapper::new();
    assert_eq!(
        mapper
            .map(&sdk("copilot.future_frame", json!({"x": 1})))
            .expect("mapped"),
        MappedEvent::Drop
    );
}

#[test]
fn sequences_are_strictly_increasing_across_mapped_events() {
    let mut mapper = CopilotEventMapper::new();
    let mut sequences = Vec::new();
    for event in [
        sdk("session.created", json!({"sessionId": "s1"})),
        sdk("turn.started", json!({"turnId": "t1"})),
        sdk("assistant.message_delta", json!({"delta": "hi"})),
        sdk("turn.completed", json!({})),
    ] {
        if let MappedEvent::Emit(native) = mapper.map(&event).expect("mapped") {
            sequences.push(native.sequence);
        }
    }
    assert_eq!(sequences, vec![0, 1, 2, 3]);
    assert_eq!(mapper.session_id(), Some("s1"));
}

#[test]
fn oversized_delta_is_refused_not_truncated() {
    let mut mapper = CopilotEventMapper::new();
    let error = mapper
        .map(&sdk(
            "assistant.message_delta",
            json!({"delta": "x".repeat(64 * 1024)}),
        ))
        .expect_err("must refuse");
    assert_eq!(
        error.code,
        crate::agents::native::NativeErrorCode::EventLimitExceeded
    );
}

#[test]
fn malformed_identifier_types_are_refused() {
    let mut mapper = CopilotEventMapper::new();
    assert!(mapper
        .map(&sdk("session.created", json!({"sessionId": 7})))
        .is_err());
    let mut mapper = CopilotEventMapper::new();
    assert!(mapper
        .map(&sdk("session.created", json!({"sessionId": "x".repeat(200)})))
        .is_err());
}

#[test]
fn secret_provider_identifiers_are_opaque_and_correlated_without_raw_retention() {
    let raw_identifiers = [
        "gho_identifier_leak",
        "ghu_identifier_leak",
        "github_pat_identifier_leak",
        "Authorization: Bearer identifier-leak",
        "Cookie: session=identifier-leak",
        "sk-live-identifier-leak",
    ];

    for raw in raw_identifiers {
        let mut mapper = CopilotEventMapper::new();
        let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default())
            .expect("normalizer");
        let frames = [
            sdk("session.start", json!({"sessionId": raw})),
            sdk("assistant.turn_start", json!({"turnId": raw})),
            sdk(
                "tool.execution_start",
                json!({"toolCallId": raw, "toolName": "alfred_file_read"}),
            ),
            sdk(
                "tool.execution_progress",
                json!({"toolCallId": raw, "progressMessage": "working"}),
            ),
            sdk("tool.execution_complete", json!({"toolCallId": raw})),
            sdk("permission.requested", json!({"requestId": raw})),
            sdk(
                "permission.completed",
                json!({"requestId": raw, "result": {"kind": "approved"}}),
            ),
            sdk(
                "session.idle",
                json!({"aborted": true, "sessionId": raw, "turnId": raw}),
            ),
        ];

        let raw_debug = format!("{:?}", frames[0]);
        assert!(!raw_debug.contains(raw), "raw SDK Debug retained {raw}");
        assert!(raw_debug.contains("[REDACTED]"));
        let hostile_type_debug = format!("{:?}", sdk(&format!("future-{raw}"), json!({})));
        assert!(!hostile_type_debug.contains(raw));

        let mut normalized = Vec::new();
        for frame in frames {
            let MappedEvent::Emit(event) = mapper.map(&frame).expect("mapped") else {
                panic!("expected mapped lifecycle event");
            };
            normalized.push(normalizer.normalize(event).expect("normalized"));
        }

        let session_id = normalized[0].session_id.clone().expect("session id");
        let turn_id = normalized[1].turn_id.clone().expect("turn id");
        let tool_id = normalized[2]
            .tool_call_id
            .clone()
            .expect("tool call id");
        let approval_id = normalized[5]
            .approval_id
            .clone()
            .expect("approval id");
        for safe in [&session_id, &turn_id, &tool_id, &approval_id] {
            assert!(safe.starts_with("copilot_opaque_"), "unsafe id {safe}");
            assert!(!safe.contains(raw));
        }
        assert_eq!(normalized[3].tool_call_id.as_deref(), Some(tool_id.as_str()));
        assert_eq!(normalized[4].tool_call_id.as_deref(), Some(tool_id.as_str()));
        assert_eq!(normalized[6].approval_id.as_deref(), Some(approval_id.as_str()));
        assert_eq!(normalized[7].session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(normalized[7].turn_id.as_deref(), Some(turn_id.as_str()));
        assert_eq!(mapper.session_id(), Some(session_id.as_str()));

        let response_and_history = json!({
            "events": normalized,
            "response": {"metadata": {"sessionId": mapper.session_id()}},
            "history": {"stats": {"sessionId": mapper.session_id()}},
        });
        let serialized = serde_json::to_string(&response_and_history).expect("serialize");
        assert!(!serialized.contains(raw), "serialized raw identifier {raw}");
        assert!(!serialized.contains("identifier-leak"), "{serialized}");
        assert!(serialized.contains(&session_id));
    }
}

#[test]
fn copilot_token_classes_are_redacted_in_event_text() {
    let mut mapper = CopilotEventMapper::new();
    let MappedEvent::Emit(native) = mapper
        .map(&sdk(
            "assistant.message_delta",
            json!({"delta": "token gho_abc123 and ghu_def456 done"}),
        ))
        .expect("mapped")
    else {
        panic!("expected an emitted event");
    };
    let text = native.text.expect("text");
    assert!(!text.contains("gho_abc123"), "{text}");
    assert!(!text.contains("ghu_def456"), "{text}");
    assert!(text.contains("[REDACTED]"), "{text}");
    assert!(text.contains("done"), "{text}");
}

#[test]
fn copilot_tokens_are_redacted_at_json_and_assignment_boundaries() {
    for value in [
        "{\"token\":\"gho_jsonsecret\"}",
        "COPILOT_GITHUB_TOKEN=ghu_assignmentsecret",
    ] {
        let redacted = scrub(value);
        assert!(!redacted.contains("secret"), "{redacted}");
        assert!(redacted.contains("[REDACTED]"), "{redacted}");
    }
}

#[test]
fn shared_redaction_still_applies_through_scrub() {
    assert!(!scrub("Authorization: Bearer ghp_secret").contains("ghp_secret"));
    assert!(!scrub("github_pat_fixture_secret").contains("fixture_secret"));
}

#[test]
fn approval_resolution_carries_its_decision() {
    for (approved, expected) in [(true, Some(true)), (false, Some(false))] {
        let mut mapper_inner = CopilotEventMapper::new();
        let MappedEvent::Emit(native) = mapper_inner
            .map(&sdk(
                "permission.resolved",
                json!({"requestId": "r1", "approved": approved}),
            ))
            .expect("mapped")
        else {
            panic!("expected an emitted event");
        };
        assert_eq!(native.approved, expected);
        assert_eq!(native.approval_id.as_deref(), Some("r1"));
    }
}

#[test]
fn abort_and_failure_frames_map_to_their_terminal_kinds() {
    let mut mapper = CopilotEventMapper::new();
    let MappedEvent::Emit(cancelled) = mapper.map(&sdk("turn.aborted", json!({}))).expect("mapped")
    else {
        panic!("expected an emitted event");
    };
    assert_eq!(
        cancelled.kind,
        crate::agents::native::NativeEventKind::TurnCancelled
    );

    let mut mapper = CopilotEventMapper::new();
    let MappedEvent::Emit(failed) = mapper
        .map(&sdk("turn.failed", json!({"message": "rate limit exceeded"})))
        .expect("mapped")
    else {
        panic!("expected an emitted event");
    };
    assert_eq!(
        failed.kind,
        crate::agents::native::NativeEventKind::TurnFailed
    );
    assert_eq!(failed.error.as_deref(), Some("rate limit exceeded"));
}

#[test]
fn current_sdk_event_names_and_idle_cancellation_are_mapped() {
    let mut mapper = CopilotEventMapper::new();
    for (event, expected) in [
        (
            sdk("session.start", json!({"sessionId": "s-current"})),
            crate::agents::native::NativeEventKind::SessionStarted,
        ),
        (
            sdk("assistant.turn_start", json!({"turnId": "t-current"})),
            crate::agents::native::NativeEventKind::TurnStarted,
        ),
        (
            sdk(
                "assistant.message_delta",
                json!({"messageId": "m-current", "deltaContent": "hello"}),
            ),
            crate::agents::native::NativeEventKind::AssistantDelta,
        ),
        (
            sdk(
                "tool.execution_progress",
                json!({"toolCallId": "tool-1", "progressMessage": "working"}),
            ),
            crate::agents::native::NativeEventKind::ToolProgress,
        ),
        (
            sdk("session.idle", json!({"aborted": true})),
            crate::agents::native::NativeEventKind::TurnCancelled,
        ),
    ] {
        let MappedEvent::Emit(native) = mapper.map(&event).expect("mapped") else {
            panic!("expected emitted current SDK event");
        };
        assert_eq!(native.kind, expected);
    }
}

#[test]
fn current_permission_completed_maps_allow_and_deny() {
    for (kind, expected) in [("approved", true), ("rejected", false)] {
        let mut mapper = CopilotEventMapper::new();
        let MappedEvent::Emit(native) = mapper
            .map(&sdk(
                "permission.completed",
                json!({"requestId": "permission-1", "result": {"kind": kind}}),
            ))
            .expect("mapped")
        else {
            panic!("expected permission event");
        };
        assert_eq!(native.approved, Some(expected));
    }
}

#[test]
fn runtime_startup_and_protocol_failures_keep_their_typed_codes() {
    for code in [
        crate::agents::native::NativeErrorCode::ProviderUnavailable,
        crate::agents::native::NativeErrorCode::InvalidEvent,
    ] {
        let error: crate::agents::native::NativeRuntimeError = CopilotStartError::Runtime(
            crate::agents::native::NativeRuntimeError::new(code, "fixture failure", false),
        )
        .into();
        assert_eq!(error.code, code);
    }
}

#[test]
fn oversized_tool_invocation_is_refused_before_alfred_execution() {
    let data = json!({
        "invocationId": "tool-oversized",
        "name": "alfred_file_write",
        "input": {"content": "x".repeat(70 * 1024)}
    });
    let error = super::runtime::validate_tool_payload_size(
        data.as_object().expect("tool object"),
    )
    .expect_err("must reject");
    assert_eq!(
        error.code,
        crate::agents::native::NativeErrorCode::EventLimitExceeded
    );
}

#[test]
fn every_raw_tool_request_field_rejects_secret_material_before_execution() {
    let fixtures = [
        json!({"invocationId": "gho_invocation_secret", "name": "alfred_shell"}),
        json!({"invocationId": "tool-gho_embedded_secret", "name": "alfred_shell"}),
        json!({"invocationId": "safe", "name": "ghu_tool_secret"}),
        json!({"invocationId": "safe", "name": "alfred_file_read", "path": "github_pat_path_secret"}),
        json!({"invocationId": "safe", "name": "alfred_shell", "cwd": "Authorization: Bearer cwd-secret"}),
        json!({"invocationId": "safe", "name": "alfred_file_write", "input": {"content": "gho_input_secret"}}),
        json!({"invocationId": "safe", "name": "alfred_shell", "arguments": ["ghu_argument_secret"]}),
        json!({"invocationId": "safe", "name": "alfred_file_write", "input": {"accessToken": "opaque-secret"}}),
    ];

    for fixture in fixtures {
        let error = super::runtime::reject_secret_tool_fields(
            fixture.as_object().expect("tool object"),
        )
        .expect_err("secret-bearing tool field must fail closed");
        assert_eq!(
            error.code,
            crate::agents::native::NativeErrorCode::PermissionDenied
        );
    }
}

#[test]
fn malformed_permission_request_is_refused_instead_of_left_pending() {
    for data in [json!({}), json!({"requestId": "x".repeat(200)})] {
        let error = super::runtime::required_permission_request_id(
            data.as_object().expect("permission object"),
        )
        .expect_err("must reject");
        assert_eq!(error.code, crate::agents::native::NativeErrorCode::InvalidEvent);
    }
}

#[test]
fn secret_permission_request_identifiers_fail_without_echoing_the_secret() {
    for raw in [
        "gho_request_leak",
        "ghu_request_leak",
        "github_pat_request_leak",
        "Authorization: Bearer request-leak",
        "Cookie: session=request-leak",
        "sk-live-request-leak",
    ] {
        let data = json!({"requestId": raw});
        let error = super::runtime::required_permission_request_id(
            data.as_object().expect("permission object"),
        )
        .expect_err("secret request id must fail closed");
        assert_eq!(
            error.code,
            crate::agents::native::NativeErrorCode::PermissionDenied
        );
        assert!(!format!("{error:?}").contains(raw));
        assert!(!error.message.contains(raw));
    }
}

#[test]
fn cancellation_sets_the_shared_handle_before_transport_abort() {
    use crate::agents::native::NativeAgentRuntime;
    let cancellation = crate::agents::native::NativeCancellation::new(
        "copilot-cancel-fixture",
        Duration::from_secs(30),
    )
    .expect("cancellation");
    super::runtime::GithubCopilotNativeRuntime::unlinked()
        .cancel(&cancellation)
        .expect("cancel");
    assert!(cancellation.is_cancelled());
}

// --- runtime / transport ----------------------------------------------

#[test]
fn the_unlinked_transport_fails_closed_and_never_calls_copilot() {
    let transport = UnlinkedSdkTransport;
    let error = transport.list_models().expect_err("must fail closed");
    assert_eq!(
        error.code,
        crate::agents::native::NativeErrorCode::ProviderUnavailable
    );
}

#[test]
fn account_provider_mismatch_fails_before_credential_access() {
    use crate::agents::native::{NativeAgentRuntime, NativeCredential, ResolvedNativeAccount};
    use crate::agents::{AgentProvider, OpaqueAgentAccountRef};
    let account = ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_copilot-mismatch")
            .expect("account ref"),
        provider: AgentProvider::ClaudeCode,
        credential: NativeCredential::new("not-a-copilot-credential".to_string()),
    };
    let error = super::runtime::GithubCopilotNativeRuntime::unlinked()
        .validate_account(&account)
        .expect_err("must reject");
    assert_eq!(
        error.code,
        crate::agents::native::NativeErrorCode::AccountMismatch
    );
}

#[test]
fn the_unlinked_runtime_is_not_claimed_as_alfred_managed() {
    let source = UnlinkedSdkTransport.runtime_source();
    assert!(!source.is_alfred_managed());
    assert_eq!(source.version(), PINNED_SDK_VERSION);
}

#[test]
fn an_explicit_cli_path_is_not_alfred_managed() {
    let source = CopilotRuntimeSource::ExplicitPath {
        path: std::path::PathBuf::from("/usr/local/bin/copilot"),
        version: "1.0.11".into(),
    };
    assert!(!source.is_alfred_managed());
}

#[test]
fn the_sdk_policy_disables_ambient_login_and_all_non_alfred_tools() {
    let policy = CopilotSessionPolicy::alfred_boundary();
    assert!(policy.client_mode_empty);
    assert!(!policy.use_logged_in_user);
    assert!(!policy.available_tools.is_empty());
    assert!(policy
        .available_tools
        .iter()
        .all(|tool| tool.starts_with("custom:alfred_")));
}

#[test]
fn a_copilot_rejection_becomes_an_account_error_not_a_runtime_error() {
    let error: crate::agents::native::NativeRuntimeError =
        CopilotStartError::Rejected(rejection("no_subscription", "")).into();
    assert_eq!(
        error.code,
        crate::agents::native::NativeErrorCode::AccountUnavailable
    );
}

#[test]
fn the_native_descriptor_declares_only_what_it_implements() {
    use crate::agents::native::NativeAgentRuntime;
    let descriptor = super::runtime::GithubCopilotNativeRuntime::unlinked().descriptor();
    assert_eq!(descriptor.provider, crate::agents::AgentProvider::GithubCopilot);
    assert_eq!(descriptor.runtime_id, super::runtime::RUNTIME_ID);
    let capabilities = descriptor.capabilities;
    assert!(capabilities.supports_oauth);
    assert!(capabilities.supports_tool_calls);
    assert!(capabilities.supports_approval_events);
    // Workspace work stays on the Alfred boundary, so these stay false.
    assert!(!capabilities.supports_native_shell);
    assert!(!capabilities.supports_native_filesystem);
    assert!(!capabilities.supports_usage);
    assert!(!capabilities.supports_mcp);
}

#[test]
fn the_cli_adapter_is_untouched_by_this_slice() {
    // The CLI harness keeps its own provider adapter; native mode adds a
    // separate runtime rather than replacing it.
    use crate::agents::AgentAdapter;
    assert_eq!(
        crate::agents::github_copilot::GithubCopilotAdapter.provider(),
        crate::agents::AgentProvider::GithubCopilot
    );
}
