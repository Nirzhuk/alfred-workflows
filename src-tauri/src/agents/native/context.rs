use super::redaction::{contains_cli_permission_flag, contains_secret_marker};
use super::{
    NativeCancellation, NativeContextBlock, NativeContextRole, NativeErrorCode, NativeEventLimits,
    NativePermissionProfile, NativeRuntimeDescriptor, NativeRuntimeError, NativeSessionMode,
    NativeToolCapabilitySet, NativeTurnRequest, DEFAULT_TURN_TIMEOUT,
    NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::{AgentExecutionTarget, AgentHarness, AgentRequest};
use crate::skills::list_skills;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_MAX_CONTEXT_BYTES: usize = 256 * 1024;
pub const DEFAULT_MAX_CONTEXT_BLOCK_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_CONTEXT_BLOCKS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeContextPolicy {
    pub max_total_bytes: usize,
    pub max_block_bytes: usize,
    pub max_blocks: usize,
}

impl Default for NativeContextPolicy {
    fn default() -> Self {
        Self {
            max_total_bytes: DEFAULT_MAX_CONTEXT_BYTES,
            max_block_bytes: DEFAULT_MAX_CONTEXT_BLOCK_BYTES,
            max_blocks: DEFAULT_MAX_CONTEXT_BLOCKS,
        }
    }
}

impl NativeContextPolicy {
    pub fn validate_blocks(&self, blocks: &[NativeContextBlock]) -> Result<(), NativeRuntimeError> {
        if blocks.is_empty() || blocks.len() > self.max_blocks {
            return Err(invalid_context("native context block count is invalid"));
        }
        let mut total = 0usize;
        for block in blocks {
            if block.content.is_empty() || block.content.len() > self.max_block_bytes {
                return Err(invalid_context(
                    "native context block exceeds its byte limit",
                ));
            }
            total = total.saturating_add(block.content.len());
            if total > self.max_total_bytes {
                return Err(invalid_context(
                    "native context exceeds its total byte limit",
                ));
            }
        }
        if blocks.last().map(|block| &block.role) != Some(&NativeContextRole::User) {
            return Err(invalid_context(
                "native context must end with the user prompt",
            ));
        }
        Ok(())
    }
}

pub fn prepare_native_request(
    target: &AgentExecutionTarget,
    request: &AgentRequest,
    descriptor: &NativeRuntimeDescriptor,
    permission_profile: NativePermissionProfile,
    tool_capabilities: NativeToolCapabilitySet,
    event_limits: NativeEventLimits,
    context_policy: &NativeContextPolicy,
) -> Result<NativeTurnRequest, NativeRuntimeError> {
    if target.harness != AgentHarness::Alfred || target.provider != descriptor.provider {
        return Err(invalid_context(
            "native execution target does not match the runtime",
        ));
    }
    if descriptor.request_contract_version != NATIVE_REQUEST_CONTRACT_VERSION {
        return Err(invalid_context(
            "native runtime request contract version is unsupported",
        ));
    }
    event_limits.validate()?;
    reject_secret_bearing_request(request)?;
    let account_ref = target.account_ref.clone().ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "native execution requires an opaque account reference",
            false,
        )
    })?;
    let model = target
        .model
        .as_deref()
        .or(request.model.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::ModelUnavailable,
                "native execution requires an explicitly selected model",
                false,
            )
        })?
        .to_string();
    let working_directory = target
        .working_directory
        .as_deref()
        .or(request.working_directory.as_deref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let working_directory = if working_directory.is_absolute() {
        working_directory
    } else {
        std::env::current_dir()
            .map_err(|_| invalid_context("working directory is unavailable"))?
            .join(working_directory)
    };
    let context = resolve_context(request, target, context_policy)?;
    let cancellation = NativeCancellation::new(
        // A run/node pair repeats across loop iterations, so the handle carries
        // a per-turn nonce; the registry keys it by provider as well.
        format!(
            "{}:{}:{}",
            target.request_metadata.run_id,
            target.request_metadata.node_id,
            turn_nonce()
        ),
        DEFAULT_TURN_TIMEOUT,
    )?;
    Ok(NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: descriptor.runtime_version.clone(),
        provider: target.provider,
        account_ref,
        run_id: target.request_metadata.run_id.clone(),
        node_id: target.request_metadata.node_id.clone(),
        model,
        prompt: request.prompt.trim().into(),
        context,
        working_directory: working_directory.clone(),
        allowed_workspace_roots: vec![working_directory],
        permission_profile,
        tool_capabilities,
        session_mode: NativeSessionMode::Ephemeral,
        session_id: None,
        event_limits,
        timeout_ms: DEFAULT_TURN_TIMEOUT.as_millis() as u64,
        cancellation: Some(cancellation),
    })
}

fn resolve_context(
    request: &AgentRequest,
    target: &AgentExecutionTarget,
    policy: &NativeContextPolicy,
) -> Result<Vec<NativeContextBlock>, NativeRuntimeError> {
    let selected = request.resolved_skill_names();
    let mut blocks = Vec::new();
    if !selected.is_empty() {
        let root = target
            .working_directory
            .as_deref()
            .or(request.working_directory.as_deref());
        let discovered = list_skills(root).map_err(|_| {
            invalid_context("Alfred could not discover skills for the native request")
        })?;
        let mut used = HashSet::new();
        for requested in selected {
            let name = requested.trim().trim_start_matches('/');
            if !used.insert(name.to_string()) {
                continue;
            }
            let skill = discovered.iter().find(|skill| {
                skill.name == name
                    && skill
                        .providers
                        .iter()
                        .any(|provider| provider == target.provider.as_str())
            });
            let skill = skill.ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    format!("native skill is unavailable: {name}"),
                    false,
                )
            })?;
            let content = fs::read_to_string(&skill.path)
                .map_err(|_| invalid_context(format!("native skill could not be read: {name}")))?;
            if contains_secret_marker(&content) {
                return Err(invalid_context(format!(
                    "native skill contains secret-looking material: {name}"
                )));
            }
            blocks.push(NativeContextBlock {
                role: NativeContextRole::Skill,
                content,
                name: Some(name.into()),
            });
        }
    }
    blocks.push(NativeContextBlock {
        role: NativeContextRole::User,
        content: request.prompt.trim().to_string(),
        name: None,
    });
    policy.validate_blocks(&blocks)?;
    Ok(blocks)
}

fn reject_secret_bearing_request(request: &AgentRequest) -> Result<(), NativeRuntimeError> {
    if !request.extra.is_null() && request.extra != serde_json::json!({}) {
        return Err(invalid_context(
            "untyped CLI request fields are prohibited in native mode",
        ));
    }
    if contains_secret_marker(&request.prompt) {
        return Err(invalid_context(
            "secret-looking credentials are prohibited in native requests",
        ));
    }
    if contains_cli_permission_flag(&request.prompt) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "CLI permission flags cannot be inherited by native mode",
            false,
        ));
    }
    Ok(())
}

fn turn_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}",
        COUNTER.fetch_add(1, Ordering::Relaxed) ^ (std::process::id() as u64).rotate_left(32)
    )
}

fn invalid_context(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidRequest, message, false)
}
