use crate::agents::{AgentHarness, AgentProvider};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAuthMethod {
    OAuthPkce,
    DeviceCode,
    Runtime,
}

impl AgentAuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OAuthPkce => "oauth_pkce",
            Self::DeviceCode => "device_code",
            Self::Runtime => "runtime",
        }
    }
}

impl FromStr for AgentAuthMethod {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "oauth_pkce" => Ok(Self::OAuthPkce),
            "device_code" => Ok(Self::DeviceCode),
            "runtime" => Ok(Self::Runtime),
            _ => Err("unknown agent auth method".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialCustodyMode {
    AlfredManaged,
    RuntimeManaged,
}

impl CredentialCustodyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlfredManaged => "alfred_managed",
            Self::RuntimeManaged => "runtime_managed",
        }
    }
}

impl FromStr for CredentialCustodyMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "alfred_managed" => Ok(Self::AlfredManaged),
            "runtime_managed" => Ok(Self::RuntimeManaged),
            _ => Err("unknown credential custody mode".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAccountStatus {
    Connected,
    Expired,
    Error,
    Revoked,
    DisconnectPending,
}

impl AgentAccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Expired => "expired",
            Self::Error => "error",
            Self::Revoked => "revoked",
            Self::DisconnectPending => "disconnect_pending",
        }
    }
}

impl FromStr for AgentAccountStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "connected" => Ok(Self::Connected),
            "expired" => Ok(Self::Expired),
            "error" => Ok(Self::Error),
            "revoked" => Ok(Self::Revoked),
            "disconnect_pending" => Ok(Self::DisconnectPending),
            _ => Err("unknown agent account status".into()),
        }
    }
}

/// Backend record. Identity and credential references never cross a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccount {
    pub id: String,
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    pub identity_key: String,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub scopes: Vec<String>,
    pub status: AgentAccountStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub credential_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub struct AuthorizedAgentAccount {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    pub display_name: Option<String>,
    pub external_account_id: String,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
}

impl AuthorizedAgentAccount {
    pub fn identity_key(&self) -> String {
        canonical_agent_identity_key(
            self.provider,
            self.harness,
            &self.external_account_id,
            self.external_workspace_id.as_deref(),
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccountDto {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub harness: AgentHarness,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub scopes: Vec<String>,
    pub status: AgentAccountStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AgentAccount> for AgentAccountDto {
    fn from(account: AgentAccount) -> Self {
        Self {
            id: account.id,
            provider_id: account.provider.as_str().into(),
            provider_name: account.provider.label().into(),
            harness: account.harness,
            display_name: account.display_name,
            external_account_id: account.external_account_id,
            external_workspace_id: account.external_workspace_id,
            auth_method: account.auth_method,
            custody_mode: account.custody_mode,
            scopes: account.scopes,
            status: account.status,
            expires_at: account.expires_at,
            last_checked_at: account.last_checked_at,
            last_error_code: account.last_error_code,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderRegistrationDto {
    pub provider_id: String,
    pub provider_name: String,
    pub harness: AgentHarness,
    pub auth_methods: Vec<String>,
    pub billing_source: String,
    pub credential_custody: String,
    pub connect_available: bool,
    pub gate_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccountCommandError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl AgentAccountCommandError {
    pub fn new(code: &str, message: &str, recoverable: bool) -> Self {
        Self {
            code: stable_error_code(code),
            message: message.into(),
            recoverable,
        }
    }
}

impl fmt::Display for AgentAccountCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentAccountCommandError {}

pub fn canonical_agent_identity_key(
    provider: AgentProvider,
    harness: AgentHarness,
    account_id: &str,
    workspace_id: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        provider.as_str(),
        harness.as_str(),
        account_id,
        workspace_id.unwrap_or(""),
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub fn stable_error_code(value: &str) -> String {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        value.into()
    } else {
        "agent_account_failed".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_deterministic_and_separate_accounts() {
        let first = canonical_agent_identity_key(
            AgentProvider::Codex,
            AgentHarness::Alfred,
            "user-a",
            Some("org-a"),
        );
        assert_eq!(
            first,
            canonical_agent_identity_key(
                AgentProvider::Codex,
                AgentHarness::Alfred,
                "user-a",
                Some("org-a")
            )
        );
        assert_ne!(
            first,
            canonical_agent_identity_key(
                AgentProvider::Codex,
                AgentHarness::Alfred,
                "user-b",
                Some("org-a")
            )
        );
    }

    #[test]
    fn dto_serialization_omits_backend_references() {
        let account = AgentAccount {
            id: "account_fixture".into(),
            provider: AgentProvider::Codex,
            harness: AgentHarness::Alfred,
            identity_key: "identity-secret-fixture".into(),
            display_name: Some("User".into()),
            external_account_id: Some("external".into()),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::OAuthPkce,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: vec!["models:read".into()],
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "credential-secret-fixture".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        let json = serde_json::to_string(&AgentAccountDto::from(account)).expect("serialize");
        assert!(!json.contains("identity-secret"));
        assert!(!json.contains("credential-secret"));
        assert!(!json.contains("token"));
    }
}
