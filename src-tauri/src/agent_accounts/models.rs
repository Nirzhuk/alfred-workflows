use crate::agents::{AgentHarness, AgentProvider};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProductId {
    ClaudeCodeSubscription,
    ClaudeApi,
    ChatgptCodex,
    OpenaiApi,
    OpencodeGo,
    OpencodeZen,
    CursorCloud,
    GithubCopilotSubscription,
    GeminiApi,
    GrokApi,
}

impl AgentProductId {
    pub const ALL: [Self; 10] = [
        Self::ClaudeCodeSubscription,
        Self::ClaudeApi,
        Self::ChatgptCodex,
        Self::OpenaiApi,
        Self::OpencodeGo,
        Self::OpencodeZen,
        Self::CursorCloud,
        Self::GithubCopilotSubscription,
        Self::GeminiApi,
        Self::GrokApi,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCodeSubscription => "claude_code_subscription",
            Self::ClaudeApi => "claude_api",
            Self::ChatgptCodex => "chatgpt_codex",
            Self::OpenaiApi => "openai_api",
            Self::OpencodeGo => "opencode_go",
            Self::OpencodeZen => "opencode_zen",
            Self::CursorCloud => "cursor_cloud",
            Self::GithubCopilotSubscription => "github_copilot_subscription",
            Self::GeminiApi => "gemini_api",
            Self::GrokApi => "grok_api",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCodeSubscription => "Claude Code subscription",
            Self::ClaudeApi => "Claude API",
            Self::ChatgptCodex => "ChatGPT Codex",
            Self::OpenaiApi => "OpenAI API",
            Self::OpencodeGo => "OpenCode Go",
            Self::OpencodeZen => "OpenCode Zen",
            Self::CursorCloud => "Cursor Cloud",
            Self::GithubCopilotSubscription => "GitHub Copilot subscription",
            Self::GeminiApi => "Gemini API",
            Self::GrokApi => "Grok API",
        }
    }

    pub fn provider(self) -> AgentProvider {
        match self {
            Self::ClaudeCodeSubscription | Self::ClaudeApi => AgentProvider::ClaudeCode,
            Self::ChatgptCodex | Self::OpenaiApi => AgentProvider::Codex,
            Self::OpencodeGo | Self::OpencodeZen => AgentProvider::Opencode,
            Self::CursorCloud => AgentProvider::Cursor,
            Self::GithubCopilotSubscription => AgentProvider::GithubCopilot,
            Self::GeminiApi => AgentProvider::Gemini,
            Self::GrokApi => AgentProvider::Grok,
        }
    }

    pub fn managed_runtime(self) -> Option<ManagedRuntimeId> {
        match self {
            Self::ClaudeCodeSubscription => Some(ManagedRuntimeId::ClaudeCodeManaged),
            Self::ChatgptCodex => Some(ManagedRuntimeId::CodexPythonSdk),
            Self::OpencodeGo | Self::OpencodeZen => Some(ManagedRuntimeId::OpencodeServer),
            Self::ClaudeApi
            | Self::OpenaiApi
            | Self::CursorCloud
            | Self::GithubCopilotSubscription
            | Self::GeminiApi
            | Self::GrokApi => None,
        }
    }

    pub fn is_managed_subscription(self) -> bool {
        matches!(
            self,
            Self::ClaudeCodeSubscription | Self::ChatgptCodex | Self::OpencodeGo
        )
    }

    pub fn requires_credential(self) -> bool {
        !self.is_managed_subscription()
    }

    pub fn uses_alfred_managed_api_key(self) -> bool {
        self.auth_method() == AgentAuthMethod::ApiKey
            && self.custody_mode() == CredentialCustodyMode::AlfredManaged
    }

    pub fn api_key_intake_gate_code(self) -> Option<&'static str> {
        match self {
            Self::ClaudeApi => Some("claude_live_api_key_smoke_missing"),
            Self::GeminiApi => Some("gemini_live_api_key_smoke_missing"),
            Self::GrokApi => Some("grok_live_api_key_smoke_missing"),
            _ => None,
        }
    }

    pub fn billing_source(self) -> &'static str {
        match self {
            Self::ClaudeCodeSubscription
            | Self::ChatgptCodex
            | Self::OpencodeGo
            | Self::GithubCopilotSubscription => "provider_subscription",
            Self::OpencodeZen => "provider_payg",
            Self::ClaudeApi
            | Self::OpenaiApi
            | Self::CursorCloud
            | Self::GeminiApi
            | Self::GrokApi => "provider_api",
        }
    }

    pub fn billing_owner(self) -> &'static str {
        match self {
            Self::ClaudeCodeSubscription
            | Self::ChatgptCodex
            | Self::OpencodeGo
            | Self::GithubCopilotSubscription => "subscription_account",
            Self::ClaudeApi
            | Self::OpenaiApi
            | Self::OpencodeZen
            | Self::CursorCloud
            | Self::GeminiApi
            | Self::GrokApi => "credential_owner",
        }
    }

    pub fn auth_method(self) -> AgentAuthMethod {
        match self {
            Self::ClaudeApi
            | Self::OpenaiApi
            | Self::OpencodeGo
            | Self::OpencodeZen
            | Self::CursorCloud
            | Self::GeminiApi
            | Self::GrokApi => AgentAuthMethod::ApiKey,
            Self::ChatgptCodex | Self::GithubCopilotSubscription => AgentAuthMethod::DeviceCode,
            Self::ClaudeCodeSubscription => AgentAuthMethod::Runtime,
        }
    }

    pub fn auth_methods(self) -> &'static [&'static str] {
        match self {
            Self::ChatgptCodex => &["oauth_pkce", "device_code"],
            product => match product.auth_method() {
                AgentAuthMethod::ApiKey => &["api_key"],
                AgentAuthMethod::OAuthPkce => &["oauth_pkce"],
                AgentAuthMethod::DeviceCode => &["device_code"],
                AgentAuthMethod::Runtime => &["runtime"],
            },
        }
    }

    pub fn custody_mode(self) -> CredentialCustodyMode {
        if self.is_managed_subscription() {
            CredentialCustodyMode::RuntimeManaged
        } else {
            CredentialCustodyMode::AlfredManaged
        }
    }

    pub fn managed_runtime_version(self) -> Option<&'static str> {
        match self.managed_runtime() {
            Some(ManagedRuntimeId::ClaudeCodeManaged) => Some("2.1.246"),
            Some(ManagedRuntimeId::CodexPythonSdk) => Some("0.147.0"),
            Some(ManagedRuntimeId::OpencodeServer) => Some("1.18.23"),
            None => None,
        }
    }

    pub fn capability_runtime_id(self) -> &'static str {
        if let Some(runtime) = self.managed_runtime() {
            return runtime.as_str();
        }
        match self {
            Self::ClaudeApi => "claude-native-anthropic-api",
            Self::OpenaiApi => "openai_responses_api",
            Self::CursorCloud => "cursor_cloud_agents_api",
            Self::GithubCopilotSubscription => "github-copilot-native",
            Self::GeminiApi => "gemini-native",
            Self::GrokApi => "xai-responses",
            Self::ClaudeCodeSubscription
            | Self::ChatgptCodex
            | Self::OpencodeGo
            | Self::OpencodeZen => unreachable!("managed products return above"),
        }
    }

    pub fn capability_runtime_version(self) -> &'static str {
        if let Some(version) = self.managed_runtime_version() {
            return version;
        }
        match self {
            Self::ClaudeApi => "0.1.0",
            Self::OpenaiApi => "unimplemented",
            Self::CursorCloud => "v1-public-beta-2026-08-25",
            Self::GithubCopilotSubscription => "github-copilot-sdk-1.0.11",
            Self::GeminiApi => "1.0.0",
            Self::GrokApi => "0.1.0-blocked-account-setup",
            Self::ClaudeCodeSubscription
            | Self::ChatgptCodex
            | Self::OpencodeGo
            | Self::OpencodeZen => unreachable!("managed products return above"),
        }
    }
}

