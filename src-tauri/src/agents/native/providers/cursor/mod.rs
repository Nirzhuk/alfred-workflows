//! Cursor Cloud Agents API v1 policy and protocol mapping.
//!
//! This module deliberately does not implement or register `NativeAgentRuntime`.
//! Cursor's official HTTP API is a viable cloud transport, but Alfred cannot
//! expose it until the shared account and request contracts can represent an
//! Alfred-managed API key and explicit repository consent. Keeping the parser
//! and boundary checks here makes those gates testable without implying that
//! the existing Cursor CLI login can authorize native execution.

use crate::agents::native::{
    redact_text, NativeContentClass, NativeErrorCode, NativeEvent, NativeEventKind, NativeModel,
    NativeRuntimeError, NativeTurnRequest, NativeUsageSnapshot, NativeUsageState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const CURSOR_PROVIDER_ID: &str = "cursor";
pub const CURSOR_CLOUD_API_BASE: &str = "https://api.cursor.com";
pub const CURSOR_CLOUD_API_VERSION: &str = "v1-public-beta-2026-08-25";
pub const CURSOR_NATIVE_GATE_CODE: &str = "cursor_native_contract_blocked";
pub const CURSOR_NATIVE_READY: bool = false;

pub const CURSOR_API_DOCS: &str = "https://cursor.com/docs/api";
pub const CURSOR_CLOUD_AGENT_DOCS: &str = "https://cursor.com/docs/cloud-agent/api/endpoints";
pub const CURSOR_SDK_DOCS: &str = "https://cursor.com/docs/sdk/typescript";
pub const CURSOR_CLI_AUTH_DOCS: &str = "https://cursor.com/docs/cli/reference/authentication";
const CURSOR_API_KEY_PREFIX: &str = "crsr_";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorNativeDecision {
    pub provider_id: &'static str,
    pub selected_surface: &'static str,
    pub auth: &'static str,
    pub billing_owner: &'static str,
    pub execution_location: &'static str,
    pub repository_requirement: &'static str,
    pub models: &'static str,
    pub usage: &'static str,
    pub packaging: &'static str,
    pub gate_code: &'static str,
    pub blocked_gates: [&'static str; 3],
}

pub fn native_decision() -> CursorNativeDecision {
    CursorNativeDecision {
        provider_id: CURSOR_PROVIDER_ID,
        selected_surface: "Cursor Cloud Agents API v1 public beta over HTTPS",
        auth: "User or service-account Cursor API key; CLI browser login is not accepted",
        billing_owner: "The Cursor user or team service account that owns the API key; Cloud Agents are charged at API pricing",
        execution_location: "Cursor-managed cloud environment, never the local Alfred workspace",
        repository_requirement: "An explicitly confirmed repository URL and starting ref accessible through Cursor's source-control integration",
        models: "GET /v1/models; omit model only when deliberately choosing Cursor's configured default",
        usage: "GET /v1/agents/{agentId}/usage returns token counts, not personal subscription quota or spend",
        packaging: "Direct HTTPS needs no Cursor CLI, IDE, SDK bridge, Node runtime, or user-installed Cursor runtime",
        gate_code: CURSOR_NATIVE_GATE_CODE,
        blocked_gates: [
            "The shared account contract has no API-key auth method or approved non-React secret-entry seam and currently advertises Cursor as runtime-managed; registering a Cursor API key under that shape would be false support.",
            "The shared native request names a local working directory but has no explicit remote repository URL/ref consent field; deriving a Git remote or uploading local data is prohibited.",
            "Cloud Agents v1 reports server-side tool activity but documents no per-tool approval callback that can satisfy Alfred's Ask policy.",
        ],
    }
}

/// Explicit local-to-cloud binding supplied by a future shared request contract.
/// No code here discovers Git remotes or reads local repository configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorRepositoryBinding {
    pub workspace_root: PathBuf,
    pub repository_url: String,
    pub starting_ref: String,
}

impl CursorRepositoryBinding {
    pub fn new(
        workspace_root: PathBuf,
        repository_url: impl Into<String>,
        starting_ref: impl Into<String>,
    ) -> Result<Self, NativeRuntimeError> {
        let repository_url = repository_url.into();
        let starting_ref = starting_ref.into();
        validate_repository_url(&repository_url)?;
        if redact_cursor_text(&repository_url) != repository_url
            || redact_cursor_text(&starting_ref) != starting_ref
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "Cursor repository binding contains secret-looking material",
                false,
            ));
        }
        if !workspace_root.is_absolute() || starting_ref.trim().is_empty() {
            return Err(workspace_mismatch());
        }
        if starting_ref.len() > 255
            || starting_ref.chars().any(char::is_whitespace)
            || starting_ref.contains("..")
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "Cursor starting ref is invalid",
                false,
            ));
        }
        Ok(Self {
            workspace_root,
            repository_url,
            starting_ref,
        })
    }
}

