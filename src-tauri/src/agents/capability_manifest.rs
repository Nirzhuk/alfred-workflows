//! Release capability source shared by execution, commands, and the editor.
//!
//! This is deliberately a closed, versioned manifest rather than a collection
//! of optimistic feature checks. A missing entry is disabled, and a native
//! entry is executable only after every release gate is recorded as passed.

use super::{AgentError, AgentHarness, AgentProvider};
use crate::agent_accounts::models::{AgentAccount, AgentProductId};
use crate::agents::native::NativeRuntimeRegistry;
use crate::agents::runtime_package::{inspect_runtime_package, RuntimePackageInspection};
use crate::db::AgentAccountReadDiagnostic;
use serde::Serialize;
use std::path::Path;

pub const AGENT_CAPABILITY_MANIFEST_VERSION: u16 = 1;
const MAX_DIAGNOSTIC_ACCOUNTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Disabled,
    #[allow(dead_code)] // Reserved by the versioned wire contract for staged provider rollout.
    Beta,
    Available,
    Blocked,
}

impl CapabilityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Beta => "beta",
            Self::Available => "available",
            Self::Blocked => "blocked",
        }
    }

    pub fn permits_execution(self) -> bool {
        matches!(self, Self::Beta | Self::Available)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Passed,
    Failed,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPlatform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopBuildKind {
    Development,
    Packaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityGate {
    pub gate: String,
    pub status: GateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethodGate {
    pub auth_method: String,
    pub status: GateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformGate {
    pub platform: DesktopPlatform,
    pub status: GateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildGate {
    pub build_kind: DesktopBuildKind,
    pub status: GateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagedRuntimeMetadata {
    pub kind: String,
    pub included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_path: Option<String>,
    pub checksum_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice_resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_resource_path: Option<String>,
    pub signing_status: String,
    pub rollback_status: String,
    pub data_independent: bool,
    pub automatic_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilityEntry {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    pub product: Option<AgentProductId>,
    pub runtime_id: Option<String>,
    pub runtime_version: Option<String>,
    pub platforms: Vec<DesktopPlatform>,
    pub build_kinds: Vec<DesktopBuildKind>,
    pub auth_methods: Vec<String>,
    pub auth_method_gates: Vec<AuthMethodGate>,
    pub platform_gates: Vec<PlatformGate>,
    pub build_gates: Vec<BuildGate>,
    pub billing_source: String,
    pub credential_custody: String,
    pub model_source: String,
    pub usage_source: String,
    pub supports_tools: bool,
    pub supports_approvals: bool,
    pub supports_resume: bool,
    pub supports_cancellation: bool,
    pub status: CapabilityStatus,
    pub block_reason: Option<String>,
    /// Final backend decision consumed by the editor. The frontend must not
    /// reconstruct package trust from descriptive metadata strings.
    pub execution_permitted: bool,
    pub gates: Vec<CapabilityGate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<PackagedRuntimeMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_inspection: Option<RuntimePackageInspection>,
}

impl AgentCapabilityEntry {
    pub fn permits_execution(
        &self,
        platform: DesktopPlatform,
        build_kind: DesktopBuildKind,
    ) -> bool {
        self.execution_permitted && self.execution_decision(platform, build_kind)
    }

    fn execution_decision(&self, platform: DesktopPlatform, build_kind: DesktopBuildKind) -> bool {
        self.status.permits_execution()
            && self.platforms.contains(&platform)
            && self.build_kinds.contains(&build_kind)
            && self
                .platform_gates
                .iter()
                .any(|gate| gate.platform == platform && gate.status == GateStatus::Passed)
            && self
                .build_gates
                .iter()
                .any(|gate| gate.build_kind == build_kind && gate.status == GateStatus::Passed)
            && self
                .auth_method_gates
                .iter()
                .any(|gate| gate.status == GateStatus::Passed)
            && self
                .gates
                .iter()
                .all(|gate| gate.status != GateStatus::Failed)
            && self
                .package
                .as_ref()
                .zip(self.package_inspection.as_ref())
                .is_some_and(|(package, inspection)| package_permits_execution(package, inspection))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilityManifest {
    pub schema_version: u16,
    pub platform: DesktopPlatform,
    pub build_kind: DesktopBuildKind,
    pub entries: Vec<AgentCapabilityEntry>,
}

impl AgentCapabilityManifest {
    pub fn is_valid(&self) -> bool {
        if self.schema_version != AGENT_CAPABILITY_MANIFEST_VERSION || self.entries.len() > 64 {
            return false;
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if self.entries[..index].iter().any(|candidate| {
                candidate.provider == entry.provider && candidate.harness == entry.harness
            }) {
                return false;
            }
            if matches!(
                entry.status,
                CapabilityStatus::Blocked | CapabilityStatus::Disabled
            ) && entry.block_reason.as_deref().is_none_or(str::is_empty)
            {
                return false;
            }
            if entry.harness == AgentHarness::Alfred
                && entry.status.permits_execution()
                && entry.runtime_version.as_deref().is_none_or(str::is_empty)
            {
                return false;
            }
            if entry.harness == AgentHarness::Cli {
                if entry.product.is_some() || entry.runtime_id.is_some() {
                    return false;
                }
            } else if let Some(product) = native_product(entry.provider) {
                let expected_auth = product
                    .auth_methods()
                    .iter()
                    .map(|method| (*method).to_owned())
                    .collect::<Vec<_>>();
                if entry.product != Some(product)
                    || entry.runtime_id.as_deref() != Some(product.capability_runtime_id())
                    || entry.runtime_version.as_deref()
                        != Some(product.capability_runtime_version())
                    || entry.auth_methods != expected_auth
                    || entry.billing_source != product.billing_source()
                    || entry.credential_custody != product.custody_mode().as_str()
                {
                    return false;
                }
            } else if entry.product.is_some() || entry.runtime_id.is_some() {
                return false;
            }
            if entry
                .package
                .as_ref()
                .is_some_and(|package| package.automatic_fallback)
            {
                return false;
            }
            let Some((package, inspection)) = entry
                .package
                .as_ref()
                .zip(entry.package_inspection.as_ref())
            else {
                return false;
            };
            if package.kind == "not_applicable" {
                if inspection.status
                    != crate::agents::runtime_package::RuntimeResourceStatus::NotApplicable
                {
                    return false;
                }
            } else if entry.status.permits_execution() && !package_metadata_is_complete(package) {
                return false;
            }
            if entry.platforms.iter().any(|platform| {
                entry
                    .platform_gates
                    .iter()
                    .filter(|gate| gate.platform == *platform)
                    .count()
                    != 1
            }) || entry.build_kinds.iter().any(|build_kind| {
                entry
                    .build_gates
                    .iter()
                    .filter(|gate| gate.build_kind == *build_kind)
                    .count()
                    != 1
            }) || entry.auth_methods.iter().any(|auth_method| {
                entry
                    .auth_method_gates
                    .iter()
                    .filter(|gate| gate.auth_method == *auth_method)
                    .count()
                    != 1
            }) {
                return false;
            }
            if entry.execution_permitted != entry.execution_decision(self.platform, self.build_kind)
            {
                return false;
            }
        }
        true
    }

    pub fn entry(
        &self,
        provider: AgentProvider,
        harness: AgentHarness,
    ) -> Option<&AgentCapabilityEntry> {
        self.entries
            .iter()
            .find(|entry| entry.provider == provider && entry.harness == harness)
    }

    pub fn product_entry(
        &self,
        product: AgentProductId,
        harness: AgentHarness,
    ) -> Option<&AgentCapabilityEntry> {
        self.entry(product.provider(), harness)
            .filter(|entry| entry.product == Some(product))
    }

    pub fn require_execution(
        &self,
        provider: AgentProvider,
        harness: AgentHarness,
    ) -> Result<(), AgentError> {
        self.is_valid()
            .then(|| self.entry(provider, harness))
            .flatten()
            .filter(|entry| entry.permits_execution(self.platform, self.build_kind))
            .map(|_| ())
            .ok_or(AgentError::NativeRuntimeUnavailable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedDiagnosticAccount {
    pub identity: String,
    pub auth_method: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stable_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessDiagnostic {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<AgentProductId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    pub capability_status: CapabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    pub runtime_state: String,
    pub last_runtime_exit_state: String,
    pub selection: String,
    pub accounts: Vec<RedactedDiagnosticAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedAccountStoreWarning {
    pub identity: String,
    pub stable_error_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessDiagnostics {
    pub schema_version: u16,
    pub platform: DesktopPlatform,
    pub build_kind: DesktopBuildKind,
    pub entries: Vec<AgentHarnessDiagnostic>,
    pub account_store_warnings: Vec<RedactedAccountStoreWarning>,
}

pub fn current_platform() -> DesktopPlatform {
    #[cfg(target_os = "macos")]
    return DesktopPlatform::Macos;
    #[cfg(target_os = "windows")]
    return DesktopPlatform::Windows;
    #[cfg(target_os = "linux")]
    return DesktopPlatform::Linux;
    #[allow(unreachable_code)]
    DesktopPlatform::Linux
}

pub fn current_build_kind() -> DesktopBuildKind {
    if cfg!(debug_assertions) {
        DesktopBuildKind::Development
    } else {
        DesktopBuildKind::Packaged
    }
}

pub fn manifest() -> AgentCapabilityManifest {
    manifest_for_resource_root(current_platform(), current_build_kind(), None)
}

pub fn manifest_for(
    platform: DesktopPlatform,
    build_kind: DesktopBuildKind,
) -> AgentCapabilityManifest {
    manifest_for_resource_root(platform, build_kind, None)
}

pub fn manifest_for_resource_root(
    platform: DesktopPlatform,
    build_kind: DesktopBuildKind,
    resource_root: Option<&Path>,
) -> AgentCapabilityManifest {
    let providers = [
        AgentProvider::ClaudeCode,
        AgentProvider::Cursor,
        AgentProvider::Codex,
        AgentProvider::Opencode,
        AgentProvider::GithubCopilot,
        AgentProvider::Gemini,
        AgentProvider::Grok,
        AgentProvider::Pi,
        AgentProvider::Omp,
    ];
    let mut entries = Vec::with_capacity(providers.len() * 2);
    for provider in providers {
        entries.push(finalize_entry(
            cli_entry(provider),
            platform,
            build_kind,
            resource_root,
        ));
        entries.push(finalize_entry(
            native_entry(provider),
            platform,
            build_kind,
            resource_root,
        ));
    }
    AgentCapabilityManifest {
        schema_version: AGENT_CAPABILITY_MANIFEST_VERSION,
        platform,
        build_kind,
        entries,
    }
}

pub fn support_diagnostics(
    manifest: &AgentCapabilityManifest,
    accounts: &[AgentAccount],
    account_read_diagnostics: &[AgentAccountReadDiagnostic],
    registry: &NativeRuntimeRegistry,
    provider_filter: Option<AgentProvider>,
    harness_filter: Option<AgentHarness>,
) -> AgentHarnessDiagnostics {
    let entries = manifest
        .entries
        .iter()
        .filter(|entry| provider_filter.is_none_or(|provider| entry.provider == provider))
        .filter(|entry| harness_filter.is_none_or(|harness| entry.harness == harness))
        .map(|entry| {
            let account_rows = accounts
                .iter()
                .filter(|account| {
                    account.provider == entry.provider
                        && account.harness == entry.harness
                        && entry
                            .product
                            .is_none_or(|product| account.product == product)
                })
                .take(MAX_DIAGNOSTIC_ACCOUNTS)
                .map(|account| RedactedDiagnosticAccount {
                    identity: redact_account_id(&account.id),
                    auth_method: account.auth_method.as_str().into(),
                    status: account.status.as_str().into(),
                    last_stable_error_code: account
                        .last_error_code
                        .as_deref()
                        .map(stable_diagnostic_code),
                })
                .collect();
            let runtime_registered =
                entry.harness == AgentHarness::Alfred && registry.contains(entry.provider);
            AgentHarnessDiagnostic {
                provider: entry.provider,
                harness: entry.harness,
                product: entry.product,
                runtime_id: entry.runtime_id.clone(),
                runtime_version: entry.runtime_version.clone(),
                capability_status: entry.status,
                block_reason: entry.block_reason.clone(),
                runtime_state: match entry.harness {
                    AgentHarness::Cli => "external_cli".into(),
                    AgentHarness::Alfred if runtime_registered => "registered_idle".into(),
                    AgentHarness::Alfred => "not_registered".into(),
                },
                // Provider payloads and process stderr never enter this DTO.
                last_runtime_exit_state: "not_observed".into(),
                selection: entry.harness.as_str().into(),
                accounts: account_rows,
            }
        })
        .collect();
    AgentHarnessDiagnostics {
        schema_version: manifest.schema_version,
        platform: manifest.platform,
        build_kind: manifest.build_kind,
        entries,
        account_store_warnings: account_read_diagnostics
            .iter()
            .take(MAX_DIAGNOSTIC_ACCOUNTS)
            .map(|diagnostic| RedactedAccountStoreWarning {
                identity: diagnostic
                    .account_id
                    .as_deref()
                    .map(redact_account_id)
                    .unwrap_or_else(|| "account_redacted".into()),
                stable_error_code: diagnostic.error_code.into(),
            })
            .collect(),
    }
}

fn cli_entry(provider: AgentProvider) -> AgentCapabilityEntry {
    AgentCapabilityEntry {
        provider,
        harness: AgentHarness::Cli,
        product: None,
        runtime_id: None,
        runtime_version: None,
        platforms: all_platforms(),
        build_kinds: all_builds(),
        auth_methods: vec!["provider_cli".into()],
        auth_method_gates: vec![auth_gate("provider_cli", GateStatus::Passed, None)],
        platform_gates: platform_gates(GateStatus::Passed, None),
        build_gates: build_gates(GateStatus::Passed, None),
        billing_source: "provider_cli_account".into(),
        credential_custody: "provider_cli_managed".into(),
        model_source: "provider_cli".into(),
        usage_source: cli_usage_source(provider).into(),
        supports_tools: true,
        supports_approvals: false,
        supports_resume: false,
        supports_cancellation: true,
        status: CapabilityStatus::Available,
        block_reason: None,
        execution_permitted: false,
        gates: vec![
            passed("compatibility"),
            passed("cancellation"),
            passed("redaction"),
        ],
        package: Some(not_applicable_package("user_installed_cli")),
        package_inspection: None,
    }
}

fn native_product(provider: AgentProvider) -> Option<AgentProductId> {
    match provider {
        AgentProvider::ClaudeCode => Some(AgentProductId::ClaudeApi),
        AgentProvider::Cursor => Some(AgentProductId::CursorCloud),
        AgentProvider::Codex => Some(AgentProductId::ChatgptCodex),
        AgentProvider::Opencode => Some(AgentProductId::OpencodeGo),
        AgentProvider::GithubCopilot => Some(AgentProductId::GithubCopilotSubscription),
        AgentProvider::Gemini => Some(AgentProductId::GeminiApi),
        AgentProvider::Grok => Some(AgentProductId::GrokApi),
        AgentProvider::Pi | AgentProvider::Omp => None,
    }
}

fn native_entry(provider: AgentProvider) -> AgentCapabilityEntry {
    let product = native_product(provider);
    let (status, models, usage, tools, approvals, resume, reason, package) = match provider {
        AgentProvider::Codex => (
            CapabilityStatus::Blocked,
            "codex_python_sdk",
            "runtime_observation_unavailable",
            true,
            true,
            true,
            "codex_cross_platform_signing_and_packaged_smoke_missing",
            unavailable_bundled_package("python_sidecar", "license_unverified"),
        ),
        AgentProvider::ClaudeCode => (
            CapabilityStatus::Blocked,
            "anthropic_models_api",
            "response_tokens_only",
            true,
            true,
            false,
            "claude_live_api_key_smoke_missing",
            not_applicable_package("direct_https"),
        ),
        AgentProvider::Cursor => (
            CapabilityStatus::Blocked,
            "cursor_cloud_agents_api",
            "cursor_per_run_usage",
            false,
            false,
            false,
            "cursor_account_repository_consent_and_e2e_gates_missing",
            not_applicable_package("cloud_https"),
        ),
        AgentProvider::Opencode => (
            CapabilityStatus::Blocked,
            "opencode_server",
            "runtime_account_observation",
            false,
            false,
            true,
            "opencode_package_account_and_tool_bridge_unverified",
            unavailable_bundled_package("sidecar", "MIT"),
        ),
        AgentProvider::GithubCopilot => (
            CapabilityStatus::Blocked,
            "github_copilot_sdk",
            "unavailable",
            true,
            true,
            false,
            "copilot_sdk_package_license_and_packaged_smoke_missing",
            unavailable_bundled_package("bundled_sdk_cli", "GitHub-Copilot-CLI-license"),
        ),
        AgentProvider::Gemini => (
            CapabilityStatus::Blocked,
            "gemini_models_api",
            "response_tokens_only",
            true,
            true,
            false,
            "gemini_live_api_key_smoke_missing",
            not_applicable_package("direct_https"),
        ),
        AgentProvider::Grok => (
            CapabilityStatus::Blocked,
            "xai_models_api",
            "response_tokens_only",
            true,
            true,
            false,
            "grok_live_api_key_smoke_missing",
            not_applicable_package("direct_https"),
        ),
        AgentProvider::Pi | AgentProvider::Omp => (
            CapabilityStatus::Disabled,
            "unavailable",
            "unavailable",
            false,
            false,
            false,
            "native_provider_not_implemented",
            not_applicable_package("none"),
        ),
    };
    let auth_methods = product
        .map(|product| {
            product
                .auth_methods()
                .iter()
                .map(|method| (*method).to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    AgentCapabilityEntry {
        provider,
        harness: AgentHarness::Alfred,
        product,
        runtime_id: product.map(|product| product.capability_runtime_id().into()),
        runtime_version: product.map(|product| product.capability_runtime_version().into()),
        platforms: all_platforms(),
        build_kinds: all_builds(),
        auth_methods: auth_methods.clone(),
        auth_method_gates: auth_methods
            .iter()
            .map(|method| auth_gate(method, GateStatus::Failed, Some(reason)))
            .collect(),
        platform_gates: platform_gates(GateStatus::Failed, Some(reason)),
        build_gates: build_gates(GateStatus::Failed, Some(reason)),
        billing_source: product
            .map(|product| product.billing_source())
            .unwrap_or("unavailable")
            .into(),
        credential_custody: product
            .map(|product| product.custody_mode().as_str())
            .unwrap_or("unavailable")
            .into(),
        model_source: models.into(),
        usage_source: usage.into(),
        supports_tools: tools,
        supports_approvals: approvals,
        supports_resume: resume,
        supports_cancellation: !matches!(provider, AgentProvider::Pi | AgentProvider::Omp),
        status,
        block_reason: Some(reason.into()),
        execution_permitted: false,
        gates: vec![
            failed("auth", reason),
            failed("runtime", reason),
            if package.kind == "not_applicable" {
                not_applicable("package")
            } else {
                failed("package", reason)
            },
            if matches!(provider, AgentProvider::Pi | AgentProvider::Omp) {
                failed("cancellation_contract", reason)
            } else {
                passed("cancellation_contract")
            },
            passed("redaction_contract"),
        ],
        package: Some(package),
        package_inspection: None,
    }
}

fn all_platforms() -> Vec<DesktopPlatform> {
    vec![
        DesktopPlatform::Macos,
        DesktopPlatform::Windows,
        DesktopPlatform::Linux,
    ]
}

fn all_builds() -> Vec<DesktopBuildKind> {
    vec![DesktopBuildKind::Development, DesktopBuildKind::Packaged]
}

fn cli_usage_source(provider: AgentProvider) -> &'static str {
    match provider {
        AgentProvider::ClaudeCode
        | AgentProvider::Cursor
        | AgentProvider::Codex
        | AgentProvider::Gemini => "provider_cli_when_supported",
        _ => "unavailable",
    }
}

fn passed(gate: &str) -> CapabilityGate {
    CapabilityGate {
        gate: gate.into(),
        status: GateStatus::Passed,
        reason: None,
    }
}

fn failed(gate: &str, reason: &str) -> CapabilityGate {
    CapabilityGate {
        gate: gate.into(),
        status: GateStatus::Failed,
        reason: Some(reason.into()),
    }
}

fn not_applicable(gate: &str) -> CapabilityGate {
    CapabilityGate {
        gate: gate.into(),
        status: GateStatus::NotApplicable,
        reason: None,
    }
}

fn auth_gate(auth_method: &str, status: GateStatus, reason: Option<&str>) -> AuthMethodGate {
    AuthMethodGate {
        auth_method: auth_method.into(),
        status,
        reason: reason.map(str::to_owned),
    }
}

fn platform_gates(status: GateStatus, reason: Option<&str>) -> Vec<PlatformGate> {
    all_platforms()
        .into_iter()
        .map(|platform| PlatformGate {
            platform,
            status,
            reason: reason.map(str::to_owned),
        })
        .collect()
}

fn build_gates(status: GateStatus, reason: Option<&str>) -> Vec<BuildGate> {
    all_builds()
        .into_iter()
        .map(|build_kind| BuildGate {
            build_kind,
            status,
            reason: reason.map(str::to_owned),
        })
        .collect()
}

fn not_applicable_package(_kind: &str) -> PackagedRuntimeMetadata {
    PackagedRuntimeMetadata {
        kind: "not_applicable".into(),
        included: false,
        resource_path: None,
        checksum_status: "not_applicable".into(),
        sha256: None,
        license: "not_applicable".into(),
        license_resource_path: None,
        notice_resource_path: None,
        signing_resource_path: None,
        rollback_resource_path: None,
        signing_status: "not_applicable".into(),
        rollback_status: "not_applicable".into(),
        data_independent: true,
        automatic_fallback: false,
    }
}

fn unavailable_bundled_package(kind: &str, license: &str) -> PackagedRuntimeMetadata {
    PackagedRuntimeMetadata {
        kind: kind.into(),
        included: false,
        resource_path: None,
        checksum_status: "unavailable".into(),
        sha256: None,
        license: license.into(),
        license_resource_path: None,
        notice_resource_path: None,
        signing_resource_path: None,
        rollback_resource_path: None,
        signing_status: "unverified".into(),
        rollback_status: "not_implemented".into(),
        data_independent: true,
        automatic_fallback: false,
    }
}

fn finalize_entry(
    mut entry: AgentCapabilityEntry,
    platform: DesktopPlatform,
    build_kind: DesktopBuildKind,
    resource_root: Option<&Path>,
) -> AgentCapabilityEntry {
    entry.package_inspection = entry.package.as_ref().map(|package| {
        resource_root.map_or_else(
            || {
                if package.kind == "not_applicable" {
                    inspect_runtime_package(
                        Path::new("."),
                        package,
                        entry.runtime_version.as_deref(),
                    )
                } else {
                    RuntimePackageInspection::missing(entry.runtime_version.as_deref())
                }
            },
            |root| inspect_runtime_package(root, package, entry.runtime_version.as_deref()),
        )
    });
    entry.execution_permitted = entry.execution_decision(platform, build_kind);
    entry
}

fn package_metadata_is_complete(package: &PackagedRuntimeMetadata) -> bool {
    package.included
        && package
            .resource_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && package.sha256.as_deref().is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        && !package.license.is_empty()
        && package.license != "not_applicable"
        && package
            .license_resource_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && package
            .notice_resource_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && package
            .signing_resource_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && package
            .rollback_resource_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && package.data_independent
        && !package.automatic_fallback
}

fn package_permits_execution(
    package: &PackagedRuntimeMetadata,
    inspection: &RuntimePackageInspection,
) -> bool {
    if package.kind == "not_applicable" {
        return package.data_independent
            && !package.automatic_fallback
            && inspection.status
                == crate::agents::runtime_package::RuntimeResourceStatus::NotApplicable;
    }
    package_metadata_is_complete(package)
        && inspection.status == crate::agents::runtime_package::RuntimeResourceStatus::Ready
        && inspection.versioned
        && inspection.checksum_verified
        && inspection.license_present
        && inspection.notice_present
        && inspection.data_independent
        && inspection.signing_verified
        && inspection.rollback_verified
}

fn redact_account_id(value: &str) -> String {
    let suffix = value.chars().rev().take(4).collect::<String>();
    let suffix = suffix.chars().rev().collect::<String>();
    if suffix.is_empty() {
        "account_redacted".into()
    } else {
        format!("account_…{suffix}")
    }
}

fn stable_diagnostic_code(value: &str) -> String {
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
    use crate::agent_accounts::models::{
        AgentAccountStatus, AgentAuthMethod, AgentEntitlementState, AgentProductId,
        CredentialCustodyMode,
    };

    fn inspected_native_fixture(
        root: Option<&Path>,
        package: PackagedRuntimeMetadata,
    ) -> AgentCapabilityManifest {
        let platform = DesktopPlatform::Macos;
        let build = DesktopBuildKind::Packaged;
        let mut entry = native_entry(AgentProvider::Codex);
        entry.status = CapabilityStatus::Available;
        entry.block_reason = None;
        entry.auth_method_gates = entry
            .auth_methods
            .iter()
            .map(|method| auth_gate(method, GateStatus::Passed, None))
            .collect();
        entry.platform_gates = platform_gates(GateStatus::Passed, None);
        entry.build_gates = build_gates(GateStatus::Passed, None);
        entry.gates = vec![passed("auth"), passed("runtime"), passed("package")];
        entry.package = Some(package);
        entry.package_inspection = None;
        entry = finalize_entry(entry, platform, build, root);
        AgentCapabilityManifest {
            schema_version: AGENT_CAPABILITY_MANIFEST_VERSION,
            platform,
            build_kind: build,
            entries: vec![entry],
        }
    }

    fn fixture_package() -> PackagedRuntimeMetadata {
        PackagedRuntimeMetadata {
            kind: "fixture_sidecar".into(),
            included: true,
            resource_path: Some("runtime/fixture-runtime.txt".into()),
            checksum_status: "expected".into(),
            sha256: Some("cbf38a1c6d10d43e8451d55fc441d491c9055a2ca5d386e1dc467e63cce2e289".into()),
            license: "MIT".into(),
            license_resource_path: Some("runtime/LICENSE.txt".into()),
            notice_resource_path: Some("runtime/NOTICE.txt".into()),
            signing_resource_path: Some("runtime/SIGNING.txt".into()),
            rollback_resource_path: Some("runtime/ROLLBACK.json".into()),
            signing_status: "verified".into(),
            rollback_status: "verified".into(),
            data_independent: true,
            automatic_fallback: false,
        }
    }

    #[test]
    fn release_matrix_keeps_cli_available_and_every_native_provider_closed() {
        let native_expectations = [
            (
                AgentProvider::ClaudeCode,
                CapabilityStatus::Blocked,
                "claude_live_api_key_smoke_missing",
            ),
            (
                AgentProvider::Cursor,
                CapabilityStatus::Blocked,
                "cursor_account_repository_consent_and_e2e_gates_missing",
            ),
            (
                AgentProvider::Codex,
                CapabilityStatus::Blocked,
                "codex_cross_platform_signing_and_packaged_smoke_missing",
            ),
            (
                AgentProvider::Opencode,
                CapabilityStatus::Blocked,
                "opencode_package_account_and_tool_bridge_unverified",
            ),
            (
                AgentProvider::GithubCopilot,
                CapabilityStatus::Blocked,
                "copilot_sdk_package_license_and_packaged_smoke_missing",
            ),
            (
                AgentProvider::Gemini,
                CapabilityStatus::Blocked,
                "gemini_live_api_key_smoke_missing",
            ),
            (
                AgentProvider::Grok,
                CapabilityStatus::Blocked,
                "grok_live_api_key_smoke_missing",
            ),
            (
                AgentProvider::Pi,
                CapabilityStatus::Disabled,
                "native_provider_not_implemented",
            ),
            (
                AgentProvider::Omp,
                CapabilityStatus::Disabled,
                "native_provider_not_implemented",
            ),
        ];
        for platform in [
            DesktopPlatform::Macos,
            DesktopPlatform::Windows,
            DesktopPlatform::Linux,
        ] {
            for build in [DesktopBuildKind::Development, DesktopBuildKind::Packaged] {
                let manifest = manifest_for(platform, build);
                assert!(manifest.is_valid());
                assert_eq!(manifest.schema_version, 1);
                assert_eq!(manifest.entries.len(), 18);
                for (provider, expected_status, expected_reason) in native_expectations {
                    assert!(manifest
                        .entry(provider, AgentHarness::Cli)
                        .unwrap()
                        .permits_execution(platform, build));
                    let native = manifest.entry(provider, AgentHarness::Alfred).unwrap();
                    assert!(!native.permits_execution(platform, build));
                    assert_eq!(native.status, expected_status);
                    assert_eq!(native.block_reason.as_deref(), Some(expected_reason));
                }
            }
        }
    }

    #[test]
    fn native_capabilities_agree_with_the_authoritative_product_registry() {
        let manifest = manifest_for(DesktopPlatform::Macos, DesktopBuildKind::Packaged);
        for entry in manifest
            .entries
            .iter()
            .filter(|entry| entry.harness == AgentHarness::Alfred)
        {
            let Some(product) = entry.product else {
                assert!(matches!(
                    entry.provider,
                    AgentProvider::Pi | AgentProvider::Omp
                ));
                continue;
            };
            assert_eq!(product.provider(), entry.provider);
            assert_eq!(
                entry.runtime_id.as_deref(),
                Some(product.capability_runtime_id())
            );
            assert_eq!(
                entry.runtime_version.as_deref(),
                Some(product.capability_runtime_version())
            );
            assert_eq!(entry.billing_source, product.billing_source());
            assert_eq!(entry.credential_custody, product.custody_mode().as_str());
            assert_eq!(
                entry.auth_methods,
                product
                    .auth_methods()
                    .iter()
                    .map(|method| (*method).to_owned())
                    .collect::<Vec<_>>()
            );
        }

        let codex = manifest
            .entry(AgentProvider::Codex, AgentHarness::Alfred)
            .expect("Codex capability");
        assert_eq!(codex.product, Some(AgentProductId::ChatgptCodex));
        assert_eq!(codex.runtime_id.as_deref(), Some("codex_python_sdk"));
        assert!(!format!("{codex:?}").contains("codex_app_server"));

        let opencode = manifest
            .entry(AgentProvider::Opencode, AgentHarness::Alfred)
            .expect("OpenCode capability");
        assert_eq!(opencode.product, Some(AgentProductId::OpencodeGo));
        assert_eq!(opencode.credential_custody, "runtime_managed");
        assert_eq!(opencode.billing_source, "provider_subscription");
        assert!(!format!("{opencode:?}").contains("upstream_provider_secret"));
    }

    #[test]
    fn missing_entry_and_failed_native_gate_are_disabled_without_fallback() {
        let mut manifest = manifest_for(DesktopPlatform::Macos, DesktopBuildKind::Packaged);
        manifest.entries.retain(|entry| {
            !(entry.provider == AgentProvider::Codex && entry.harness == AgentHarness::Cli)
        });
        assert!(matches!(
            manifest.require_execution(AgentProvider::Codex, AgentHarness::Cli),
            Err(AgentError::NativeRuntimeUnavailable)
        ));
        assert!(matches!(
            manifest.require_execution(AgentProvider::Codex, AgentHarness::Alfred),
            Err(AgentError::NativeRuntimeUnavailable)
        ));
        let mut duplicate = manifest_for(DesktopPlatform::Macos, DesktopBuildKind::Packaged);
        duplicate.entries.push(duplicate.entries[0].clone());
        assert!(!duplicate.is_valid());
        assert!(duplicate
            .require_execution(AgentProvider::ClaudeCode, AgentHarness::Cli)
            .is_err());
    }

    #[test]
    fn serialized_entries_keep_required_nulls_and_scoped_release_gates() {
        let manifest = manifest_for(DesktopPlatform::Macos, DesktopBuildKind::Packaged);
        let value = serde_json::to_value(&manifest).unwrap();
        let cli = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["provider"] == "codex" && entry["harness"] == "cli")
            .unwrap();
        assert!(cli.get("runtimeVersion").is_some());
        assert!(cli["runtimeVersion"].is_null());
        assert!(cli.get("blockReason").is_some());
        assert!(cli["blockReason"].is_null());

        let native = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["provider"] == "codex" && entry["harness"] == "alfred")
            .unwrap();
        assert_eq!(native["product"], "chatgpt_codex");
        assert_eq!(native["runtimeId"], "codex_python_sdk");
        assert_eq!(native["runtimeVersion"], "0.147.0");
        assert_eq!(native["platformGates"].as_array().unwrap().len(), 3);
        assert_eq!(native["buildGates"].as_array().unwrap().len(), 2);
        assert!(native["authMethodGates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate["status"] == "failed"));
    }

    #[test]
    fn execution_uses_real_package_inspection_and_fails_closed_on_bad_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");
        let ready = inspected_native_fixture(Some(&root), fixture_package());
        assert!(ready.is_valid());
        assert!(ready
            .require_execution(AgentProvider::Codex, AgentHarness::Alfred)
            .is_ok());

        let missing = inspected_native_fixture(None, fixture_package());
        assert!(missing.is_valid());
        assert!(missing
            .require_execution(AgentProvider::Codex, AgentHarness::Alfred)
            .is_err());

        let mut tampered = fixture_package();
        tampered.sha256 = Some("0".repeat(64));
        assert!(inspected_native_fixture(Some(&root), tampered)
            .require_execution(AgentProvider::Codex, AgentHarness::Alfred)
            .is_err());

        let mut unlicensed = fixture_package();
        unlicensed.license_resource_path = Some("runtime/NO-LICENSE".into());
        assert!(inspected_native_fixture(Some(&root), unlicensed)
            .require_execution(AgentProvider::Codex, AgentHarness::Alfred)
            .is_err());
    }

    #[test]
    fn diagnostics_are_bounded_and_never_serialize_private_account_fields() {
        let account = AgentAccount {
            id: "account_opaque_1234".into(),
            provider: AgentProvider::ClaudeCode,
            product: AgentProductId::ClaudeApi,
            harness: AgentHarness::Alfred,
            identity_key: "identity-private".into(),
            display_name: Some("private@example.com".into()),
            external_account_id: Some("external-private".into()),
            external_workspace_id: Some("workspace-private".into()),
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            managed_runtime_id: None,
            managed_runtime_version: None,
            runtime_profile_ref: None,
            scopes: vec!["private-scope".into()],
            billing_source: "provider_api".into(),
            billing_owner: "credential_owner".into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "provider_unobserved".into(),
            entitlement_observed_at: None,
            status: AgentAccountStatus::Error,
            expires_at: None,
            last_checked_at: None,
            last_error_code: Some("credential_invalid".into()),
            credential_ref: Some("credential-private".into()),
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        let registry = NativeRuntimeRegistry::default();
        let manifest = manifest();
        let diagnostic = support_diagnostics(
            &manifest,
            &[account],
            &[AgentAccountReadDiagnostic {
                account_id: Some("account_corrupt_5678".into()),
                error_code: "agent_account_row_corrupt",
            }],
            &registry,
            Some(AgentProvider::ClaudeCode),
            Some(AgentHarness::Alfred),
        );
        let json = serde_json::to_string(&diagnostic).unwrap();
        assert!(json.contains("account_…1234"));
        assert!(json.contains("account_…5678"));
        assert!(json.contains("agent_account_row_corrupt"));
        assert!(json.contains("credential_invalid"));
        for private in [
            "identity-private",
            "private@example.com",
            "external-private",
            "workspace-private",
            "private-scope",
            "credential-private",
        ] {
            assert!(!json.contains(private));
        }
        assert!(json.len() < 4_096);
    }
}
