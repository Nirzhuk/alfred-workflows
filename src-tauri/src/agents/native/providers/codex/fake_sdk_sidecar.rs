//! Self-contained stand-in for the packaged Python SDK sidecar.
//!
//! The fixture is intentionally Rust-only: conformance does not require a
//! system Python, an installed Codex package, a Codex CLI, network, or user
//! credentials. It speaks the same bounded JSONL frames as the Python source.

use super::sdk_package::{CODEX_CLI_WHEELS, CODEX_SDK_RUNTIME_VERSION, SEALED_PACKAGE_BLOCKER};
use super::sdk_protocol::{
    self, CodexSdkAccount, CodexSdkApprovalDecision, CodexSdkCapabilities, CodexSdkEventMapper,
    CodexSdkInbound, CodexSdkLoginCancellation, CodexSdkLoginKind, CodexSdkLoginPrompt,
    CodexSdkLoginWait, CodexSdkLogout, CodexSdkMethod, CodexSdkModels, CodexSdkProtocol,
    CodexSdkProtocolErrorCode, CodexSdkStreamEvent, CodexSdkThread, CodexSdkTurn,
    CodexSdkTurnCancellation, CODEX_SDK_HOST_APPROVAL_BLOCKER, CODEX_SDK_PROTOCOL_VERSION,
    MAX_CODEX_SDK_FRAME_BYTES,
};
use super::sdk_runtime::{
    codex_sdk_native_ready, register, KNOWN_CLIENT_ENTERPRISE_BLOCKER, PACKAGED_SMOKE_BLOCKER,
    PUBLIC_CAPABILITY_AUDIT_BLOCKER,
};
use crate::agents::native::{NativeEventKind, NativeEventLimits};
use serde_json::{json, Value};

#[derive(Default)]
struct FakePythonSidecar {
    logged_in: bool,
    active_operation: Option<String>,
    crashed: bool,
}

