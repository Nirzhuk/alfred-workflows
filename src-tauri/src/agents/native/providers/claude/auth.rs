//! Safe interpretation of the documented `claude auth status` JSON surface.
//!
//! The parser intentionally has no access to Claude's profile files, keychain,
//! OAuth tokens, browser redirects, or authorization codes. It classifies only
//! bounded process output from the exact managed binary.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_AUTH_STATUS_BYTES: usize = 64 * 1024;
pub const MAX_AUTH_IDENTITY_BYTES: usize = 512;
pub const API_KEY_PRECEDENCE_WARNING_CODE: &str = "claude_api_key_overrides_subscription";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAuthMethod {
    NotAuthenticated,
    ClaudeAccount,
    AnthropicConsole,
    ThirdPartyPlatform,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeBillingSource {
    NotAuthenticated,
    ClaudeSubscription,
    EnvironmentApiKey,
    AnthropicConsoleApi,
    ThirdPartyPlatform,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeApiProvider {
    FirstParty,
    AmazonBedrock,
    GoogleVertex,
    MicrosoftFoundry,
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAuthIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_name: Option<String>,
}

impl fmt::Debug for ClaudeAuthIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClaudeAuthIdentity([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAuthStatus {
    pub logged_in: bool,
    pub auth_method: ClaudeAuthMethod,
    pub api_provider: ClaudeApiProvider,
    pub billing_source: ClaudeBillingSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<ClaudeAuthIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_warning_code: Option<&'static str>,
}

impl ClaudeAuthStatus {
    pub fn is_subscription_billed(&self) -> bool {
        self.billing_source == ClaudeBillingSource::ClaudeSubscription
    }

    pub fn api_key_takes_precedence(&self) -> bool {
        self.billing_source == ClaudeBillingSource::EnvironmentApiKey
    }
}

impl fmt::Debug for ClaudeAuthStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeAuthStatus")
            .field("logged_in", &self.logged_in)
            .field("auth_method", &self.auth_method)
            .field("api_provider", &self.api_provider)
            .field("billing_source", &self.billing_source)
            .field("subscription_type", &self.subscription_type)
            .field("has_identity", &self.identity.is_some())
            .field("billing_warning_code", &self.billing_warning_code)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAuthStatusError {
    OutputEmpty,
    OutputTooLarge,
    OutputInvalid,
    IdentityInvalid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAuthStatus {
    logged_in: bool,
    #[serde(default)]
    auth_method: Option<String>,
    #[serde(default)]
    api_provider: Option<String>,
    #[serde(default)]
    api_key_source: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    org_name: Option<String>,
    #[serde(default)]
    subscription_type: Option<String>,
}

pub fn parse_auth_status(output: &[u8]) -> Result<ClaudeAuthStatus, ClaudeAuthStatusError> {
    if output.is_empty() {
        return Err(ClaudeAuthStatusError::OutputEmpty);
    }
    if output.len() > MAX_AUTH_STATUS_BYTES {
        return Err(ClaudeAuthStatusError::OutputTooLarge);
    }
    let raw: RawAuthStatus =
        serde_json::from_slice(output).map_err(|_| ClaudeAuthStatusError::OutputInvalid)?;
    let provider = classify_provider(raw.api_provider.as_deref());
    let environment_api_key = raw
        .api_key_source
        .as_deref()
        .is_some_and(|source| source.eq_ignore_ascii_case("ANTHROPIC_API_KEY"));
    let auth_method = classify_auth_method(raw.logged_in, raw.auth_method.as_deref(), provider);
    let billing_source = if !raw.logged_in {
        ClaudeBillingSource::NotAuthenticated
    } else if environment_api_key {
        // Official precedence: ANTHROPIC_API_KEY overrides a logged-in Claude
        // subscription. This branch must precede every OAuth classification.
        ClaudeBillingSource::EnvironmentApiKey
    } else if provider != ClaudeApiProvider::FirstParty && provider != ClaudeApiProvider::Unknown {
        ClaudeBillingSource::ThirdPartyPlatform
    } else {
        match auth_method {
            ClaudeAuthMethod::ClaudeAccount => ClaudeBillingSource::ClaudeSubscription,
            ClaudeAuthMethod::AnthropicConsole => ClaudeBillingSource::AnthropicConsoleApi,
            ClaudeAuthMethod::ThirdPartyPlatform => ClaudeBillingSource::ThirdPartyPlatform,
            ClaudeAuthMethod::NotAuthenticated => ClaudeBillingSource::NotAuthenticated,
            ClaudeAuthMethod::Unknown => ClaudeBillingSource::Unknown,
        }
    };
    let (identity, subscription_type) = if raw.logged_in {
        let email = bounded_identity(raw.email)?;
        let organization_name = bounded_identity(raw.org_name)?;
        let subscription_type = bounded_identity(raw.subscription_type)?;
        let identity =
            (email.is_some() || organization_name.is_some()).then_some(ClaudeAuthIdentity {
                email,
                organization_name,
            });
        (identity, subscription_type)
    } else {
        // Never surface stale identity or plan observations for a logged-out
        // profile, even if a publisher build leaves those fields populated.
        (None, None)
    };
    Ok(ClaudeAuthStatus {
        logged_in: raw.logged_in,
        auth_method,
        api_provider: provider,
        billing_source,
        subscription_type,
        identity,
        billing_warning_code: environment_api_key.then_some(API_KEY_PRECEDENCE_WARNING_CODE),
    })
}

fn classify_auth_method(
    logged_in: bool,
    value: Option<&str>,
    provider: ClaudeApiProvider,
) -> ClaudeAuthMethod {
    if !logged_in {
        return ClaudeAuthMethod::NotAuthenticated;
    }
    if provider != ClaudeApiProvider::FirstParty && provider != ClaudeApiProvider::Unknown {
        return ClaudeAuthMethod::ThirdPartyPlatform;
    }
    match value.unwrap_or_default() {
        "claude.ai" | "oauth" => ClaudeAuthMethod::ClaudeAccount,
        "api_key" | "console" => ClaudeAuthMethod::AnthropicConsole,
        _ => ClaudeAuthMethod::Unknown,
    }
}

fn classify_provider(value: Option<&str>) -> ClaudeApiProvider {
    match value.unwrap_or_default() {
        "firstParty" | "first_party" | "anthropic" => ClaudeApiProvider::FirstParty,
        "bedrock" | "aws" => ClaudeApiProvider::AmazonBedrock,
        "vertex" | "vertex_ai" => ClaudeApiProvider::GoogleVertex,
        "foundry" | "azure" => ClaudeApiProvider::MicrosoftFoundry,
        _ => ClaudeApiProvider::Unknown,
    }
}

fn bounded_identity(value: Option<String>) -> Result<Option<String>, ClaudeAuthStatusError> {
    value
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.len() > MAX_AUTH_IDENTITY_BYTES
                || trimmed
                    .chars()
                    .any(|character| character == '\0' || character.is_control())
            {
                return Err(ClaudeAuthStatusError::IdentityInvalid);
            }
            Ok(Some(trimmed.to_owned()))
        })
        .transpose()
        .map(Option::flatten)
}