fn validate_repository_url(repository_url: &str) -> Result<(), NativeRuntimeError> {
    let parsed = url::Url::parse(repository_url).map_err(|_| workspace_mismatch())?;
    let is_github_https = parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none();
    let segments = parsed
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).count())
        .unwrap_or_default();
    if !is_github_https || segments != 2 {
        return Err(workspace_mismatch());
    }
    Ok(())
}

fn workspace_mismatch() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::WorkspaceDenied,
        "Cursor cloud repository does not match the explicitly confirmed workspace binding",
        false,
    )
}

/// Builds only the documented cloud request fields. It never serializes local
/// paths, workflow history, skills, environment variables, or credentials.
pub fn create_agent_payload(
    request: &NativeTurnRequest,
    binding: &CursorRepositoryBinding,
    confirmed_workspace_root: &Path,
) -> Result<Value, NativeRuntimeError> {
    request.cancellation()?.checkpoint()?;
    if request.working_directory != binding.workspace_root
        || confirmed_workspace_root != binding.workspace_root
        || request.allowed_workspace_roots.as_slice() != [binding.workspace_root.clone()]
    {
        return Err(workspace_mismatch());
    }
    if request.context.len() != 1
        || request.context[0].role != crate::agents::native::NativeContextRole::User
        || request.context[0].content != request.prompt
        || request.context[0].name.is_some()
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "Cursor cloud execution refuses local skills or workflow history without explicit data scope",
            false,
        ));
    }
    if request.prompt.is_empty()
        || request.prompt.len() > 64 * 1024
        || redact_cursor_text(&request.prompt) != request.prompt
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "Cursor prompt is empty, oversized, or contains secret-looking material",
            false,
        ));
    }

    let mut payload = json!({
        "prompt": { "text": request.prompt },
        "repos": [{
            "url": binding.repository_url,
            "startingRef": binding.starting_ref,
        }],
        "workOnCurrentBranch": false,
        "autoCreatePR": false,
        "mode": "agent",
    });
    if !request.model.trim().is_empty() && request.model != "default" {
        payload["model"] = json!({ "id": request.model });
    }
    Ok(payload)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorTransportFailure {
    Timeout,
    Cancelled,
    Network,
}

pub fn map_transport_failure(failure: CursorTransportFailure) -> NativeRuntimeError {
    match failure {
        CursorTransportFailure::Timeout => NativeRuntimeError::timed_out(),
        CursorTransportFailure::Cancelled => NativeRuntimeError::cancelled(),
        CursorTransportFailure::Network => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "Cursor Cloud Agents API is unavailable",
            true,
        ),
    }
}

/// Maps status and stable provider error code without echoing response bodies.
pub fn map_http_failure(status: u16, provider_code: Option<&str>) -> NativeRuntimeError {
    let provider_code = provider_code.unwrap_or_default();
    match status {
        401 => NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            if provider_code == "api_key_revoked" {
                "Cursor API key was revoked; reconnect the native account"
            } else {
                "Cursor API key was rejected; reconnect the native account"
            },
            false,
        ),
        403 => NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "Cursor account cannot access the requested cloud repository or operation",
            false,
        ),
        408 | 504 => NativeRuntimeError::timed_out(),
        429 => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "Cursor Cloud Agents API rate limit reached",
            true,
        ),
        404 if matches!(provider_code, "repository_not_found" | "repo_not_found") => {
            workspace_mismatch()
        }
        400 => NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "Cursor rejected the bounded cloud-agent request",
            false,
        ),
        _ => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "Cursor Cloud Agents API request failed",
            status >= 500,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsResponse {
    items: Vec<ModelItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelItem {
    id: String,
    display_name: String,
}

pub fn decode_models(status: u16, body: &[u8]) -> Result<Vec<NativeModel>, NativeRuntimeError> {
    if status != 200 {
        return Err(map_http_failure(status, decode_error_code(body).as_deref()));
    }
    let response: ModelsResponse =
        serde_json::from_slice(body).map_err(|_| malformed_response())?;
    if response.items.len() > 128 {
        return Err(malformed_response());
    }
    response
        .items
        .into_iter()
        .map(|model| {
            if model.id.is_empty()
                || model.id.len() > 128
                || !model.id.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | ':' | '/')
                })
                || model.display_name.is_empty()
                || model.display_name.len() > 256
            {
                return Err(malformed_response());
            }
            Ok(NativeModel {
                id: model.id,
                label: redact_cursor_text(&model.display_name),
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageResponse {
    total_usage: TokenUsage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
}

pub fn decode_usage(status: u16, body: &[u8]) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
    if status != 200 {
        return Err(map_http_failure(status, decode_error_code(body).as_deref()));
    }
    let response: UsageResponse = serde_json::from_slice(body).map_err(|_| malformed_response())?;
    Ok(NativeUsageSnapshot {
        state: NativeUsageState::Supported,
        input_tokens: Some(response.total_usage.input_tokens),
        output_tokens: Some(response.total_usage.output_tokens),
        // The Cloud Agents endpoint documents token counts, not a quota window.
        window_resets_at: None,
    })
}

fn malformed_response() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "Cursor Cloud Agents API returned an invalid bounded response",
        false,
    )
}

