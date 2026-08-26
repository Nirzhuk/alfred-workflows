//! Documented GitHub OAuth **device flow** for the native Copilot harness.
//!
//! Endpoints are the public, documented ones:
//! `POST https://github.com/login/device/code` and
//! `POST https://github.com/login/oauth/access_token` with
//! `grant_type=urn:ietf:params:oauth:grant-type:device_code`.
//!
//! The decision logic is pure and takes already-parsed JSON so every branch is
//! reachable from a fixture without a socket. Transport lives behind
//! [`DeviceFlowHttp`].

use serde_json::Value;
use std::time::Duration;

pub const GITHUB_AUTH_BASE: &str = "https://github.com/login";
pub const DEVICE_CODE_PATH: &str = "/device/code";
pub const ACCESS_TOKEN_PATH: &str = "/oauth/access_token";
pub const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Copilot needs a seat-scoped user token; `read:user` identifies the account
/// so entitlement can be attributed to a login rather than an opaque token.
pub const DEVICE_SCOPES: &str = "read:user";

/// GitHub's documented default poll interval when the response omits one.
const DEFAULT_INTERVAL_SECONDS: u64 = 5;
const MIN_INTERVAL_SECONDS: u64 = 1;
const MAX_INTERVAL_SECONDS: u64 = 60;
/// GitHub device codes expire in 15 minutes; refuse to hold one longer.
const MAX_DEVICE_TTL: Duration = Duration::from_secs(900);
/// A device/user code is short and opaque; anything larger is a malformed or
/// hostile response, not a code.
const MAX_CODE_BYTES: usize = 256;

/// A started device authorization. `device_code` is the secret half and never
/// reaches the UI, logs, or an event.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceAuthorizationStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: Duration,
    pub ttl: Duration,
}

impl std::fmt::Debug for DeviceAuthorizationStart {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAuthorizationStart")
            .field("device_code", &"[REDACTED]")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("interval", &self.interval)
            .field("ttl", &self.ttl)
            .finish()
    }
}

/// Every terminal and non-terminal outcome of one device-code poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePollOutcome {
    Authorized {
        token: CopilotAccessToken,
    },
    Pending {
        retry_in: Duration,
    },
    SlowDown {
        retry_in: Duration,
    },
    /// The user (or an org admin) refused the grant. Terminal.
    Denied,
    /// The device code aged out before the user finished. Terminal.
    Expired,
    /// GitHub answered with something that is not a device-flow response.
    Malformed {
        code: &'static str,
    },
}

/// Token classes the Copilot SDK documents as accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotTokenKind {
    /// `gho_` — OAuth app user-to-server token.
    OAuthUser,
    /// `ghu_` — GitHub App user access token.
    GitHubAppUser,
    /// `github_pat_` — fine-grained PAT.
    FineGrainedPat,
}

/// A validated Copilot access token. Never `Serialize`, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct CopilotAccessToken {
    token: String,
    kind: CopilotTokenKind,
}

impl std::fmt::Debug for CopilotAccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CopilotAccessToken")
            .field("kind", &self.kind)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl CopilotAccessToken {
    /// Classifies a raw token, rejecting the classes the SDK does not accept.
    ///
    /// Classic `ghp_` tokens are deprecated and documented as **not supported**;
    /// accepting one here would produce an opaque runtime failure later.
    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        let token = raw.trim();
        if token.is_empty() {
            return Err("copilot_token_empty");
        }
        if token.len() > MAX_CODE_BYTES * 4 {
            return Err("copilot_token_malformed");
        }
        let kind = if token.starts_with("gho_") {
            CopilotTokenKind::OAuthUser
        } else if token.starts_with("ghu_") {
            CopilotTokenKind::GitHubAppUser
        } else if token.starts_with("github_pat_") {
            CopilotTokenKind::FineGrainedPat
        } else if token.starts_with("ghp_") {
            return Err("copilot_token_classic_pat_unsupported");
        } else {
            return Err("copilot_token_unrecognized");
        };
        Ok(Self {
            token: token.to_string(),
            kind,
        })
    }

    pub fn kind(&self) -> CopilotTokenKind {
        self.kind
    }

    /// The only accessor that yields the secret. Callers hand it straight to
    /// the SDK child-process environment and drop it.
    pub fn expose(&self) -> &str {
        &self.token
    }

    /// Clears the live token during logout. The shared account service owns
    /// deletion of the durable credential; this clears the provider-local
    /// copy before it leaves memory.
    pub fn clear_for_logout(&mut self) {
        use zeroize::Zeroize;
        self.token.zeroize();
    }
}

impl Drop for CopilotAccessToken {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.token.zeroize();
    }
}

/// Confirms that the account returned by GitHub is the account the user chose
/// before device authorization. OAuth success is not allowed to silently
/// switch Alfred to a different GitHub identity.
pub fn verify_expected_login(expected: &str, observed: &str) -> Result<(), &'static str> {
    let expected = expected.trim();
    let observed = observed.trim();
    if expected.is_empty() || observed.is_empty() {
        return Err("copilot_account_identity_missing");
    }
    if expected.eq_ignore_ascii_case(observed) {
        Ok(())
    } else {
        Err("copilot_account_mismatch")
    }
}

