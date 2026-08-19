use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    Expired,
    Error,
    Revoked,
}

impl ConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Expired => "expired",
            Self::Error => "error",
            Self::Revoked => "revoked",
        }
    }
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ConnectionStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "connected" => Ok(Self::Connected),
            "expired" => Ok(Self::Expired),
            "error" => Ok(Self::Error),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unknown connection status: {value}")),
        }
    }
}

/// Backend-only connection record. `credential_ref` and `identity_key` are
/// deliberately omitted from the command DTO below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConnection {
    pub id: String,
    pub provider_id: String,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_tenant_id: Option<String>,
    pub connection_mode: String,
    pub identity_key: String,
    pub scopes: Vec<String>,
    /// Provider-owned, non-secret routing metadata. This never crosses the
    /// frontend command boundary; credentials belong in `CredentialEnvelope`.
    pub provider_metadata: BTreeMap<String, String>,
    pub status: ConnectionStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub credential_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct UpsertAppConnection {
    pub provider_id: String,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_tenant_id: Option<String>,
    pub connection_mode: String,
    pub identity_key: String,
    pub scopes: Vec<String>,
    pub provider_metadata: BTreeMap<String, String>,
    pub expires_at: Option<String>,
    pub credential_ref: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConnectionDto {
    pub id: String,
    pub provider_id: String,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_tenant_id: Option<String>,
    pub connection_mode: String,
    pub scopes: Vec<String>,
    pub status: ConnectionStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AppConnection> for AppConnectionDto {
    fn from(connection: AppConnection) -> Self {
        Self {
            id: connection.id,
            provider_id: connection.provider_id,
            display_name: connection.display_name,
            external_account_id: connection.external_account_id,
            external_tenant_id: connection.external_tenant_id,
            connection_mode: connection.connection_mode,
            scopes: connection.scopes,
            status: connection.status,
            expires_at: connection.expires_at,
            last_checked_at: connection.last_checked_at,
            last_error_code: connection.last_error_code,
            created_at: connection.created_at,
            updated_at: connection.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProviderDto {
    pub id: String,
    pub name: String,
    pub capability_summary: String,
    pub connection_modes: Vec<String>,
    pub connect_available: bool,
    /// Unofficial integration that may break or put the linked account at risk.
    /// The UI must show a badge wherever the provider appears.
    pub experimental: bool,
    /// Only one account may be linked per Alfred installation. Enforced in
    /// `upsert_app_connection`, not merely in the UI.
    pub single_connection: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionUsageItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConnectionUsage {
    pub workflows: Vec<ConnectionUsageItem>,
    pub schedules: Vec<ConnectionUsageItem>,
    pub triggers: Vec<ConnectionUsageItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationCommandError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl IntegrationCommandError {
    pub fn new(code: &str, message: &str, recoverable: bool) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            recoverable,
        }
    }

    pub fn not_found() -> Self {
        Self::new(
            "connection_not_found",
            "The connected app could not be found.",
            false,
        )
    }
}

/// A length-framed digest prevents ambiguous concatenation while keeping raw
/// account, tenant, and installation identifiers out of the identity column.
pub fn canonical_identity_key(
    provider_id: &str,
    connection_mode: &str,
    identity_parts: &[&str],
) -> String {
    let mut hasher = Sha256::new();
    for value in std::iter::once(provider_id)
        .chain(std::iter::once(connection_mode))
        .chain(identity_parts.iter().copied())
    {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_but_separates_tenant_and_mode() {
        let first = canonical_identity_key("slack", "native_oauth", &["team-a", "user-a"]);
        assert_eq!(
            first,
            canonical_identity_key("slack", "native_oauth", &["team-a", "user-a"])
        );
        assert_ne!(
            first,
            canonical_identity_key("slack", "native_oauth", &["team-b", "user-a"])
        );
        assert_ne!(
            first,
            canonical_identity_key("slack", "private_bot", &["team-a", "user-a"])
        );
    }

    #[test]
    fn command_dto_does_not_serialize_backend_references() {
        let dto = AppConnectionDto::from(AppConnection {
            id: "connection".into(),
            provider_id: "slack".into(),
            display_name: Some("Workspace".into()),
            external_account_id: None,
            external_tenant_id: None,
            connection_mode: "native_oauth".into(),
            identity_key: "identity-fixture-that-must-not-leak".into(),
            scopes: vec!["channels:read".into()],
            provider_metadata: BTreeMap::new(),
            status: ConnectionStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "credential-fixture-that-must-not-leak".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        let json = serde_json::to_string(&dto).expect("serialize DTO");
        assert!(!json.contains("identity-fixture"));
        assert!(!json.contains("credential-fixture"));
    }
}
