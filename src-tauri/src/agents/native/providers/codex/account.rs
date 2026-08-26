use super::ChatGptLoginKind;
use crate::agents::native::redact_text;
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::time::{Duration, Instant};
use thiserror::Error;
use url::Url;

const MAX_ACCOUNT_LABEL_BYTES: usize = 320;
const MAX_LOGIN_ID_BYTES: usize = 128;
const MAX_USER_CODE_BYTES: usize = 64;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodexAccountError {
    #[error("Codex account response is invalid")]
    InvalidResponse,
    #[error("Codex returned an authorization URL outside the allow-list")]
    UnsafeAuthorizationUrl,
    #[error("a Codex login is already pending")]
    LoginAlreadyPending,
    #[error("Codex login completion did not match the pending attempt")]
    LoginMismatch,
    #[error("Codex login was denied")]
    LoginDenied,
    #[error("Codex login timed out")]
    LoginTimedOut,
    #[error("Codex login was cancelled")]
    LoginCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexAuthMode {
    ChatGpt,
    ApiKey,
    PersonalAccessToken,
    Other,
}

impl CodexAuthMode {
    fn parse(value: &str) -> Self {
        match value {
            "chatgpt" => Self::ChatGpt,
            "apikey" | "apiKey" => Self::ApiKey,
            "personalAccessToken" => Self::PersonalAccessToken,
            _ => Self::Other,
        }
    }
}

/// Safe account projection. Runtime-owned tokens never appear in this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountMetadata {
    pub auth_mode: CodexAuthMode,
    pub display_label: Option<String>,
    pub plan_type: Option<String>,
    pub requires_openai_auth: bool,
}

impl CodexAccountMetadata {
    pub fn from_account_read(result: &Value) -> Result<Option<Self>, CodexAccountError> {
        let object = result
            .as_object()
            .ok_or(CodexAccountError::InvalidResponse)?;
        let requires_openai_auth = object
            .get("requiresOpenaiAuth")
            .and_then(Value::as_bool)
            .ok_or(CodexAccountError::InvalidResponse)?;
        let Some(account) = object.get("account") else {
            return Ok(None);
        };
        if account.is_null() {
            return Ok(None);
        }
        let account = account
            .as_object()
            .ok_or(CodexAccountError::InvalidResponse)?;
        let auth_mode = account
            .get("type")
            .and_then(Value::as_str)
            .map(CodexAuthMode::parse)
            .ok_or(CodexAccountError::InvalidResponse)?;
        let display_label = bounded_optional(account.get("email"), MAX_ACCOUNT_LABEL_BYTES)?;
        let plan_type = bounded_optional(account.get("planType"), 128)?;
        Ok(Some(Self {
            auth_mode,
            display_label,
            plan_type,
            requires_openai_auth,
        }))
    }