impl FakePythonSidecar {
    fn transact(&mut self, request: &[u8]) -> Vec<Vec<u8>> {
        if self.crashed {
            return Vec::new();
        }
        let request: Value = serde_json::from_slice(request).expect("fixture request");
        let request_id = request["requestId"].as_str().expect("request id");
        let method = request["method"].as_str().expect("method");
        let params = &request["params"];
        match method {
            "capabilities" => vec![response(
                request_id,
                method,
                json!({
                    "account": true,
                    "browserLogin": true,
                    "deviceCodeLogin": true,
                    "experimentalApi": false,
                    "hostApprovalBlocker": CODEX_SDK_HOST_APPROVAL_BLOCKER,
                    "hostApprovals": false,
                    "logout": true,
                    "models": true,
                    "sdkVersion": CODEX_SDK_RUNTIME_VERSION,
                    "streamedTurns": true,
                    "threadCreate": true,
                    "threadResume": true,
                    "turnCancellation": true,
                    "usage": false
                }),
            )],
            "login_start" => {
                let kind = params["kind"].as_str().expect("login kind");
                let result = match kind {
                    "browser" => json!({
                        "authorizationUrl": "https://chatgpt.com/auth/codex",
                        "kind": "browser",
                        "loginId": "login_browser_1",
                        "userCode": null
                    }),
                    "device_code" => json!({
                        "authorizationUrl": "https://auth.openai.com/codex/device",
                        "kind": "device_code",
                        "loginId": "login_device_1",
                        "userCode": "ABCD-EFGH"
                    }),
                    _ => unreachable!("closed fixture method"),
                };
                vec![response(request_id, method, result)]
            }
            "login_wait" => {
                let login_id = params["loginId"].as_str().expect("login id");
                self.logged_in = true;
                vec![
                    response(
                        request_id,
                        method,
                        json!({"accepted": true, "loginId": login_id}),
                    ),
                    event(
                        login_id,
                        json!({"kind":"login_completed","loginId":login_id,"success":true}),
                    ),
                ]
            }
            "login_cancel" => vec![response(
                request_id,
                method,
                json!({"cancelled":true,"loginId":params["loginId"]}),
            )],
            "account" => vec![response(
                request_id,
                method,
                if self.logged_in {
                    json!({
                        "authenticated": true,
                        "authMode": "chatgpt",
                        "displayLabel": "fixture@example.invalid",
                        "planType": "fixture",
                        "requiresOpenaiAuth": true
                    })
                } else {
                    json!({"authenticated": false})
                },
            )],
            "models" => vec![response(
                request_id,
                method,
                json!({"models":[
                    {"id":"gpt-5.3-codex","isDefault":true,"label":"GPT-5.3 Codex"},
                    {"id":"gpt-5.2-codex","isDefault":false,"label":"GPT-5.2 Codex"}
                ]}),
            )],
            "thread_start" | "thread_resume" => vec![response(
                request_id,
                method,
                json!({"threadId":params.get("threadId").and_then(Value::as_str).unwrap_or("thread_1")}),
            )],
            "turn_start" => {
                let operation_id = params["operationId"].as_str().expect("operation id");
                let thread_id = params["threadId"].as_str().expect("thread id");
                self.active_operation = Some(operation_id.into());
                vec![
                    response(
                        request_id,
                        method,
                        json!({
                            "operationId": operation_id,
                            "threadId": thread_id,
                            "turnId": "turn_1"
                        }),
                    ),
                    event(
                        operation_id,
                        json!({"kind":"turn_started","threadId":thread_id,"turnId":"turn_1"}),
                    ),
                    event(
                        operation_id,
                        json!({
                            "kind":"assistant_delta",
                            "threadId":thread_id,
                            "turnId":"turn_1",
                            "text":"fixture token=sk-secret-material"
                        }),
                    ),
                    event(
                        operation_id,
                        json!({
                            "kind":"turn_completed",
                            "status":"completed",
                            "threadId":thread_id,
                            "turnId":"turn_1"
                        }),
                    ),
                ]
            }
            "turn_cancel" => {
                self.active_operation = None;
                vec![response(
                    request_id,
                    method,
                    json!({"cancelled":true,"operationId":params["operationId"]}),
                )]
            }
            "approval_decide" => vec![error(
                Some(request_id),
                CODEX_SDK_HOST_APPROVAL_BLOCKER,
                false,
            )],
            "logout" => {
                self.logged_in = false;
                vec![response(
                    request_id,
                    method,
                    json!({"loggedOut":true,"profileState":"logged_out"}),
                )]
            }
            "shutdown" => vec![response(request_id, method, json!({"closed":true}))],
            _ => vec![error(
                Some(request_id),
                "codex_sidecar_method_unsupported",
                false,
            )],
        }
    }

    fn crash(&mut self) {
        self.crashed = true;
        self.active_operation = None;
    }
}

fn response(request_id: &str, method: &str, result: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "method":method,
        "protocolVersion":CODEX_SDK_PROTOCOL_VERSION,
        "requestId":request_id,
        "result":result,
        "type":"response"
    }))
    .expect("fixture response")
}

fn event(operation_id: &str, payload: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "event":payload,
        "operationId":operation_id,
        "protocolVersion":CODEX_SDK_PROTOCOL_VERSION,
        "type":"event"
    }))
    .expect("fixture event")
}

fn error(request_id: Option<&str>, code: &str, retryable: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "code":code,
        "protocolVersion":CODEX_SDK_PROTOCOL_VERSION,
        "requestId":request_id,
        "retryable":retryable,
        "type":"error"
    }))
    .expect("fixture error")
}

fn request(
    protocol: &mut CodexSdkProtocol,
    fake: &mut FakePythonSidecar,
    request_id: &str,
    method: CodexSdkMethod,
    params: Value,
) -> Vec<Vec<u8>> {
    let frame = protocol
        .encode_request(request_id, method, params)
        .expect("encode fixture request");
    fake.transact(&frame)
}

