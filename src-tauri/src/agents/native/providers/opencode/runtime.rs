//! Native OpenCode Go runtime over the pinned managed V1 server.

use super::account::{profile_ref_for_account, OPENCODE_GO_USAGE_URL};
use super::launch::{OpenCodeServerProvider, OpenCodeServerSession, OpenCodeServerState};
use super::package::{OPENCODE_RUNTIME_ID, OPENCODE_RUNTIME_VERSION};
use super::protocol::{
    build_prompt_body, build_session_body, parse_go_models, permission_policy, session_id,
    OpenCodeEventMapper, OpenCodePermissionReply, OpenCodePermissionRequest, OpenCodeProtocolEvent,
    OpenCodeRoute, OpenCodeSseDecoder,
};
use crate::agent_accounts::models::AgentProductId;
use crate::agents::native::{
    NativeAgentRuntime, NativeApprovalPolicy, NativeCapabilities, NativeErrorCode, NativeEvent,
    NativeEventKind, NativeModel, NativeRuntimeDescriptor, NativeRuntimeError, NativeSessionMode,
    NativeToolExecutionOwner, NativeTurnHost, NativeTurnOutcome, NativeTurnRequest,
    NativeUsageSnapshot, ResolvedNativeAccount, NATIVE_EVENT_CONTRACT_VERSION,
    NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::AgentProvider;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Host-decision seam for runtime-executed tools. The shared native registry
/// currently exposes approval decisions only through `invoke_tool`, which is
/// intentionally unavailable to this execution-owner mode. Integration must
/// provide this narrow callback before registration can be enabled.
pub trait OpenCodePermissionBroker: Send + Sync {
    fn decide(
        &self,
        request: &OpenCodePermissionRequest,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<OpenCodePermissionReply, NativeRuntimeError>;
}

pub struct OpenCodeNativeRuntime {
    servers: Arc<dyn OpenCodeServerProvider>,
    permissions: Arc<dyn OpenCodePermissionBroker>,
    catalog_repository: PathBuf,
}

impl OpenCodeNativeRuntime {
    pub fn new(
        servers: Arc<dyn OpenCodeServerProvider>,
        permissions: Arc<dyn OpenCodePermissionBroker>,
        catalog_repository: PathBuf,
    ) -> Result<Self, NativeRuntimeError> {
        if !catalog_repository.is_absolute() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "OpenCode catalog repository must be an explicit absolute directory",
                false,
            ));
        }
        Ok(Self {
            servers,
            permissions,
            catalog_repository,
        })
    }

    fn launch(
        &self,
        account: &ResolvedNativeAccount,
        repository: &Path,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        let profile_ref = profile_ref_for_account(account)?;
        self.servers
            .launch_existing(&account.account_ref, &profile_ref, repository, cancellation)
    }

    fn run_on_server(
        &self,
        server: &dyn OpenCodeServerSession,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let route = OpenCodeRoute::parse(&request.model)?;
        let directory = request.working_directory.to_str().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "OpenCode repository path is not valid UTF-8",
                false,
            )
        })?;
        let ephemeral = request.session_mode == NativeSessionMode::Ephemeral;
        let created = matches!(
            request.session_mode,
            NativeSessionMode::Ephemeral | NativeSessionMode::Start
        );
        let session = match request.session_mode {
            NativeSessionMode::Ephemeral | NativeSessionMode::Start => {
                let value = server.api().create_session(
                    directory,
                    &build_session_body(request, &route),
                    host.cancellation(),
                )?;
                session_id(&value, directory)?
            }
            NativeSessionMode::Resume => {
                let expected = request
                    .session_id
                    .as_deref()
                    .ok_or_else(session_unavailable)?;
                let value = server
                    .api()
                    .get_session(directory, expected, host.cancellation())?;
                let actual = session_id(&value, directory)?;
                if actual != expected {
                    return Err(session_unavailable());
                }
                actual
            }
            NativeSessionMode::Fork => {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::CapabilityUnsupported,
                    "OpenCode native mode resumes only the exact selected session",
                    false,
                ))
            }
        };

        let result = (|| {
            let mut started = NativeEvent::new(0, NativeEventKind::SessionStarted);
            started.session_id = Some(session.clone());
            host.emit(started)?;
            let mut turn_started = NativeEvent::new(0, NativeEventKind::TurnStarted);
            turn_started.session_id = Some(session.clone());
            host.emit(turn_started)?;

            // Establish the event stream before the async prompt so no fast
            // response or permission request can race past Alfred.
            let mut stream = server.api().subscribe(directory, host.cancellation())?;
            server.api().prompt_async(
                directory,
                &session,
                &build_prompt_body(request, &route),
                host.cancellation(),
            )?;

            let mut decoder = OpenCodeSseDecoder::default();
            let mut mapper = OpenCodeEventMapper::new(session.clone())?;
            let mut terminal = false;
            while !terminal {
                host.cancellation().checkpoint()?;
                let Some(chunk) = stream.next_chunk(host.cancellation())? else {
                    break;
                };
                for value in decoder.push(&chunk)? {
                    match mapper.map(value)? {
                        OpenCodeProtocolEvent::Connected
                        | OpenCodeProtocolEvent::SessionBusy
                        | OpenCodeProtocolEvent::Ignored => {}
                        OpenCodeProtocolEvent::AssistantDelta(event)
                        | OpenCodeProtocolEvent::ToolEvent(event) => host.emit(event)?,
                        OpenCodeProtocolEvent::PermissionAsked(permission) => {
                            self.resolve_permission(server, request, host, directory, permission)?;
                        }
                        OpenCodeProtocolEvent::PermissionReplied { .. } => {
                            // The decision was emitted after the authenticated
                            // reply succeeded. The bus echo is validation only.
                        }
                        OpenCodeProtocolEvent::SessionRetry => {
                            let mut warning = NativeEvent::new(0, NativeEventKind::Warning);
                            warning.session_id = Some(session.clone());
                            warning.text =
                                Some("OpenCode Go is retrying the model request.".into());
                            host.emit(warning)?;
                        }
                        OpenCodeProtocolEvent::SessionIdle => {
                            terminal = true;
                        }
                        OpenCodeProtocolEvent::SessionError(failure) => {
                            let error = failure.error();
                            if failure == super::protocol::OpenCodeGoFailure::Aborted {
                                let mut cancelled =
                                    NativeEvent::new(0, NativeEventKind::TurnCancelled);
                                cancelled.session_id = Some(session.clone());
                                host.emit(cancelled)?;
                            } else {
                                let mut failed = NativeEvent::new(0, NativeEventKind::TurnFailed);
                                failed.session_id = Some(session.clone());
                                failed.error = Some(error.message.clone());
                                host.emit(failed)?;
                            }
                            return Err(error);
                        }
                    }
                }
            }
            decoder.finish()?;
            if !terminal {
                return Err(match server.state() {
                    OpenCodeServerState::Active => NativeRuntimeError::new(
                        NativeErrorCode::ProviderUnavailable,
                        "OpenCode event stream ended before the session became idle",
                        true,
                    ),
                    OpenCodeServerState::Exited | OpenCodeServerState::Failed => {
                        NativeRuntimeError::new(
                            NativeErrorCode::ProviderUnavailable,
                            "OpenCode managed runtime crashed during the turn",
                            true,
                        )
                    }
                });
            }
            let mut completed = NativeEvent::new(0, NativeEventKind::TurnCompleted);
            completed.session_id = Some(session.clone());
            completed
                .metadata
                .insert("accountUsageState".into(), json!("unavailable"));
            completed.metadata.insert(
                "usageDeepLinkOnly".into(),
                Value::String(OPENCODE_GO_USAGE_URL.into()),
            );
            host.emit(completed)?;
            Ok(NativeTurnOutcome {
                session_id: if ephemeral {
                    None
                } else {
                    Some(session.clone())
                },
            })
        })();

        if result.is_err() {
            let _ = server.api().abort_session(directory, &session);
        }
        if ephemeral && created {
            let _ = server.api().delete_session(directory, &session);
        }
        result
    }

    fn resolve_permission(
        &self,
        server: &dyn OpenCodeServerSession,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
        directory: &str,
        permission: OpenCodePermissionRequest,
    ) -> Result<(), NativeRuntimeError> {
        host.cancellation().checkpoint()?;
        let mut requested = NativeEvent::new(0, NativeEventKind::ApprovalRequested);
        requested.session_id = Some(permission.session_id.clone());
        requested.approval_id = Some(permission.request_id.clone());
        requested.metadata.insert(
            "permissionKind".into(),
            Value::String(permission.permission.clone()),
        );
        requested.metadata.insert(
            "resourcePatterns".into(),
            json!(permission.patterns.clone()),
        );
        requested.metadata.insert(
            "alwaysResourcePatterns".into(),
            json!(permission.always_patterns.clone()),
        );
        host.emit(requested)?;

        let reply = match permission_policy(&permission.permission, request) {
            NativeApprovalPolicy::Deny => OpenCodePermissionReply::Reject,
            NativeApprovalPolicy::Allow => OpenCodePermissionReply::Once,
            NativeApprovalPolicy::Ask => {
                self.permissions.decide(&permission, host.cancellation())?
            }
        };
        host.cancellation().checkpoint()?;
        server.api().reply_permission(
            directory,
            &permission.request_id,
            reply,
            host.cancellation(),
        )?;
        let mut resolved = NativeEvent::new(0, NativeEventKind::ApprovalResolved);
        resolved.session_id = Some(permission.session_id);
        resolved.approval_id = Some(permission.request_id);
        resolved.approved = Some(reply.approved());
        host.emit(resolved)
    }
}

