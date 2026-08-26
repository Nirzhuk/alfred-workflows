use serde_json::{json, Value};

/// The complete request allowlist reachable from the initial Alfred Codex
/// harness. Callers select an enum variant; workflow JSON can never supply a
/// raw method name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexMethod {
    Initialize,
    AccountRead,
    AccountLoginStart,
    AccountLoginCancel,
    AccountLogout,
    AccountRateLimitsRead,
    ModelList,
    ThreadStart,
    ThreadResume,
    TurnStart,
    TurnInterrupt,
}

impl CodexMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::AccountRead => "account/read",
            Self::AccountLoginStart => "account/login/start",
            Self::AccountLoginCancel => "account/login/cancel",
            Self::AccountLogout => "account/logout",
            Self::AccountRateLimitsRead => "account/rateLimits/read",
            Self::ModelList => "model/list",
            Self::ThreadStart => "thread/start",
            Self::ThreadResume => "thread/resume",
            Self::TurnStart => "turn/start",
            Self::TurnInterrupt => "turn/interrupt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatGptLoginKind {
    Browser,
    DeviceCode,
}

impl ChatGptLoginKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "chatgpt",
            Self::DeviceCode => "chatgptDeviceCode",
        }
    }
}

pub fn initialize_params(client_version: &str) -> Value {
    json!({
        "clientInfo": {
            "name": "alfred_desktop",
            "title": "Alfred Desktop",
            "version": client_version,
        },
        // The first release deliberately stays on documented stable fields.
        "capabilities": { "experimentalApi": false }
    })
}

pub fn account_read_params(refresh: bool) -> Value {
    json!({ "refreshToken": refresh })
}

pub fn login_start_params(kind: ChatGptLoginKind) -> Value {
    json!({ "type": kind.as_str() })
}

pub fn login_cancel_params(login_id: &str) -> Value {
    json!({ "loginId": login_id })
}

pub fn model_list_params() -> Value {
    json!({ "includeHidden": false })
}

pub fn thread_start_params(model: &str, cwd: &str) -> Value {
    json!({
        "model": model,
        "cwd": cwd,
        "ephemeral": true,
        // Never inherit a saved CLI permission escape hatch.
        "approvalPolicy": "onRequest",
        "sandbox": "workspaceWrite",
    })
}

pub fn thread_resume_params(thread_id: &str) -> Value {
    json!({ "threadId": thread_id })
}

pub fn turn_start_params(thread_id: &str, prompt: &str, model: &str) -> Value {
    json!({
        "threadId": thread_id,
        "model": model,
        "input": [{ "type": "text", "text": prompt }],
        "approvalPolicy": "onRequest",
    })
}

pub fn turn_interrupt_params(thread_id: &str, turn_id: &str) -> Value {
    json!({ "threadId": thread_id, "turnId": turn_id })
}

pub fn initialized_notification() -> Value {
    json!({ "method": "initialized" })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexNotificationMethod {
    AccountLoginCompleted,
    AccountUpdated,
    RateLimitsUpdated,
    ThreadStarted,
    TurnStarted,
    TurnCompleted,
    ItemStarted,
    ItemCompleted,
    AgentMessageDelta,
    CommandOutputDelta,
    TurnDiffUpdated,
    TokenUsageUpdated,
    ConfigWarning,
}

impl CodexNotificationMethod {
    pub fn parse(method: &str) -> Option<Self> {
        Some(match method {
            "account/login/completed" => Self::AccountLoginCompleted,
            "account/updated" => Self::AccountUpdated,
            "account/rateLimits/updated" => Self::RateLimitsUpdated,
            "thread/started" => Self::ThreadStarted,
            "turn/started" => Self::TurnStarted,
            "turn/completed" => Self::TurnCompleted,
            "item/started" => Self::ItemStarted,
            "item/completed" => Self::ItemCompleted,
            "item/agentMessage/delta" => Self::AgentMessageDelta,
            "item/commandExecution/outputDelta" => Self::CommandOutputDelta,
            "turn/diff/updated" => Self::TurnDiffUpdated,
            "thread/tokenUsage/updated" => Self::TokenUsageUpdated,
            "configWarning" => Self::ConfigWarning,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexServerRequestMethod {
    CommandApproval,
    FileChangeApproval,
}

impl CodexServerRequestMethod {
    pub fn parse(method: &str) -> Option<Self> {
        Some(match method {
            "item/commandExecution/requestApproval" => Self::CommandApproval,
            "item/fileChange/requestApproval" => Self::FileChangeApproval,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_surface_is_closed_and_uses_safe_defaults() {
        let start = thread_start_params("gpt-5.3-codex", "/workspace");
        assert_eq!(start["ephemeral"], true);
        assert_eq!(start["approvalPolicy"], "onRequest");
        assert_eq!(start["sandbox"], "workspaceWrite");
        let serialized = start.to_string();
        assert!(!serialized.contains("full-auto"));
        assert!(!serialized.contains("dangerFullAccess"));
        assert!(CodexNotificationMethod::parse("rawResponse/completed").is_none());
        assert!(CodexServerRequestMethod::parse("attestation/generate").is_none());
    }

    #[test]
    fn auth_requests_only_offer_documented_chatgpt_flows() {
        assert_eq!(
            login_start_params(ChatGptLoginKind::Browser)["type"],
            "chatgpt"
        );
        assert_eq!(
            login_start_params(ChatGptLoginKind::DeviceCode)["type"],
            "chatgptDeviceCode"
        );
    }
}
