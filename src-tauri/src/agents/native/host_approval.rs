//! Host-owned approval broker for runtime-executed tools.
//!
//! Alfred decides; the provider runtime executes. The broker waits for an
//! explicit Once / Always / Reject decision and never infers approval from a
//! missing UI, a timeout, or a serialized `approved: true` flag.

use super::{
    AlfredApprovalDecision, AlfredApprovalHandler, AlfredApprovalRequest, NativeCancellation,
    NativeErrorCode, NativeRuntimeError,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostApprovalDecision {
    Once,
    Always,
    Reject,
}

impl HostApprovalDecision {
    pub fn parse(value: &str) -> Result<Self, NativeRuntimeError> {
        match value {
            "once" => Ok(Self::Once),
            "always" => Ok(Self::Always),
            "reject" => Ok(Self::Reject),
            _ => Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "host approval decision is invalid",
                false,
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }

    pub fn approved(self) -> bool {
        !matches!(self, Self::Reject)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostApprovalPrompt {
    pub request_id: String,
    pub session_id: Option<String>,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always_patterns: Vec<String>,
    pub tool_call_id: Option<String>,
}

struct BrokerState {
    pending: HashMap<String, Option<HostApprovalDecision>>,
    remembered: HashSet<String>,
}

pub struct HostApprovalBroker {
    inner: Mutex<BrokerState>,
    signal: Condvar,
    listener: Mutex<Option<Arc<dyn Fn(HostApprovalPrompt) + Send + Sync>>>,
}

impl HostApprovalBroker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BrokerState {
                pending: HashMap::new(),
                remembered: HashSet::new(),
            }),
            signal: Condvar::new(),
            listener: Mutex::new(None),
        }
    }

    pub fn set_listener(&self, listener: Arc<dyn Fn(HostApprovalPrompt) + Send + Sync>) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(listener);
        }
    }

    pub fn decide_host(
        &self,
        prompt: HostApprovalPrompt,
        cancellation: &NativeCancellation,
    ) -> Result<HostApprovalDecision, NativeRuntimeError> {
        if prompt.request_id.is_empty()
            || prompt.request_id.len() > 128
            || prompt.permission.is_empty()
            || prompt.permission.len() > 128
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "host approval request identity is invalid",
                false,
            ));
        }
        let remember_key = remember_key(&prompt);
        {
            let state = self.inner.lock().map_err(|_| broker_unavailable())?;
            if state.remembered.contains(&remember_key) {
                return Ok(HostApprovalDecision::Always);
            }
        }

        {
            let mut state = self.inner.lock().map_err(|_| broker_unavailable())?;
            if state.pending.contains_key(&prompt.request_id) {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    "host approval request is already pending",
                    false,
                ));
            }
            state.pending.insert(prompt.request_id.clone(), None);
        }

        if let Ok(listener) = self.listener.lock() {
            if let Some(listener) = listener.as_ref() {
                listener(prompt.clone());
            }
        }

        loop {
            cancellation.checkpoint()?;
            let state = self.inner.lock().map_err(|_| broker_unavailable())?;
            match state.pending.get(&prompt.request_id).copied() {
                Some(Some(decision)) => {
                    let mut state = state;
                    state.pending.remove(&prompt.request_id);
                    if decision == HostApprovalDecision::Always {
                        state.remembered.insert(remember_key);
                    }
                    return Ok(decision);
                }
                Some(None) => {
                    let result = self
                        .signal
                        .wait_timeout(state, POLL_INTERVAL)
                        .map_err(|_| broker_unavailable())?;
                    drop(result);
                }
                None => {
                    return Err(NativeRuntimeError::new(
                        NativeErrorCode::PermissionDenied,
                        "host approval request was cancelled",
                        false,
                    ));
                }
            }
        }
    }

    pub fn resolve(
        &self,
        request_id: &str,
        decision: HostApprovalDecision,
    ) -> Result<(), NativeRuntimeError> {
        let mut state = self.inner.lock().map_err(|_| broker_unavailable())?;
        match state.pending.get_mut(request_id) {
            Some(slot) if slot.is_none() => {
                *slot = Some(decision);
                self.signal.notify_all();
                Ok(())
            }
            Some(_) => Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "host approval request was already resolved",
                false,
            )),
            None => Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "host approval request is not pending",
                false,
            )),
        }
    }
}