impl NativeAgentRuntime for OpenCodeNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: OPENCODE_RUNTIME_ID.into(),
            runtime_version: OPENCODE_RUNTIME_VERSION.into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: AgentProvider::Opencode,
            product: AgentProductId::OpencodeGo,
            tool_execution_owner: NativeToolExecutionOwner::RuntimeExecutedWithHostApproval,
            capabilities: NativeCapabilities {
                supports_api_key: true,
                supports_sessions: true,
                supports_resume: true,
                supports_model_list: true,
                supports_usage: false,
                supports_tool_calls: true,
                supports_approval_events: true,
                supports_native_filesystem: true,
                supports_native_shell: true,
                supports_patch: true,
                supports_mcp: false,
                supports_subagents: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        profile_ref_for_account(account).map(|_| ())
    }

    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        self.validate_account(account)?;
        let cancellation = crate::agents::native::NativeCancellation::new(
            "opencode_go_model_discovery",
            MODEL_DISCOVERY_TIMEOUT,
        )?;
        let server = self.launch(account, &self.catalog_repository, &cancellation)?;
        let directory = self.catalog_repository.to_str().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "OpenCode catalog repository is not valid UTF-8",
                false,
            )
        })?;
        let result = server
            .api()
            .list_providers(directory)
            .and_then(|value| parse_go_models(&value));
        let stop = server.stop();
        match (result, stop) {
            (Ok(models), Ok(())) => Ok(models),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        self.validate_account(account)?;
        OpenCodeRoute::parse(&request.model)?;
        host.cancellation().checkpoint()?;
        let server = self.launch(account, &request.working_directory, host.cancellation())?;
        let result = self.run_on_server(server.as_ref(), request, host);
        let stop = server.stop();
        match (result, stop) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn cancel(
        &self,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        cancellation.cancel();
        Ok(())
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        Ok(NativeUsageSnapshot::unavailable())
    }
}

fn session_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::SessionUnavailable,
        "OpenCode session is unavailable",
        false,
    )
}

impl OpenCodePermissionBroker for crate::agents::native::HostApprovalBroker {
    fn decide(
        &self,
        request: &OpenCodePermissionRequest,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<OpenCodePermissionReply, NativeRuntimeError> {
        let decision = self.decide_host(
            crate::agents::native::HostApprovalPrompt {
                request_id: request.request_id.clone(),
                session_id: Some(request.session_id.clone()),
                permission: request.permission.clone(),
                patterns: request.patterns.clone(),
                always_patterns: request.always_patterns.clone(),
                tool_call_id: request.tool_call_id.clone(),
            },
            cancellation,
        )?;
        Ok(match decision {
            crate::agents::native::HostApprovalDecision::Once => OpenCodePermissionReply::Once,
            crate::agents::native::HostApprovalDecision::Always => OpenCodePermissionReply::Always,
            crate::agents::native::HostApprovalDecision::Reject => OpenCodePermissionReply::Reject,
        })
    }
}