impl FromStr for AgentProductId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude_code_subscription" => Ok(Self::ClaudeCodeSubscription),
            "claude_api" => Ok(Self::ClaudeApi),
            "chatgpt_codex" => Ok(Self::ChatgptCodex),
            "openai_api" => Ok(Self::OpenaiApi),
            "opencode_go" => Ok(Self::OpencodeGo),
            "opencode_zen" => Ok(Self::OpencodeZen),
            "cursor_cloud" => Ok(Self::CursorCloud),
            "github_copilot_subscription" => Ok(Self::GithubCopilotSubscription),
            "gemini_api" => Ok(Self::GeminiApi),
            "grok_api" => Ok(Self::GrokApi),
            _ => Err("unknown agent product".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedRuntimeId {
    ClaudeCodeManaged,
    CodexPythonSdk,
    OpencodeServer,
}

impl ManagedRuntimeId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCodeManaged => "claude_code_managed",
            Self::CodexPythonSdk => "codex_python_sdk",
            Self::OpencodeServer => "opencode_server",
        }
    }
}

impl FromStr for ManagedRuntimeId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude_code_managed" => Ok(Self::ClaudeCodeManaged),
            "codex_python_sdk" => Ok(Self::CodexPythonSdk),
            "opencode_server" => Ok(Self::OpencodeServer),
            _ => Err("unknown managed runtime".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEntitlementState {
    Unknown,
    Eligible,
    Limited,
    Exhausted,
    Ineligible,
}

impl AgentEntitlementState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Eligible => "eligible",
            Self::Limited => "limited",
            Self::Exhausted => "exhausted",
            Self::Ineligible => "ineligible",
        }
    }
}