#[test]
fn browser_and_device_login_ceremonies_are_bounded_and_token_free() {
    for (kind, expected_login_id) in [
        (CodexSdkLoginKind::Browser, "login_browser_1"),
        (CodexSdkLoginKind::DeviceCode, "login_device_1"),
    ] {
        let mut protocol = CodexSdkProtocol::default();
        let mut fake = FakePythonSidecar::default();
        let frames = request(
            &mut protocol,
            &mut fake,
            "login_start_1",
            CodexSdkMethod::LoginStart,
            sdk_protocol::login_start_params(kind),
        );
        let CodexSdkInbound::Response(response) = protocol.ingest(&frames[0]).expect("response")
        else {
            panic!("expected response")
        };
        let prompt: CodexSdkLoginPrompt = sdk_protocol::parse_result(&response).expect("prompt");
        prompt.validate().expect("safe prompt");
        assert_eq!(prompt.login_id, expected_login_id);
        let serialized = serde_json::to_string(&prompt).expect("serialize prompt");
        assert!(!serialized.contains("accessToken"));
        assert!(!serialized.contains("refreshToken"));
        assert!(!serialized.contains("apiKey"));

        let frames = request(
            &mut protocol,
            &mut fake,
            "login_wait_1",
            CodexSdkMethod::LoginWait,
            sdk_protocol::login_id_params(expected_login_id).expect("login params"),
        );
        let CodexSdkInbound::Response(wait_response) =
            protocol.ingest(&frames[0]).expect("wait response")
        else {
            panic!("expected wait response")
        };
        let wait: CodexSdkLoginWait = sdk_protocol::parse_result(&wait_response).expect("wait dto");
        wait.validate().expect("accepted wait");
        protocol
            .track_login_operation(expected_login_id)
            .expect("track login");
        assert!(matches!(
            protocol.ingest(&frames[1]).expect("login event"),
            CodexSdkInbound::Event {
                event: CodexSdkStreamEvent::LoginCompleted { success: true, .. },
                ..
            }
        ));
    }
}

#[test]
fn account_models_and_logout_results_have_no_secret_fields() {
    let mut protocol = CodexSdkProtocol::default();
    let mut fake = FakePythonSidecar {
        logged_in: true,
        ..FakePythonSidecar::default()
    };
    let account_frame = request(
        &mut protocol,
        &mut fake,
        "account_1",
        CodexSdkMethod::Account,
        sdk_protocol::empty_params(),
    )
    .remove(0);
    let CodexSdkInbound::Response(account_response) =
        protocol.ingest(&account_frame).expect("account response")
    else {
        panic!("expected account response")
    };
    let account: CodexSdkAccount =
        sdk_protocol::parse_result(&account_response).expect("account dto");
    account.validate().expect("chatgpt account");
    let raw_account = serde_json::to_string(&account).expect("account json");
    for forbidden in ["accessToken", "refreshToken", "apiKey", "credential"] {
        assert!(!raw_account.contains(forbidden));
    }

    let models_frame = request(
        &mut protocol,
        &mut fake,
        "models_1",
        CodexSdkMethod::Models,
        sdk_protocol::empty_params(),
    )
    .remove(0);
    let CodexSdkInbound::Response(models_response) =
        protocol.ingest(&models_frame).expect("models response")
    else {
        panic!("expected models response")
    };
    let models: CodexSdkModels = sdk_protocol::parse_result(&models_response).expect("models dto");
    models.validate().expect("models bounded");
    assert_eq!(models.models.len(), 2);
    assert!(models.models[0].is_default);

    let thread_frame = request(
        &mut protocol,
        &mut fake,
        "thread_start_1",
        CodexSdkMethod::ThreadStart,
        sdk_protocol::thread_start_params("/fixture/workspace", "gpt-5.3-codex")
            .expect("thread params"),
    )
    .remove(0);
    let CodexSdkInbound::Response(thread_response) =
        protocol.ingest(&thread_frame).expect("thread response")
    else {
        panic!("expected thread response")
    };
    let thread: CodexSdkThread = sdk_protocol::parse_result(&thread_response).expect("thread dto");
    thread.validate().expect("bounded thread");
    let resume_frame = request(
        &mut protocol,
        &mut fake,
        "thread_resume_1",
        CodexSdkMethod::ThreadResume,
        sdk_protocol::thread_resume_params(
            "/fixture/workspace",
            "gpt-5.3-codex",
            &thread.thread_id,
        )
        .expect("resume params"),
    )
    .remove(0);
    let CodexSdkInbound::Response(resume_response) =
        protocol.ingest(&resume_frame).expect("resume response")
    else {
        panic!("expected resume response")
    };
    let resumed: CodexSdkThread =
        sdk_protocol::parse_result(&resume_response).expect("resumed dto");
    resumed.validate().expect("bounded resumed thread");
    assert_eq!(resumed.thread_id, thread.thread_id);

    let logout_frame = request(
        &mut protocol,
        &mut fake,
        "logout_1",
        CodexSdkMethod::Logout,
        sdk_protocol::empty_params(),
    )
    .remove(0);
    let CodexSdkInbound::Response(logout_response) =
        protocol.ingest(&logout_frame).expect("logout response")
    else {
        panic!("expected logout response")
    };
    let logout: CodexSdkLogout = sdk_protocol::parse_result(&logout_response).expect("logout dto");
    logout.validate().expect("logout acknowledgement");
    assert!(!fake.logged_in);
}

