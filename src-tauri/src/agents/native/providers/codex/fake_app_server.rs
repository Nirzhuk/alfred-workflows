use super::protocol::{
    account_read_params, initialize_params, model_list_params, turn_interrupt_params, CodexMethod,
};
use super::transport::{CodexIncoming, CodexJsonlTransport};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

/// Deterministic fake for provider acceptance tests. It speaks only the pinned
/// wire methods and never starts a real CLI, browser, or network request.
struct FakeCodexAppServer {
    runtime_home: String,
    exited: bool,
}

impl FakeCodexAppServer {
    fn new(runtime_home: &str) -> Self {
        Self {
            runtime_home: runtime_home.into(),
            exited: false,
        }
    }

    fn respond(&self, request: &[u8]) -> Option<Vec<u8>> {
        if self.exited {
            return None;
        }
        let request: Value = serde_json::from_slice(request).ok()?;
        let id = request.get("id")?.clone();
        let result = match request.get("method")?.as_str()? {
            "initialize" => json!({
                "userAgent":"codex/0.149.1",
                "codexHome":self.runtime_home,
                "platformFamily":"unix",
                "platformOs":"macos"
            }),
            "account/read" => json!({
                "account":{"type":"chatgpt","email":"fixture@example.com","planType":"plus"},
                "requiresOpenaiAuth":true
            }),
            "model/list" => json!({"data":[{"id":"fixture-model","isDefault":true}]}),
            "account/rateLimits/read" => json!({"rateLimits":{
                "primary":{"usedPercent":10.0,"windowDurationMins":300,"resetsAt":1730947200},
                "secondary":null
            }}),
            _ => {
                return Some(
                    json!({"id":id,"error":{"code":-32601,"message":"Method not found"}})
                        .to_string()
                        .into_bytes(),
                )
            }
        };
        Some(json!({"id":id,"result":result}).to_string().into_bytes())
    }

    fn exit(&mut self) {
        self.exited = true;
    }
}

#[test]
fn fake_app_server_drives_handshake_account_models_limits_and_exit() {
    let now = Instant::now();
    let home = "/alfred/codex-home";
    let mut fake = FakeCodexAppServer::new(home);
    let mut transport = CodexJsonlTransport::default();

    let (_, initialize) = transport
        .encode_request(
            CodexMethod::Initialize,
            initialize_params("1.0.0"),
            now,
            Duration::from_secs(1),
        )
        .unwrap();
    transport
        .ingest(&fake.respond(&initialize).unwrap())
        .unwrap();
    let CodexIncoming::Response(response) = transport.pop().unwrap() else {
        panic!("initialize response")
    };
    transport
        .accept_initialize_response(&response, Path::new(home))
        .unwrap();

    for (method, params) in [
        (CodexMethod::AccountRead, account_read_params(false)),
        (CodexMethod::ModelList, model_list_params()),
        (CodexMethod::AccountRateLimitsRead, Value::Null),
    ] {
        let (_, request) = transport
            .encode_request(method, params, now, Duration::from_secs(1))
            .unwrap();
        transport.ingest(&fake.respond(&request).unwrap()).unwrap();
        assert!(matches!(transport.pop(), Some(CodexIncoming::Response(_))));
    }

    let (_, pending) = transport
        .encode_request(
            CodexMethod::TurnInterrupt,
            turn_interrupt_params("thr_fixture", "turn_fixture"),
            now,
            Duration::from_secs(1),
        )
        .unwrap();
    fake.exit();
    assert!(fake.respond(&pending).is_none());
    assert_eq!(transport.process_exited().len(), 1);
}