fn decode_error_code(body: &[u8]) -> Option<String> {
    if body.len() > 8 * 1024 {
        return None;
    }
    serde_json::from_slice::<Value>(body)
        .ok()?
        .get("code")?
        .as_str()
        .filter(|code| code.len() <= 128)
        .map(str::to_owned)
}

pub fn cancel_endpoint(agent_id: &str, run_id: &str) -> Result<String, NativeRuntimeError> {
    validate_provider_id(agent_id, "bc-")?;
    validate_provider_id(run_id, "run-")?;
    Ok(format!(
        "{CURSOR_CLOUD_API_BASE}/v1/agents/{agent_id}/runs/{run_id}/cancel"
    ))
}

fn validate_provider_id(value: &str, prefix: &str) -> Result<(), NativeRuntimeError> {
    if value.starts_with(prefix)
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "Cursor cloud identifier is invalid",
            false,
        ))
    }
}

/// Maps the documented simplified SSE events. `thinking` is intentionally
/// replaced with a warning because Alfred prohibits reasoning content.
pub fn map_stream_event(
    kind: &str,
    data: &[u8],
) -> Result<Option<NativeEvent>, NativeRuntimeError> {
    if data.len() > 128 * 1024 {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "Cursor stream event exceeded the provider boundary",
            false,
        ));
    }
    let payload = if matches!(kind, "heartbeat" | "done") {
        Value::Null
    } else {
        serde_json::from_slice(data).map_err(|_| malformed_response())?
    };
    match kind {
        "heartbeat" | "done" => Ok(None),
        "thinking" => {
            let mut event = NativeEvent::new(0, NativeEventKind::Warning);
            event.text = Some("Cursor thinking stream omitted by Alfred's event contract.".into());
            Ok(Some(event))
        }
        "assistant" => {
            let text = redact_cursor_text(&required_string(&payload, "text", 64 * 1024)?);
            let mut event = NativeEvent::new(0, NativeEventKind::AssistantDelta);
            event.content_class = Some(NativeContentClass::Assistant);
            event.text = Some(text);
            Ok(Some(event))
        }
        "status" => map_status(&payload),
        "tool_call" => map_tool_call(&payload),
        "result" => map_result(&payload),
        "error" => {
            let message = required_string(&payload, "message", 8 * 1024)?;
            let code = optional_string(&payload, "code", 128)?;
            let mut event = NativeEvent::new(0, NativeEventKind::TurnFailed);
            event.error = Some(if code.as_deref() == Some("tool_failed") {
                "Cursor cloud tool failed".into()
            } else {
                redact_cursor_text(&message)
            });
            Ok(Some(event))
        }
        // Rich SDK-shape events are ignored while simplified events are used,
        // as required by the API documentation to prevent duplicate output.
        "interaction_update" => Ok(None),
        _ => Err(malformed_response()),
    }
}

fn map_status(payload: &Value) -> Result<Option<NativeEvent>, NativeRuntimeError> {
    let status = required_string(payload, "status", 64)?;
    let event = match status.as_str() {
        "CREATING" | "RUNNING" => NativeEvent::new(0, NativeEventKind::TurnStarted),
        "CANCELLED" => NativeEvent::new(0, NativeEventKind::TurnCancelled),
        "ERROR" | "EXPIRED" => {
            let mut event = NativeEvent::new(0, NativeEventKind::TurnFailed);
            event.error = Some("Cursor cloud run failed".into());
            event
        }
        "FINISHED" => NativeEvent::new(0, NativeEventKind::TurnCompleted),
        _ => return Err(malformed_response()),
    };
    Ok(Some(event))
}