/// Parses `POST /login/device/code`.
pub fn parse_device_start(body: &Value) -> Result<DeviceAuthorizationStart, &'static str> {
    let object = body.as_object().ok_or("device_start_malformed")?;
    let device_code = bounded_code(object.get("device_code"))?;
    let user_code = bounded_code(object.get("user_code"))?;
    let verification_uri = object
        .get("verification_uri")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_github_verification_uri(value))
        .ok_or("device_start_malformed")?
        .to_string();
    let expires_in = object
        .get("expires_in")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .ok_or("device_start_malformed")?;
    let interval = object
        .get("interval")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_INTERVAL_SECONDS)
        .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS);
    Ok(DeviceAuthorizationStart {
        device_code,
        user_code,
        verification_uri,
        interval: Duration::from_secs(interval),
        ttl: Duration::from_secs(expires_in).min(MAX_DEVICE_TTL),
    })
}

/// Classifies one `POST /login/oauth/access_token` response.
///
/// `current_interval` is the caller's live backoff so `slow_down` compounds the
/// way GitHub documents rather than resetting.
pub fn classify_device_poll(body: &Value, current_interval: Duration) -> DevicePollOutcome {
    let Some(object) = body.as_object() else {
        return DevicePollOutcome::Malformed {
            code: "device_poll_malformed",
        };
    };

    if let Some(raw) = object.get("access_token").and_then(Value::as_str) {
        let bearer = object
            .get("token_type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("bearer"));
        if !bearer {
            return DevicePollOutcome::Malformed {
                code: "device_poll_token_type_unsupported",
            };
        }
        return match CopilotAccessToken::parse(raw) {
            Ok(token) => DevicePollOutcome::Authorized { token },
            Err(code) => DevicePollOutcome::Malformed { code },
        };
    }

    match object.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => DevicePollOutcome::Pending {
            retry_in: current_interval,
        },
        Some("slow_down") => DevicePollOutcome::SlowDown {
            retry_in: next_backoff(object, current_interval),
        },
        Some("expired_token") => DevicePollOutcome::Expired,
        Some("access_denied") => DevicePollOutcome::Denied,
        // `incorrect_device_code` / `unsupported_grant_type` mean this attempt
        // can never succeed; treat them as malformed rather than retrying.
        Some(_) | None => DevicePollOutcome::Malformed {
            code: "device_poll_malformed",
        },
    }
}

/// GitHub may send an updated `interval` with `slow_down`; otherwise its
/// documented behaviour is to add five seconds.
fn next_backoff(object: &serde_json::Map<String, Value>, current: Duration) -> Duration {
    let seconds = object
        .get("interval")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| current.as_secs().saturating_add(5))
        .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS);
    Duration::from_secs(seconds)
}

fn bounded_code(value: Option<&Value>) -> Result<String, &'static str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty() && code.len() <= MAX_CODE_BYTES)
        .map(str::to_string)
        .ok_or("device_start_malformed")
}

/// Only GitHub's own verification host is accepted, so a tampered response
/// cannot send the user to an attacker-controlled consent page.
fn is_github_verification_uri(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https" && matches!(url.host_str(), Some("github.com") | Some("www.github.com"))
}

/// The network half, kept behind a trait so the flow above is fixture-testable.
pub trait DeviceFlowHttp: Send + Sync {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Value, &'static str>;
}

/// Starts the documented device authorization request.
pub fn start_device_flow(
    http: &dyn DeviceFlowHttp,
    client_id: &str,
) -> Result<DeviceAuthorizationStart, &'static str> {
    let client_id = client_id.trim();
    if client_id.is_empty() || client_id.len() > MAX_CODE_BYTES {
        return Err("device_client_id_malformed");
    }
    let url = format!("{GITHUB_AUTH_BASE}{DEVICE_CODE_PATH}");
    let body = http.post_form(&url, &[("client_id", client_id), ("scope", DEVICE_SCOPES)])?;
    parse_device_start(&body)
}

/// Drives the documented device flow to a terminal outcome using `http`.
///
/// `sleep` is injected so a fixture advances the clock without waiting; the
/// production caller passes a real sleep.
pub fn run_device_flow(
    http: &dyn DeviceFlowHttp,
    client_id: &str,
    start: &DeviceAuthorizationStart,
    mut sleep: impl FnMut(Duration),
    mut elapsed: impl FnMut() -> Duration,
) -> DevicePollOutcome {
    let url = format!("{GITHUB_AUTH_BASE}{ACCESS_TOKEN_PATH}");
    let mut interval = start.interval;
    loop {
        if elapsed() >= start.ttl {
            return DevicePollOutcome::Expired;
        }
        sleep(interval);
        if elapsed() >= start.ttl {
            return DevicePollOutcome::Expired;
        }
        let body = match http.post_form(
            &url,
            &[
                ("client_id", client_id),
                ("device_code", start.device_code.as_str()),
                ("grant_type", DEVICE_GRANT_TYPE),
            ],
        ) {
            Ok(body) => body,
            Err(code) => return DevicePollOutcome::Malformed { code },
        };
        match classify_device_poll(&body, interval) {
            DevicePollOutcome::Pending { retry_in } => interval = retry_in,
            DevicePollOutcome::SlowDown { retry_in } => interval = retry_in,
            terminal => return terminal,
        }
    }
}
