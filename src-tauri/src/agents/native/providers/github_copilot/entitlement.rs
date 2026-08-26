//! Copilot account states, kept deliberately distinct from GitHub auth.
//!
//! Plan 037's hard rule: "Do not present GitHub login success as proof that a
//! Copilot seat is active." Signing in only proves [`CopilotAccountState::
//! GithubAuthenticated`]. Every stronger claim needs a signal that names
//! Copilot, and each such signal maps to exactly one state below.
//!
//! There is no documented public REST endpoint that reports an *individual*
//! user's Copilot entitlement. A successful SDK session proves a usable seat;
//! failures are classified only from bounded SDK/runtime error fields. Nothing
//! is inferred from unrelated GitHub API scopes or a successful `GET /user`.

use serde::Serialize;
use serde_json::Value;

/// Mutually exclusive account states surfaced to the native settings UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CopilotAccountState {
    /// The GitHub OAuth device flow succeeded. Says nothing about Copilot.
    GithubAuthenticated { login: String },
    /// A Copilot session started, so a seat is active for this account.
    CopilotEntitled { login: String, plan: Option<String> },
    /// Authenticated, but no Copilot seat is assigned.
    CopilotNotEntitled { login: String },
    /// An org requires SAML SSO authorization for this token before Copilot
    /// will answer. Recoverable by the user authorizing the org.
    SsoAuthorizationRequired { organization: Option<String> },
    /// An org or enterprise policy blocks Copilot for this account. Not
    /// recoverable by the user alone.
    OrganizationPolicyDenied { organization: Option<String> },
    /// Running against the user's own model-provider key. Copilot billing and
    /// usage are not GitHub's to report in this mode.
    ByokConfigured { provider: Option<String> },
    /// Quota, rate limit, or billing exhausted for the seat.
    QuotaExhausted { retry_after_seconds: Option<u64> },
    /// The token GitHub returned was revoked or has expired.
    CredentialExpired,
}

impl CopilotAccountState {
    /// True only for the one state that authorizes running a turn.
    pub fn can_run_turn(&self) -> bool {
        matches!(
            self,
            Self::CopilotEntitled { .. } | Self::ByokConfigured { .. }
        )
    }

    /// Stable code for the account row's `last_error_code`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::GithubAuthenticated { .. } => "github_authenticated",
            Self::CopilotEntitled { .. } => "copilot_entitled",
            Self::CopilotNotEntitled { .. } => "copilot_not_entitled",
            Self::SsoAuthorizationRequired { .. } => "copilot_sso_authorization_required",
            Self::OrganizationPolicyDenied { .. } => "copilot_organization_policy_denied",
            Self::ByokConfigured { .. } => "copilot_byok_configured",
            Self::QuotaExhausted { .. } => "copilot_quota_exhausted",
            Self::CredentialExpired => "copilot_credential_expired",
        }
    }
}

/// Billing and usage reporting, separate from entitlement.
///
/// A seat can be active while usage is unreportable (BYOK, or an org that hides
/// per-seat metrics), so this never collapses into the state above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopilotBillingVisibility {
    /// GitHub reported a usage/quota snapshot for this seat.
    Reported,
    /// Entitled, but GitHub exposes no per-seat usage here.
    Unavailable,
    /// BYOK: spend is with the user's own model provider.
    NotApplicableByok,
}

/// A bounded first-party Copilot failure normalized by the SDK transport.
///
/// `error_type`, `code`, and `message` correspond to the documented
/// `session.error` shape. Optional org/backoff data may only be populated when
/// the official SDK/runtime response supplies it; the adapter must not infer
/// either value from GitHub membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotSessionRejection {
    pub error_type: String,
    pub code: String,
    pub message: String,
    pub organization: Option<String>,
    pub retry_after_seconds: Option<u64>,
}

/// Maps a Copilot session rejection onto exactly one account state.
///
/// Matching is on the SDK's stable code first and only falls back to message
/// text for the phrases GitHub documents, so a reworded message degrades to
/// "not entitled" rather than silently claiming a seat.
pub fn classify_rejection(login: &str, rejection: &CopilotSessionRejection) -> CopilotAccountState {
    let code = rejection.code.to_ascii_lowercase();
    let error_type = rejection.error_type.to_ascii_lowercase();
    let message = rejection.message.to_ascii_lowercase();

    if code.contains("sso") || message.contains("saml") || message.contains("single sign-on") {
        return CopilotAccountState::SsoAuthorizationRequired {
            organization: rejection.organization.clone(),
        };
    }
    if error_type.contains("authorization")
        || code.contains("policy")
        || code.contains("forbidden_by_org")
        || message.contains("organization policy")
        || message.contains("enterprise policy")
    {
        return CopilotAccountState::OrganizationPolicyDenied {
            organization: rejection.organization.clone(),
        };
    }
    if error_type.contains("rate_limit")
        || error_type.contains("quota")
        || code.contains("rate_limit")
        || code.contains("quota")
        || code.contains("billing")
    {
        return CopilotAccountState::QuotaExhausted {
            retry_after_seconds: rejection.retry_after_seconds,
        };
    }
    if code.contains("unauthorized")
        || code.contains("bad_credentials")
        || code.contains("token_expired")
    {
        return CopilotAccountState::CredentialExpired;
    }
    // Everything else that reached a Copilot rejection means the account is
    // known to GitHub but has no usable seat.
    CopilotAccountState::CopilotNotEntitled {
        login: login.to_string(),
    }
}

/// Parses the SDK's rejection payload defensively.
///
/// Unknown or oversized fields are dropped rather than propagated, so a hostile
/// runtime cannot smuggle text into the account row.
pub fn parse_rejection(value: &Value) -> Option<CopilotSessionRejection> {
    const MAX_FIELD_BYTES: usize = 512;
    let object = value.as_object()?;
    let bounded = |key: &str| -> Option<String> {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.len() <= MAX_FIELD_BYTES)
            .map(str::to_string)
    };
    Some(CopilotSessionRejection {
        error_type: bounded("errorType")
            .or_else(|| bounded("error_type"))
            .unwrap_or_default(),
        code: bounded("errorCode")
            .or_else(|| bounded("error_code"))
            .or_else(|| bounded("code"))
            .unwrap_or_default(),
        message: bounded("message").unwrap_or_default(),
        organization: bounded("organization"),
        retry_after_seconds: object
            .get("retryAfterSeconds")
            .or_else(|| object.get("retry_after_seconds"))
            .and_then(Value::as_u64)
            .filter(|seconds| *seconds <= 86_400),
    })
}