impl FromStr for AgentEntitlementState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "eligible" => Ok(Self::Eligible),
            "limited" => Ok(Self::Limited),
            "exhausted" => Ok(Self::Exhausted),
            "ineligible" => Ok(Self::Ineligible),
            _ => Err("unknown agent entitlement state".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAuthMethod {
    ApiKey,
    OAuthPkce,
    DeviceCode,
    Runtime,
}

impl AgentAuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
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
            "api_key" => Ok(Self::ApiKey),
            "oauth_pkce" => Ok(Self::OAuthPkce),
            "device_code" => Ok(Self::DeviceCode),
            "runtime" => Ok(Self::Runtime),
            _ => Err("unknown agent auth method".into()),
        }
    }
}

/// Secret-bearing input for the dedicated native API-key account command.
/// It cannot be serialized or formatted and is scrubbed after being moved
/// into the narrower `Zeroizing` service boundary.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct AgentApiKeySecret(String);

impl AgentApiKeySecret {
    pub fn into_zeroizing(mut self) -> Zeroizing<String> {
        Zeroizing::new(std::mem::take(&mut self.0))
    }
}

impl Drop for AgentApiKeySecret {
    fn drop(&mut self) {
        self.0.zeroize();
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
#[derive(Clone, PartialEq, Eq)]
pub struct AgentAccount {
    pub id: String,
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub harness: AgentHarness,
    pub identity_key: String,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub managed_runtime_id: Option<ManagedRuntimeId>,
    pub managed_runtime_version: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub scopes: Vec<String>,
    pub billing_source: String,
    pub billing_owner: String,
    pub entitlement_state: AgentEntitlementState,
    pub entitlement_source: String,
    pub entitlement_observed_at: Option<String>,
    pub status: AgentAccountStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub credential_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl fmt::Debug for AgentAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAccount")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("product", &self.product)
            .field("harness", &self.harness)
            .field("identity_key", &"[REDACTED]")
            .field(
                "display_name",
                &self.display_name.as_ref().map(|_| "[REDACTED]"),
            )
            .field("auth_method", &self.auth_method)
            .field("custody_mode", &self.custody_mode)
            .field("managed_runtime_id", &self.managed_runtime_id)
            .field("managed_runtime_version", &self.managed_runtime_version)
            .field(
                "runtime_profile_ref",
                &self.runtime_profile_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field("billing_source", &self.billing_source)
            .field("billing_owner", &self.billing_owner)
            .field("entitlement_state", &self.entitlement_state)
            .field("entitlement_source", &self.entitlement_source)
            .field("entitlement_observed_at", &self.entitlement_observed_at)
            .field("status", &self.status)
            .field(
                "credential_ref",
                &self.credential_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

pub struct AuthorizedAgentAccount {
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub harness: AgentHarness,
    pub display_name: Option<String>,
    pub external_account_id: String,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub managed_runtime_id: Option<ManagedRuntimeId>,
    pub managed_runtime_version: Option<String>,
    pub runtime_profile_ref: Option<String>,
    pub scopes: Vec<String>,
    pub billing_source: String,
    pub billing_owner: String,
    pub entitlement_state: AgentEntitlementState,
    pub entitlement_source: String,
    pub entitlement_observed_at: Option<String>,
    pub expires_at: Option<String>,
}

impl fmt::Debug for AuthorizedAgentAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedAgentAccount")
            .field("provider", &self.provider)
            .field("product", &self.product)
            .field("harness", &self.harness)
            .field(
                "display_name",
                &self.display_name.as_ref().map(|_| "[REDACTED]"),
            )
            .field("external_account_id", &"[REDACTED]")
            .field(
                "external_workspace_id",
                &self.external_workspace_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("auth_method", &self.auth_method)
            .field("custody_mode", &self.custody_mode)
            .field("managed_runtime_id", &self.managed_runtime_id)
            .field("managed_runtime_version", &self.managed_runtime_version)
            .field(
                "runtime_profile_ref",
                &self.runtime_profile_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field("billing_source", &self.billing_source)
            .field("billing_owner", &self.billing_owner)
            .field("entitlement_state", &self.entitlement_state)
            .field("entitlement_source", &self.entitlement_source)
            .field("entitlement_observed_at", &self.entitlement_observed_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl AuthorizedAgentAccount {
    pub fn identity_key(&self) -> String {
        canonical_agent_identity_key(
            self.provider,
            self.product,
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
    pub product_id: String,
    pub product_name: String,
    pub harness: AgentHarness,
    pub display_name: Option<String>,
    pub external_account_id: Option<String>,
    pub external_workspace_id: Option<String>,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub managed_runtime_id: Option<String>,
    pub managed_runtime_version: Option<String>,
    pub scopes: Vec<String>,
    pub billing_source: String,
    pub billing_owner: String,
    pub entitlement_state: AgentEntitlementState,
    pub entitlement_source: String,
    pub entitlement_observed_at: Option<String>,
    pub status: AgentAccountStatus,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AgentAccount> for AgentAccountDto {
    fn from(account: AgentAccount) -> Self {
        let (provider_name, external_account_id, external_workspace_id) =
            if account.auth_method == AgentAuthMethod::ApiKey {
                let provider_name = match account.provider {
                    AgentProvider::ClaudeCode => "Claude",
                    provider => provider.label(),
                };
                (provider_name, None, None)
            } else {
                (
                    account.provider.label(),
                    account.external_account_id,
                    account.external_workspace_id,
                )
            };
        Self {
            id: account.id,
            provider_id: account.provider.as_str().into(),
            provider_name: provider_name.into(),
            product_id: account.product.as_str().into(),
            product_name: account.product.label().into(),
            harness: account.harness,
            display_name: account.display_name,
            external_account_id,
            external_workspace_id,
            auth_method: account.auth_method,
            custody_mode: account.custody_mode,
            managed_runtime_id: account
                .managed_runtime_id
                .map(|runtime| runtime.as_str().into()),
            managed_runtime_version: account.managed_runtime_version,
            scopes: account.scopes,
            billing_source: account.billing_source,
            billing_owner: account.billing_owner,
            entitlement_state: account.entitlement_state,
            entitlement_source: account.entitlement_source,
            entitlement_observed_at: account.entitlement_observed_at,
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
    pub product_id: String,
    pub product_name: String,
    pub harness: AgentHarness,
    pub auth_methods: Vec<String>,
    pub billing_source: String,
    pub billing_owner: String,
    pub credential_custody: String,
    pub managed_runtime_id: Option<String>,
    pub managed_runtime_version: Option<String>,
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
    product: AgentProductId,
    harness: AgentHarness,
    account_id: &str,
    workspace_id: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        provider.as_str(),
        product.as_str(),
        harness.as_str(),
        account_id,
        workspace_id.unwrap_or(""),
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub fn validate_domain_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn validate_authorized_agent_account(account: &AuthorizedAgentAccount) -> Result<(), String> {
    validate_account_contract(
        account.provider,
        account.product,
        account.harness,
        account.auth_method,
        account.custody_mode,
        account.managed_runtime_id,
        account.managed_runtime_version.as_deref(),
        account.runtime_profile_ref.as_deref(),
        None,
        &account.billing_source,
        &account.billing_owner,
        account.entitlement_state,
        &account.entitlement_source,
        account.entitlement_observed_at.as_deref(),
        AccountValidationContext::Write,
    )
}

pub fn validate_agent_account(account: &AgentAccount) -> Result<(), String> {
    validate_account_contract(
        account.provider,
        account.product,
        account.harness,
        account.auth_method,
        account.custody_mode,
        account.managed_runtime_id,
        account.managed_runtime_version.as_deref(),
        account.runtime_profile_ref.as_deref(),
        account.credential_ref.as_deref(),
        &account.billing_source,
        &account.billing_owner,
        account.entitlement_state,
        &account.entitlement_source,
        account.entitlement_observed_at.as_deref(),
        AccountValidationContext::Read,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountValidationContext {
    Read,
    Write,
}

#[allow(clippy::too_many_arguments)]
fn validate_account_contract(
    provider: AgentProvider,
    product: AgentProductId,
    harness: AgentHarness,
    auth_method: AgentAuthMethod,
    custody_mode: CredentialCustodyMode,
    managed_runtime_id: Option<ManagedRuntimeId>,
    managed_runtime_version: Option<&str>,
    runtime_profile_ref: Option<&str>,
    credential_ref: Option<&str>,
    billing_source: &str,
    billing_owner: &str,
    entitlement_state: AgentEntitlementState,
    entitlement_source: &str,
    entitlement_observed_at: Option<&str>,
    context: AccountValidationContext,
) -> Result<(), String> {
    if provider != product.provider() || harness != AgentHarness::Alfred {
        return Err("agent product does not match its provider and harness".into());
    }
    if !product.auth_methods().contains(&auth_method.as_str())
        || custody_mode != product.custody_mode()
    {
        return Err("agent product auth or custody mode is invalid".into());
    }
    match product.managed_runtime() {
        Some(expected) => {
            if managed_runtime_id != Some(expected)
                || runtime_profile_ref
                    .is_none_or(|value| value.trim().is_empty() || value.len() > 512)
            {
                return Err("managed agent product requires its registered runtime profile".into());
            }
            let version_is_invalid = managed_runtime_version.is_none_or(|version| {
                version.trim().is_empty()
                    || version.len() > 64
                    || (context == AccountValidationContext::Write
                        && Some(version) != product.managed_runtime_version())
            });
            if version_is_invalid {
                return Err("managed agent product runtime version is invalid".into());
            }
        }
        None => {
            if managed_runtime_id.is_some()
                || managed_runtime_version.is_some()
                || runtime_profile_ref.is_some()
            {
                return Err("agent product does not use a managed runtime profile".into());
            }
        }
    }
    if product.is_managed_subscription() && credential_ref.is_some() {
        return Err(
            "managed subscription accounts must not use a secret credential reference".into(),
        );
    }
    if context == AccountValidationContext::Read
        && product.requires_credential()
        && credential_ref.is_none_or(|value| value.trim().is_empty() || value.len() > 512)
    {
        return Err("agent product requires an opaque secret credential reference".into());
    }
    if billing_source != product.billing_source()
        || billing_owner != product.billing_owner()
        || !validate_domain_identifier(entitlement_source)
    {
        return Err("agent billing or entitlement source is invalid".into());
    }
    if entitlement_state != AgentEntitlementState::Unknown && entitlement_observed_at.is_none() {
        return Err("observed entitlement state requires a timestamp".into());
    }
    if entitlement_observed_at
        .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_err())
    {
        return Err("entitlement observation timestamp is invalid".into());
    }
    Ok(())
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
            AgentProductId::ChatgptCodex,
            AgentHarness::Alfred,
            "user-a",
            Some("org-a"),
        );
        assert_eq!(
            first,
            canonical_agent_identity_key(
                AgentProvider::Codex,
                AgentProductId::ChatgptCodex,
                AgentHarness::Alfred,
                "user-a",
                Some("org-a")
            )
        );
        assert_ne!(
            first,
            canonical_agent_identity_key(
                AgentProvider::Codex,
                AgentProductId::ChatgptCodex,
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
            product: AgentProductId::OpenaiApi,
            harness: AgentHarness::Alfred,
            identity_key: "identity-secret-fixture".into(),
            display_name: Some("User".into()),
            external_account_id: Some("external".into()),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            managed_runtime_id: None,
            managed_runtime_version: None,
            runtime_profile_ref: None,
            scopes: vec!["models:read".into()],
            billing_source: "provider_api".into(),
            billing_owner: "credential_owner".into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "not_observed".into(),
            entitlement_observed_at: None,
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: Some("credential-secret-fixture".into()),
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        let json = serde_json::to_string(&AgentAccountDto::from(account)).expect("serialize");
        assert!(!json.contains("identity-secret"));
        assert!(!json.contains("credential-secret"));
        assert!(!json.contains("token"));
    }

    #[test]
    fn product_registry_is_complete_and_managed_subscriptions_require_profiles() {
        assert_eq!(AgentProductId::ALL.len(), 10);
        let ids = AgentProductId::ALL
            .into_iter()
            .map(AgentProductId::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "claude_code_subscription",
                "claude_api",
                "chatgpt_codex",
                "openai_api",
                "opencode_go",
                "opencode_zen",
                "cursor_cloud",
                "github_copilot_subscription",
                "gemini_api",
                "grok_api",
            ]
        );
        assert_eq!(
            AgentProductId::ClaudeCodeSubscription.managed_runtime(),
            Some(ManagedRuntimeId::ClaudeCodeManaged)
        );
        assert_eq!(
            AgentProductId::ChatgptCodex.managed_runtime(),
            Some(ManagedRuntimeId::CodexPythonSdk)
        );
        assert_eq!(
            AgentProductId::OpencodeGo.managed_runtime(),
            Some(ManagedRuntimeId::OpencodeServer)
        );
        assert!(AgentProductId::ALL.into_iter().all(|product| {
            product.managed_runtime().is_some() == product.managed_runtime_version().is_some()
        }));
        assert!(AgentProductId::ALL.into_iter().all(|product| {
            product.managed_runtime().is_none_or(|runtime| {
                product.capability_runtime_id() == runtime.as_str()
                    && Some(product.capability_runtime_version())
                        == product.managed_runtime_version()
            })
        }));
        assert!(AgentProductId::ALL
            .into_iter()
            .filter(|product| product.is_managed_subscription())
            .all(|product| !product.requires_credential()));
        assert!(AgentProductId::OpencodeZen.managed_runtime().is_some());
        assert!(AgentProductId::OpencodeZen.requires_credential());
        assert!(AgentProductId::OpencodeZen.uses_alfred_managed_api_key());

        let managed = AgentAccount {
            id: "account_managed".into(),
            provider: AgentProvider::Codex,
            product: AgentProductId::ChatgptCodex,
            harness: AgentHarness::Alfred,
            identity_key: "opaque".into(),
            display_name: None,
            external_account_id: Some("user".into()),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::DeviceCode,
            custody_mode: CredentialCustodyMode::RuntimeManaged,
            managed_runtime_id: Some(ManagedRuntimeId::CodexPythonSdk),
            managed_runtime_version: Some("0.147.0".into()),
            runtime_profile_ref: Some("profile-opaque".into()),
            scopes: Vec::new(),
            billing_source: "provider_subscription".into(),
            billing_owner: "subscription_account".into(),
            entitlement_state: AgentEntitlementState::Eligible,
            entitlement_source: "runtime_account".into(),
            entitlement_observed_at: Some("2026-08-26T12:00:00Z".into()),
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: None,
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        assert!(validate_agent_account(&managed).is_ok());
        let mut older_runtime_observation = managed.clone();
        older_runtime_observation.managed_runtime_version = Some("0.146.0".into());
        assert!(validate_agent_account(&older_runtime_observation).is_ok());
        let mut stale_write = AuthorizedAgentAccount {
            provider: older_runtime_observation.provider,
            product: older_runtime_observation.product,
            harness: older_runtime_observation.harness,
            display_name: older_runtime_observation.display_name.clone(),
            external_account_id: "user".into(),
            external_workspace_id: None,
            auth_method: older_runtime_observation.auth_method,
            custody_mode: older_runtime_observation.custody_mode,
            managed_runtime_id: older_runtime_observation.managed_runtime_id,
            managed_runtime_version: older_runtime_observation.managed_runtime_version.clone(),
            runtime_profile_ref: older_runtime_observation.runtime_profile_ref.clone(),
            scopes: Vec::new(),
            billing_source: older_runtime_observation.billing_source.clone(),
            billing_owner: older_runtime_observation.billing_owner.clone(),
            entitlement_state: older_runtime_observation.entitlement_state,
            entitlement_source: older_runtime_observation.entitlement_source.clone(),
            entitlement_observed_at: older_runtime_observation.entitlement_observed_at.clone(),
            expires_at: None,
        };
        assert!(validate_authorized_agent_account(&stale_write).is_err());
        stale_write.managed_runtime_version = Some("0.147.0".into());
        assert!(validate_authorized_agent_account(&stale_write).is_ok());
        let managed_json = serde_json::to_string(&AgentAccountDto::from(managed.clone()))
            .expect("serialize managed DTO");
        assert!(!managed_json.contains("profile-opaque"));
        assert!(!managed_json.contains("runtimeProfileRef"));
        assert!(!managed_json.contains("credentialRef"));
        let mut invalid = managed.clone();
        invalid.credential_ref = Some("fake-secret-ref".into());
        assert!(validate_agent_account(&invalid).is_err());
    }
}
