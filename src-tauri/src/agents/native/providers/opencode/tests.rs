use super::*;
use crate::agents::native::{
    redact_text, NativeCancellation, NativeErrorCode, NativeEventKind, NativeRuntimeError,
};
use serde_json::{json, Value};
use std::time::Duration;

fn text_event(delta: &str) -> Value {
    json!({
        "type": "message.part.updated",
        "properties": {
            "part": {
                "id": "part_1",
                "sessionID": "session_1",
                "messageID": "message_1",
                "type": "text",
                "text": delta
            },
            "delta": delta
        }
    })
}

fn permission_reply(response: &str) -> Value {
    json!({
        "type": "permission.replied",
        "properties": {
            "sessionID": "session_1",
            "permissionID": "permission_1",
            "response": response
        }
    })
}

#[test]
fn release_freezes_version_license_platforms_and_exact_blockers() {
    let gate = native_release_gate();
    assert_eq!(gate.runtime_version, "1.18.23");
    assert_eq!(gate.license, "MIT");
    assert_eq!(gate.platforms.len(), 6);
    assert!(!gate.ready);
    assert_eq!(
        gate.blockers
            .iter()
            .map(|(code, _)| *code)
            .collect::<Vec<_>>(),
        vec![
            package::PACKAGE_GATE_CODE,
            package::ACCOUNT_GATE_CODE,
            package::TOOL_GATE_CODE,
        ]
    );
}