#[test]
fn stream_is_normalized_redacted_and_reasoning_never_crosses() {
    let mut protocol = CodexSdkProtocol::default();
    let mut fake = FakePythonSidecar {
        logged_in: true,
        ..FakePythonSidecar::default()
    };
    let frames = request(
        &mut protocol,
        &mut fake,
        "turn_request_1",
        CodexSdkMethod::TurnStart,
        sdk_protocol::turn_start_params(
            "/fixture/workspace",
            "gpt-5.3-codex",
            "operation_1",
            "hello",
            "thread_1",
        )
        .expect("turn params"),
    );
    let CodexSdkInbound::Response(response) = protocol.ingest(&frames[0]).expect("turn response")
    else {
        panic!("expected turn response")
    };
    let turn: CodexSdkTurn = sdk_protocol::parse_result(&response).expect("turn dto");
    turn.validate().expect("bounded turn dto");
    assert_eq!(turn.operation_id, "operation_1");
    protocol
        .track_turn_operation(&turn.operation_id, &turn.thread_id, &turn.turn_id)
        .expect("track turn");
    let mut mapper = CodexSdkEventMapper::new(NativeEventLimits::default()).expect("mapper");
    let mut native = Vec::new();
    for frame in &frames[1..] {
        let CodexSdkInbound::Event { event, .. } = protocol.ingest(frame).expect("stream event")
        else {
            panic!("expected stream event")
        };
        if let Some(event) = mapper.map(event).expect("normalize event") {
            native.push(event);
        }
    }
    assert_eq!(native[0].kind, NativeEventKind::TurnStarted);
    assert_eq!(native[1].kind, NativeEventKind::AssistantDelta);
    assert!(!native[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("sk-secret-material"));
    assert_eq!(native[2].kind, NativeEventKind::TurnCompleted);
    assert_eq!(protocol.operation_len(), 0);

    protocol
        .track_turn_operation("reasoning_probe", "thread_1", "turn_1")
        .expect("track probe");
    let reasoning = event(
        "reasoning_probe",
        json!({"kind":"reasoning_delta","text":"private chain of thought"}),
    );
    assert_eq!(
        protocol.ingest(&reasoning).unwrap_err().code(),
        CodexSdkProtocolErrorCode::MalformedFrame
    );
}

#[test]
fn cancellation_is_supported_while_host_approval_is_a_stable_blocker() {
    let mut protocol = CodexSdkProtocol::default();
    let mut fake = FakePythonSidecar::default();
    let capabilities = request(
        &mut protocol,
        &mut fake,
        "capabilities_1",
        CodexSdkMethod::Capabilities,
        sdk_protocol::empty_params(),
    )
    .remove(0);
    let CodexSdkInbound::Response(response) = protocol.ingest(&capabilities).expect("capabilities")
    else {
        panic!("expected capabilities")
    };
    let capabilities: CodexSdkCapabilities =
        sdk_protocol::parse_result(&response).expect("capabilities dto");
    capabilities.validate().expect("audited capabilities");
    assert!(capabilities.turn_cancellation);
    assert!(!capabilities.host_approvals);

    let login_cancel = request(
        &mut protocol,
        &mut fake,
        "login_cancel_1",
        CodexSdkMethod::LoginCancel,
        sdk_protocol::login_id_params("login_browser_1").expect("login cancel params"),
    )
    .remove(0);
    let CodexSdkInbound::Response(login_cancel_response) = protocol
        .ingest(&login_cancel)
        .expect("login cancel response")
    else {
        panic!("expected login cancel response")
    };
    let login_cancelled: CodexSdkLoginCancellation =
        sdk_protocol::parse_result(&login_cancel_response).expect("login cancel dto");
    login_cancelled.validate().expect("login cancelled");

    let cancel = request(
        &mut protocol,
        &mut fake,
        "cancel_1",
        CodexSdkMethod::TurnCancel,
        sdk_protocol::turn_cancel_params("operation_1").expect("cancel params"),
    )
    .remove(0);
    let CodexSdkInbound::Response(cancel_response) =
        protocol.ingest(&cancel).expect("cancel response")
    else {
        panic!("expected cancel response")
    };
    let cancelled: CodexSdkTurnCancellation =
        sdk_protocol::parse_result(&cancel_response).expect("cancel dto");
    cancelled.validate().expect("turn cancelled");

    let approval = request(
        &mut protocol,
        &mut fake,
        "approval_1",
        CodexSdkMethod::ApprovalDecide,
        sdk_protocol::approval_params("approval_1", CodexSdkApprovalDecision::Deny)
            .expect("approval params"),
    )
    .remove(0);
    let CodexSdkInbound::Error(error) = protocol.ingest(&approval).expect("approval blocker")
    else {
        panic!("expected stable blocker")
    };
    assert_eq!(error.code, CODEX_SDK_HOST_APPROVAL_BLOCKER);
}

#[test]
fn malformed_oversized_and_crashed_sidecars_fail_closed_without_python_or_codex() {
    let mut protocol = CodexSdkProtocol::default();
    assert_eq!(
        protocol.ingest(b"not-json").unwrap_err().code(),
        CodexSdkProtocolErrorCode::MalformedFrame
    );
    assert_eq!(
        protocol
            .ingest(
                br#"{"type":"error","type":"response","protocolVersion":1,"requestId":"x","method":"account","result":{}}"#,
            )
            .unwrap_err()
            .code(),
        CodexSdkProtocolErrorCode::MalformedFrame
    );
    let oversized = vec![b'x'; MAX_CODEX_SDK_FRAME_BYTES + 1];
    assert_eq!(
        protocol.ingest(&oversized).unwrap_err().code(),
        CodexSdkProtocolErrorCode::OversizedFrame
    );
    let unknown = response("never_sent", "account", json!({"authenticated":false}));
    assert_eq!(
        protocol.ingest(&unknown).unwrap_err().code(),
        CodexSdkProtocolErrorCode::UnknownRequest
    );

    let mut fake = FakePythonSidecar::default();
    let pending = protocol
        .encode_request(
            "pending_1",
            CodexSdkMethod::Account,
            sdk_protocol::empty_params(),
        )
        .expect("pending request");
    fake.crash();
    assert!(fake.transact(&pending).is_empty());
    protocol.process_exited();
    assert_eq!(protocol.pending_len(), 0);
    assert_eq!(protocol.operation_len(), 0);
    // No process was spawned: this fixture cannot consult Python, Codex, PATH,
    // CODEX_HOME, a user auth.json, keyring, or the network.
}

#[test]
fn production_registration_names_every_stable_release_blocker() {
    assert!(!codex_sdk_native_ready());
    let error = register(&crate::agents::native::NativeRuntimeRegistry::default())
        .expect_err("registration must remain blocked");
    for blocker in [
        PUBLIC_CAPABILITY_AUDIT_BLOCKER,
        CODEX_SDK_HOST_APPROVAL_BLOCKER,
        KNOWN_CLIENT_ENTERPRISE_BLOCKER,
        SEALED_PACKAGE_BLOCKER,
        PACKAGED_SMOKE_BLOCKER,
    ] {
        assert!(error.message.contains(blocker));
    }
    assert_eq!(CODEX_CLI_WHEELS.len(), 8);
    assert!(CODEX_CLI_WHEELS
        .iter()
        .all(|artifact| artifact.sha256.len() == 64));
}