impl Default for HostApprovalBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl AlfredApprovalHandler for HostApprovalBroker {
    fn decide(
        &self,
        request: &AlfredApprovalRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
        let decision = self.decide_host(
            HostApprovalPrompt {
                request_id: request.approval_id.clone(),
                session_id: None,
                permission: request.tool_name.clone(),
                patterns: Vec::new(),
                always_patterns: Vec::new(),
                tool_call_id: Some(request.tool_request_id.clone()),
            },
            cancellation,
        )?;
        Ok(if decision.approved() {
            AlfredApprovalDecision::Allow
        } else {
            AlfredApprovalDecision::Deny
        })
    }
}

fn remember_key(prompt: &HostApprovalPrompt) -> String {
    let patterns = if prompt.always_patterns.is_empty() {
        &prompt.patterns
    } else {
        &prompt.always_patterns
    };
    format!("{}\u{1f}{}", prompt.permission, patterns.join("\u{1f}"))
}

fn broker_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "host approval broker is unavailable",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn cancellation() -> NativeCancellation {
        NativeCancellation::new("approval_test", Duration::from_secs(2)).expect("cancellation")
    }

    fn prompt(id: &str, permission: &str) -> HostApprovalPrompt {
        HostApprovalPrompt {
            request_id: id.into(),
            session_id: Some("session_fixture".into()),
            permission: permission.into(),
            patterns: vec!["src/**".into()],
            always_patterns: vec!["src/**".into()],
            tool_call_id: None,
        }
    }

    #[test]
    fn once_always_and_reject_are_distinct_and_cancel_fails_closed() {
        let broker = Arc::new(HostApprovalBroker::new());
        let once_broker = Arc::clone(&broker);
        let once_handle = thread::spawn(move || {
            once_broker.decide_host(prompt("req_once", "edit"), &cancellation())
        });
        thread::sleep(Duration::from_millis(20));
        broker
            .resolve("req_once", HostApprovalDecision::Once)
            .expect("resolve once");
        assert_eq!(
            once_handle.join().expect("once thread"),
            Ok(HostApprovalDecision::Once)
        );

        let always_broker = Arc::clone(&broker);
        let always_handle = thread::spawn(move || {
            always_broker.decide_host(prompt("req_always", "edit"), &cancellation())
        });
        thread::sleep(Duration::from_millis(20));
        broker
            .resolve("req_always", HostApprovalDecision::Always)
            .expect("resolve always");
        assert_eq!(
            always_handle.join().expect("always thread"),
            Ok(HostApprovalDecision::Always)
        );
        assert_eq!(
            broker
                .decide_host(prompt("req_remembered", "edit"), &cancellation())
                .expect("remembered always"),
            HostApprovalDecision::Always
        );

        let reject_broker = Arc::clone(&broker);
        let reject_handle = thread::spawn(move || {
            reject_broker.decide_host(prompt("req_reject", "bash"), &cancellation())
        });
        thread::sleep(Duration::from_millis(20));
        broker
            .resolve("req_reject", HostApprovalDecision::Reject)
            .expect("resolve reject");
        assert_eq!(
            reject_handle.join().expect("reject thread"),
            Ok(HostApprovalDecision::Reject)
        );

        let cancelled = NativeCancellation::new("approval_cancel", Duration::from_secs(2))
            .expect("cancellation");
        cancelled.cancel();
        let error = broker
            .decide_host(prompt("req_cancel", "read"), &cancelled)
            .expect_err("cancelled approval");
        assert_eq!(error.code, NativeErrorCode::Cancelled);
    }
}