fn map_tool_call(payload: &Value) -> Result<Option<NativeEvent>, NativeRuntimeError> {
    let call_id = required_string(payload, "callId", 128)?;
    let name = required_string(payload, "name", 128)?;
    let status = required_string(payload, "status", 64)?;
    let mut event = match status.as_str() {
        "running" | "started" => NativeEvent::new(0, NativeEventKind::ToolStarted),
        "completed" | "error" | "failed" => NativeEvent::new(0, NativeEventKind::ToolCompleted),
        _ => return Err(malformed_response()),
    };
    event.tool_call_id = Some(redact_cursor_text(&call_id));
    event.tool_name = Some(redact_cursor_text(&name));
    if matches!(status.as_str(), "completed" | "error" | "failed") {
        let output = payload
            .get("result")
            .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".into()))
            .unwrap_or_default();
        event.tool_output = Some(redact_cursor_text(&output));
    }
    Ok(Some(event))
}

fn map_result(payload: &Value) -> Result<Option<NativeEvent>, NativeRuntimeError> {
    let status = required_string(payload, "status", 64)?;
    match status.as_str() {
        "FINISHED" => {
            let mut event = NativeEvent::new(0, NativeEventKind::TurnCompleted);
            if let Some(git) = payload.get("git") {
                event.metadata = bounded_git_metadata(git)?;
            }
            Ok(Some(event))
        }
        "CANCELLED" => Ok(Some(NativeEvent::new(0, NativeEventKind::TurnCancelled))),
        "ERROR" | "EXPIRED" => {
            let mut event = NativeEvent::new(0, NativeEventKind::TurnFailed);
            event.error = Some("Cursor cloud run failed".into());
            Ok(Some(event))
        }
        _ => Err(malformed_response()),
    }
}

fn bounded_git_metadata(git: &Value) -> Result<Map<String, Value>, NativeRuntimeError> {
    let encoded = serde_json::to_vec(git).map_err(|_| malformed_response())?;
    if encoded.len() > 8 * 1024 {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "Cursor changed-file metadata exceeded the provider boundary",
            false,
        ));
    }
    let branches = git
        .get("branches")
        .and_then(Value::as_array)
        .ok_or_else(malformed_response)?;
    if branches.len() > 20 {
        return Err(malformed_response());
    }
    let safe_branches = branches
        .iter()
        .map(|branch| {
            let repo_url = required_string(branch, "repoUrl", 2_048)?;
            let mut safe = Map::new();
            safe.insert(
                "repoUrl".into(),
                Value::String(redact_cursor_text(&repo_url)),
            );
            for key in ["branch", "prUrl"] {
                if let Some(value) = optional_string(branch, key, 2_048)? {
                    safe.insert(key.into(), Value::String(redact_cursor_text(&value)));
                }
            }
            Ok(Value::Object(safe))
        })
        .collect::<Result<Vec<_>, NativeRuntimeError>>()?;
    let mut metadata = Map::new();
    metadata.insert("executionLocation".into(), Value::String("cloud".into()));
    metadata.insert("gitBranches".into(), Value::Array(safe_branches));
    Ok(metadata)
}

/// Cursor currently documents API keys with the `crsr_` prefix. Provider text
/// is scrubbed here before the provider-neutral redactor gets a second pass.
pub fn redact_cursor_text(value: &str) -> String {
    let value = redact_text(value);
    let lower = value.to_ascii_lowercase();
    let mut spans = Vec::new();
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find(CURSOR_API_KEY_PREFIX) {
        let start = search + offset;
        let at_boundary = start == 0
            || value[..start].chars().next_back().is_some_and(|character| {
                character.is_whitespace()
                    || matches!(
                        character,
                        '"' | '\''
                            | ','
                            | ';'
                            | ':'
                            | '('
                            | ')'
                            | '{'
                            | '}'
                            | '['
                            | ']'
                            | '='
                            | '<'
                            | '>'
                    )
            });
        let end = value[start..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ',' | ';' | '"' | '\'' | ')' | '}')
            })
            .map(|offset| start + offset)
            .unwrap_or(value.len());
        if at_boundary && end > start + CURSOR_API_KEY_PREFIX.len() {
            spans.push((start, end));
        }
        search = start + CURSOR_API_KEY_PREFIX.len();
        if search >= value.len() {
            break;
        }
    }
    if spans.is_empty() {
        return value;
    }
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    for (start, end) in spans {
        output.push_str(&value[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn required_string(payload: &Value, key: &str, max: usize) -> Result<String, NativeRuntimeError> {
    optional_string(payload, key, max)?.ok_or_else(malformed_response)
}

fn optional_string(
    payload: &Value,
    key: &str,
    max: usize,
) -> Result<Option<String>, NativeRuntimeError> {
    let Some(value) = payload.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(malformed_response)?;
    if value.is_empty() || value.len() > max {
        return Err(malformed_response());
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
mod fixtures;