    pub fn from_account_updated(params: &Value) -> Result<Option<Self>, CodexAccountError> {
        let object = params
            .as_object()
            .ok_or(CodexAccountError::InvalidResponse)?;
        let Some(auth_mode) = object.get("authMode") else {
            return Err(CodexAccountError::InvalidResponse);
        };
        if auth_mode.is_null() {
            return Ok(None);
        }
        let auth_mode = auth_mode
            .as_str()
            .map(CodexAuthMode::parse)
            .ok_or(CodexAccountError::InvalidResponse)?;
        Ok(Some(Self {
            auth_mode,
            display_label: None,
            plan_type: bounded_optional(object.get("planType"), 128)?,
            requires_openai_auth: true,
        }))
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLoginPrompt {
    pub login_id: String,
    pub kind: String,
    pub authorization_url: String,
    pub user_code: Option<String>,
}

impl fmt::Debug for CodexLoginPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexLoginPrompt")
            .field("login_id", &self.login_id)
            .field("kind", &self.kind)
            .field("authorization_url", &"[REDACTED URL]")
            .field("user_code", &self.user_code.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexLoginState {
    Idle,
    Pending {
        prompt: CodexLoginPrompt,
        deadline: Instant,
    },
    Completed,
    Denied(String),
    Cancelled,
    TimedOut,
}

pub struct CodexLoginLifecycle {
    state: CodexLoginState,
}

impl Default for CodexLoginLifecycle {
    fn default() -> Self {
        Self {
            state: CodexLoginState::Idle,
        }
    }
}

impl CodexLoginLifecycle {
    pub fn state(&self) -> &CodexLoginState {
        &self.state
    }

    pub fn start(
        &mut self,
        kind: ChatGptLoginKind,
        result: &Value,
        now: Instant,
        timeout: Duration,
    ) -> Result<CodexLoginPrompt, CodexAccountError> {
        if matches!(self.state, CodexLoginState::Pending { .. }) {
            return Err(CodexAccountError::LoginAlreadyPending);
        }
        if timeout.is_zero() {
            return Err(CodexAccountError::LoginTimedOut);
        }
        let object = result
            .as_object()
            .ok_or(CodexAccountError::InvalidResponse)?;
        let login_id = bounded_required(object.get("loginId"), MAX_LOGIN_ID_BYTES)?;
        let (authorization_url, user_code) = match kind {
            ChatGptLoginKind::Browser => {
                let url = bounded_required(object.get("authUrl"), 4096)?;
                validate_authorization_url(&url, kind)?;
                (url, None)
            }
            ChatGptLoginKind::DeviceCode => {
                let url = bounded_required(object.get("verificationUrl"), 4096)?;
                validate_authorization_url(&url, kind)?;
                let user_code = bounded_required(object.get("userCode"), MAX_USER_CODE_BYTES)?;
                (url, Some(user_code))
            }
        };
        let prompt = CodexLoginPrompt {
            login_id,
            kind: kind.as_str().to_owned(),
            authorization_url,
            user_code,
        };
        self.state = CodexLoginState::Pending {
            prompt: prompt.clone(),
            deadline: now + timeout,
        };
        Ok(prompt)
    }

    pub fn complete(&mut self, params: &Value) -> Result<(), CodexAccountError> {
        let object = params
            .as_object()
            .ok_or(CodexAccountError::InvalidResponse)?;
        let login_id = bounded_required(object.get("loginId"), MAX_LOGIN_ID_BYTES)?;
        let CodexLoginState::Pending { prompt, .. } = &self.state else {
            return Err(CodexAccountError::LoginMismatch);
        };
        if prompt.login_id != login_id {
            return Err(CodexAccountError::LoginMismatch);
        }
        let success = object
            .get("success")
            .and_then(Value::as_bool)
            .ok_or(CodexAccountError::InvalidResponse)?;
        if success {
            self.state = CodexLoginState::Completed;
            Ok(())
        } else {
            let error = object
                .get("error")
                .and_then(Value::as_str)
                .map(redact_text)
                .unwrap_or_else(|| "login denied".into());
            self.state = CodexLoginState::Denied(error);
            Err(CodexAccountError::LoginDenied)
        }
    }

    pub fn cancel(&mut self, login_id: &str) -> Result<(), CodexAccountError> {
        let CodexLoginState::Pending { prompt, .. } = &self.state else {
            return Err(CodexAccountError::LoginMismatch);
        };
        if prompt.login_id != login_id {
            return Err(CodexAccountError::LoginMismatch);
        }
        self.state = CodexLoginState::Cancelled;
        Ok(())
    }

    pub fn check_timeout(&mut self, now: Instant) -> Result<(), CodexAccountError> {
        if matches!(
            &self.state,
            CodexLoginState::Pending { deadline, .. } if *deadline <= now
        ) {
            self.state = CodexLoginState::TimedOut;
            return Err(CodexAccountError::LoginTimedOut);
        }
        Ok(())
    }
}

fn validate_authorization_url(raw: &str, kind: ChatGptLoginKind) -> Result<(), CodexAccountError> {
    let url = Url::parse(raw).map_err(|_| CodexAccountError::UnsafeAuthorizationUrl)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(CodexAccountError::UnsafeAuthorizationUrl);
    }
    let host = url
        .host_str()
        .ok_or(CodexAccountError::UnsafeAuthorizationUrl)?;
    let allowed = match kind {
        ChatGptLoginKind::Browser => matches!(host, "chatgpt.com" | "auth.openai.com"),
        ChatGptLoginKind::DeviceCode => host == "auth.openai.com" && url.path() == "/codex/device",
    };
    allowed
        .then_some(())
        .ok_or(CodexAccountError::UnsafeAuthorizationUrl)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelSummary {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

pub fn parse_models(result: &Value) -> Result<Vec<CodexModelSummary>, CodexAccountError> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(CodexAccountError::InvalidResponse)?;
    if data.len() > 256 {
        return Err(CodexAccountError::InvalidResponse);
    }
    data.iter()
        .map(|model| {
            let object = model
                .as_object()
                .ok_or(CodexAccountError::InvalidResponse)?;
            Ok(CodexModelSummary {
                id: bounded_required(object.get("id"), 256)?,
                label: object
                    .get("displayName")
                    .or_else(|| object.get("id"))
                    .map(|value| bounded_required(Some(value), 256))
                    .transpose()?
                    .ok_or(CodexAccountError::InvalidResponse)?,
                is_default: object
                    .get("isDefault")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimitWindow {
    pub used_percent: f64,
    pub window_duration_minutes: Option<u64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimits {
    pub primary: Option<CodexRateLimitWindow>,
    pub secondary: Option<CodexRateLimitWindow>,
}

pub fn parse_rate_limits(result: &Value) -> Result<CodexRateLimits, CodexAccountError> {
    let limits = result
        .get("rateLimits")
        .and_then(Value::as_object)
        .ok_or(CodexAccountError::InvalidResponse)?;
    Ok(CodexRateLimits {
        primary: parse_window(limits.get("primary"))?,
        secondary: parse_window(limits.get("secondary"))?,
    })
}

fn parse_window(value: Option<&Value>) -> Result<Option<CodexRateLimitWindow>, CodexAccountError> {
    let Some(value) = value else { return Ok(None) };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or(CodexAccountError::InvalidResponse)?;
    let used_percent = object
        .get("usedPercent")
        .and_then(Value::as_f64)
        .filter(|value| (0.0..=100.0).contains(value))
        .ok_or(CodexAccountError::InvalidResponse)?;
    Ok(Some(CodexRateLimitWindow {
        used_percent,
        window_duration_minutes: object.get("windowDurationMins").and_then(Value::as_u64),
        resets_at: object.get("resetsAt").and_then(Value::as_i64),
    }))
}

fn bounded_optional(
    value: Option<&Value>,
    max: usize,
) -> Result<Option<String>, CodexAccountError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => bounded_required(Some(value), max).map(Some),
    }
}

fn bounded_required(value: Option<&Value>, max: usize) -> Result<String, CodexAccountError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= max)
        .map(ToOwned::to_owned)
        .ok_or(CodexAccountError::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn browser_result(login_id: &str) -> Value {
        json!({
            "type": "chatgpt",
            "loginId": login_id,
            "authUrl": "https://chatgpt.com/auth?redirect_uri=http%3A%2F%2Flocalhost%3A4567%2Fauth%2Fcallback"
        })
    }

    #[test]
    fn login_denial_timeout_duplicate_cancel_and_id_mismatch_are_safe() {
        let now = Instant::now();
        let mut login = CodexLoginLifecycle::default();
        login
            .start(
                ChatGptLoginKind::Browser,
                &browser_result("login-1"),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            login
                .start(
                    ChatGptLoginKind::Browser,
                    &browser_result("login-2"),
                    now,
                    Duration::from_secs(1)
                )
                .unwrap_err(),
            CodexAccountError::LoginAlreadyPending
        );
        assert_eq!(
            login
                .complete(&json!({"loginId":"other","success":true}))
                .unwrap_err(),
            CodexAccountError::LoginMismatch
        );
        assert_eq!(
            login
                .complete(&json!({
                    "loginId":"login-1",
                    "success":false,
                    "error":"Authorization: Bearer secret"
                }))
                .unwrap_err(),
            CodexAccountError::LoginDenied
        );
        let CodexLoginState::Denied(error) = login.state() else {
            panic!("denied")
        };
        assert!(!error.contains("secret"));

        login
            .start(
                ChatGptLoginKind::Browser,
                &browser_result("login-3"),
                now,
                Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(
            login
                .check_timeout(now + Duration::from_millis(2))
                .unwrap_err(),
            CodexAccountError::LoginTimedOut
        );
        login
            .start(
                ChatGptLoginKind::Browser,
                &browser_result("login-4"),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        login.cancel("login-4").unwrap();
        assert_eq!(login.state(), &CodexLoginState::Cancelled);
        login
            .start(
                ChatGptLoginKind::Browser,
                &browser_result("login-5"),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        login
            .complete(&json!({"loginId":"login-5","success":true,"error":null}))
            .unwrap();
        assert_eq!(login.state(), &CodexLoginState::Completed);
    }

    #[test]
    fn authorization_urls_are_strictly_allowlisted() {
        let mut login = CodexLoginLifecycle::default();
        let unsafe_result = json!({
            "loginId":"login-1",
            "authUrl":"https://evil.example/auth"
        });
        assert_eq!(
            login
                .start(
                    ChatGptLoginKind::Browser,
                    &unsafe_result,
                    Instant::now(),
                    Duration::from_secs(1)
                )
                .unwrap_err(),
            CodexAccountError::UnsafeAuthorizationUrl
        );
        let device = json!({
            "loginId":"login-2",
            "verificationUrl":"https://auth.openai.com/codex/device",
            "userCode":"ABCD-1234"
        });
        assert_eq!(
            login
                .start(
                    ChatGptLoginKind::DeviceCode,
                    &device,
                    Instant::now(),
                    Duration::from_secs(1)
                )
                .unwrap()
                .user_code
                .as_deref(),
            Some("ABCD-1234")
        );
    }

    #[test]
    fn account_switch_logout_models_and_rate_limits_are_projected_without_tokens() {
        let first = CodexAccountMetadata::from_account_read(&json!({
            "account":{"type":"chatgpt","email":"first@example.com","planType":"plus"},
            "requiresOpenaiAuth":true
        }))
        .unwrap()
        .unwrap();
        let second = CodexAccountMetadata::from_account_read(&json!({
            "account":{"type":"chatgpt","email":"second@example.com","planType":"pro"},
            "requiresOpenaiAuth":true
        }))
        .unwrap()
        .unwrap();
        assert_ne!(first.display_label, second.display_label);
        assert!(CodexAccountMetadata::from_account_updated(
            &json!({"authMode":null,"planType":null})
        )
        .unwrap()
        .is_none());

        let models = parse_models(&json!({"data":[
            {"id":"gpt-5.3-codex","displayName":"GPT-5.3 Codex","isDefault":true}
        ]}))
        .unwrap();
        assert_eq!(models[0].id, "gpt-5.3-codex");
        let limits = parse_rate_limits(&json!({"rateLimits":{
            "primary":{"usedPercent":25.0,"windowDurationMins":300,"resetsAt":1730947200},
            "secondary":null
        }}))
        .unwrap();
        assert_eq!(limits.primary.unwrap().used_percent, 25.0);
        assert!(limits.secondary.is_none());
    }
}