#[test]
fn isolated_launch_contract_has_no_path_or_global_state_fallback() {
    let root = std::env::temp_dir().join("alfred-opencode-runtime-test");
    let executable = root.join("bundle").join("opencode");
    let spec = OpenCodeLaunchSpec::new(
        &executable,
        root.join("home"),
        49152,
        "0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    assert_eq!(spec.executable(), executable);
    assert_eq!(
        spec.args(),
        ["serve", "--hostname=127.0.0.1", "--port=49152"]
    );
    let environment = spec.environment();
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_DISABLE_PROJECT_CONFIG",
        "OPENCODE_SERVER_USERNAME",
        "OPENCODE_SERVER_PASSWORD",
    ] {
        assert!(environment.contains_key(key), "missing {key}");
    }
    assert_eq!(environment["OPENCODE_DISABLE_PROJECT_CONFIG"], "true");
    assert!(environment["OPENCODE_CONFIG_CONTENT"].contains(r#""*":"deny""#));
    assert!(!environment.contains_key("HOME"));
    assert!(!environment.contains_key("PATH"));
    assert!(!format!("{spec:?}").contains("0123456789abcdef"));

    assert_eq!(
        OpenCodeLaunchSpec::new(
            "opencode",
            root.join("home"),
            49152,
            "0123456789abcdef0123456789abcdef",
        )
        .unwrap_err()
        .code,
        NativeErrorCode::InvalidRequest
    );
}

#[test]
fn upstream_identity_auth_billing_and_route_are_explicit() {
    let account = OpenCodeAccountBinding::new(
        "openrouter",
        "OpenRouter organization",
        OpenCodeAuthKind::ProviderApiKey,
    )
    .unwrap();
    let route = OpenCodeRoute::parse("openrouter/anthropic/claude-sonnet-4").unwrap();
    account.validate_route(&route).unwrap();
    assert_eq!(account.upstream_provider_id(), "openrouter");
    assert_eq!(account.billing_owner(), "OpenRouter organization");
    assert_eq!(account.auth_kind(), OpenCodeAuthKind::ProviderApiKey);
    assert_eq!(route.model_id(), "anthropic/claude-sonnet-4");
    assert_eq!(route.id(), "openrouter/anthropic/claude-sonnet-4");

    let wrong = OpenCodeRoute::parse("anthropic/claude-sonnet-4").unwrap();
    assert_eq!(
        account.validate_route(&wrong).unwrap_err().code,
        NativeErrorCode::AccountMismatch
    );
    assert_eq!(
        OpenCodeRoute::parse("claude-sonnet-4").unwrap_err().code,
        NativeErrorCode::InvalidRequest
    );
}

#[test]
fn startup_protocol_login_and_rate_limit_failures_are_typed() {
    let startup = map_http_failure(OpenCodeServerFailure::Unavailable);
    assert_eq!(startup.code, NativeErrorCode::ProviderUnavailable);
    assert!(startup.retryable);

    let protocol = map_http_failure(OpenCodeServerFailure::Protocol);
    assert_eq!(protocol.code, NativeErrorCode::InvalidEvent);
    assert!(!protocol.retryable);

    for failure in [
        OpenCodeServerFailure::Unauthorized,
        OpenCodeServerFailure::Forbidden,
    ] {
        let error = map_http_failure(failure);
        assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
        assert!(!error.retryable);
    }

    let rate_limit = map_http_failure(OpenCodeServerFailure::RateLimited);
    assert_eq!(rate_limit.code, NativeErrorCode::ProviderUnavailable);
    assert!(rate_limit.retryable);
}

#[test]
fn documented_text_event_is_bounded_and_normalized() {
    let decoded = decode_server_event(text_event("hello"), Some("session_1"), 7).unwrap();
    let OpenCodeProtocolEvent::AssistantDelta(event) = decoded else {
        panic!("expected assistant delta");
    };
    assert_eq!(event.kind, NativeEventKind::AssistantDelta);
    assert_eq!(event.sequence, 7);
    assert_eq!(event.session_id.as_deref(), Some("session_1"));
    assert_eq!(event.text.as_deref(), Some("hello"));
}

#[test]
fn malformed_oversized_reasoning_and_cross_session_events_are_rejected() {
    assert_eq!(
        decode_server_event(json!({"type": 1}), None, 1)
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidEvent
    );

    let oversized = text_event(&"x".repeat(300 * 1024));
    assert_eq!(
        decode_server_event(oversized, Some("session_1"), 1)
            .unwrap_err()
            .code,
        NativeErrorCode::EventLimitExceeded
    );

    let mut reasoning = text_event("private chain");
    reasoning["properties"]["part"]["type"] = json!("reasoning");
    assert_eq!(
        decode_server_event(reasoning, Some("session_1"), 1)
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidEvent
    );

    assert_eq!(
        decode_server_event(text_event("wrong session"), Some("session_2"), 1)
            .unwrap_err()
            .code,
        NativeErrorCode::SessionUnavailable
    );
}

#[test]
fn tool_permission_approval_and_denial_are_observable_but_not_executable_input() {
    let pending = json!({
        "type": "permission.updated",
        "properties": {
            "id": "permission_1",
            "type": "bash",
            "sessionID": "session_1",
            "messageID": "message_1",
            "callID": "call_1",
            "title": "Run a command",
            "metadata": {"command": "cat ~/.config/opencode/auth.json"},
            "time": {"created": 1}
        }
    });
    assert_eq!(
        decode_server_event(pending, Some("session_1"), 1).unwrap(),
        OpenCodeProtocolEvent::ToolPermission(OpenCodeToolPermission::Pending {
            permission_id: "permission_1".into(),
            session_id: "session_1".into(),
            permission_type: "bash".into(),
            title: "Run a command".into(),
        })
    );
    // The untyped metadata never becomes an AlfredToolRequest. Until the
    // official server can accept an Alfred-owned result, live tool execution
    // remains blocked even though allow/reject replies are observable.
    assert_eq!(
        decode_server_event(permission_reply("once"), Some("session_1"), 2).unwrap(),
        OpenCodeProtocolEvent::ToolPermission(OpenCodeToolPermission::Replied {
            permission_id: "permission_1".into(),
            session_id: "session_1".into(),
            approved: true,
        })
    );
    assert_eq!(
        decode_server_event(permission_reply("reject"), Some("session_1"), 3).unwrap(),
        OpenCodeProtocolEvent::ToolPermission(OpenCodeToolPermission::Replied {
            permission_id: "permission_1".into(),
            session_id: "session_1".into(),
            approved: false,
        })
    );
}

#[test]
fn cancellation_timeout_and_exact_session_resume_are_bounded() {
    let cancellation = NativeCancellation::new("opencode_cancel", Duration::from_secs(1)).unwrap();
    cancellation.cancel();
    assert_eq!(
        cancellation.checkpoint().unwrap_err().code,
        NativeErrorCode::Cancelled
    );

    let timeout = NativeCancellation::new("opencode_timeout", Duration::from_millis(1)).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    assert_eq!(
        timeout.checkpoint().unwrap_err().code,
        NativeErrorCode::TimedOut
    );

    let idle = json!({
        "type": "session.idle",
        "properties": {"sessionID": "session_resume"}
    });
    assert_eq!(
        decode_server_event(idle.clone(), Some("session_resume"), 1).unwrap(),
        OpenCodeProtocolEvent::SessionIdle {
            session_id: "session_resume".into()
        }
    );
    assert_eq!(
        decode_server_event(idle, Some("session_other"), 1)
            .unwrap_err()
            .code,
        NativeErrorCode::SessionUnavailable
    );
}

#[test]
fn provider_errors_and_redaction_never_echo_credentials_or_payloads() {
    let secret = "sk-opencode-secret-value";
    let malformed = json!({
        "type": "permission.updated",
        "properties": {
            "id": secret,
            "type": "bash",
            "sessionID": "session_1",
            "title": "bad\nsecret"
        }
    });
    let error = decode_server_event(malformed, Some("session_1"), 1).unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(!map_http_failure(OpenCodeServerFailure::Unauthorized)
        .to_string()
        .contains(secret));
    assert!(!redact_text(&format!("Authorization: Bearer {secret}")).contains(secret));

    let credential = crate::agents::native::NativeCredential::new(secret.to_string());
    assert_eq!(format!("{credential:?}"), "NativeCredential([REDACTED])");

    let permission = json!({
        "type": "permission.updated",
        "properties": {
            "id": "permission_1",
            "type": "bash",
            "sessionID": "session_1",
            "messageID": "message_1",
            "title": format!("Authorization: Bearer {secret}"),
            "metadata": {},
            "time": {"created": 1}
        }
    });
    let decoded = decode_server_event(permission, Some("session_1"), 1).unwrap();
    assert!(!format!("{decoded:?}").contains(secret));
}

#[test]
fn arbitrary_server_methods_are_not_decoded_or_exposed() {
    let event = json!({
        "type": "server.method.invoke",
        "properties": {"method": "auth.set", "arguments": ["anything"]}
    });
    assert_eq!(
        decode_server_event(event, None, 1).unwrap(),
        OpenCodeProtocolEvent::Ignored
    );
}

#[test]
fn error_messages_are_static_and_sanitized() {
    let errors = [
        map_http_failure(OpenCodeServerFailure::Unauthorized),
        map_http_failure(OpenCodeServerFailure::RateLimited),
        NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "OpenCode isolated server is unavailable",
            true,
        ),
    ];
    for error in errors {
        assert!(!error.message.contains("http://"));
        assert!(!error.message.contains("Authorization"));
    }
}
