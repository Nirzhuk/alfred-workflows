//! Runtime boundary for the WhatsApp linked device (Plan 023 Steps 4 and 5).
//!
//! The protocol client is never handed to commands, workflows, or the frontend.
//! Everything they may do goes through [`RuntimeHandle`], which exposes exactly
//! four capabilities: read the authenticated own JID, send to that account's own
//! chat, log out, and shut down.
//!
//! The trait exists so the pairing state machine and the runtime owner can be
//! tested deterministically against [`FakeRuntime`], with no live account, no
//! network, and no committed device state.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;

use super::store::EncryptedProtocolStore;

/// Safe lifecycle signals. Deliberately carries no message, history, contact,
/// media, call, presence, or profile payload — see Plan 023 Step 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// A short-lived pairing code. Streamed to the open modal and never
    /// persisted, logged, or reused once superseded.
    QrCode {
        payload: String,
        expires_in_seconds: u64,
    },
    /// Authenticated. Carries the own JID so the backend can derive the
    /// self-chat destination without ever asking the frontend for one.
    Connected { own_jid: String },
    /// The device was unlinked remotely. Requires a fresh acknowledged pairing;
    /// never auto-restart one.
    LoggedOut,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeError {
    #[error("the WhatsApp runtime could not be started")]
    Start,
    #[error("the linked device is logged out and must be relinked")]
    LoggedOut,
    #[error("the WhatsApp runtime is not connected")]
    Disconnected,
    #[error("the message was rejected before dispatch")]
    Rejected(String),
    /// The send may or may not have reached WhatsApp. Maps to the shared
    /// `delivery_unknown` action error and must never be retried automatically.
    #[error("the message may have been delivered")]
    DeliveryUnknown,
}

impl RuntimeError {
    /// Whether the caller may safely try again. A possibly dispatched send is
    /// never retryable (Plan 023 STOP condition).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Start | Self::Disconnected | Self::Rejected(_) => true,
            Self::LoggedOut | Self::DeliveryUnknown => false,
        }
    }
}

/// What a caller learns about a submitted message. Never the body, the own JID,
/// a raw frame, or any receipt detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendReceipt {
    pub message_id: String,
    pub submitted_at: String,
    pub masked_destination: String,
}

/// Bounded capabilities over one connected client.
#[async_trait]
pub trait RuntimeHandle: Send + Sync {
    /// The authenticated own JID, once connected.
    fn own_jid(&self) -> Option<String>;

    /// Sends plain text to the paired account's own chat. The destination is
    /// derived internally; there is no recipient parameter by design.
    async fn send_self_message(&self, body: &str) -> Result<SendReceipt, RuntimeError>;

    /// Best-effort removal from WhatsApp's Linked Devices.
    async fn logout(&self) -> Result<(), RuntimeError>;

    /// Stops the client and flushes required protocol state. Idempotent: the
    /// pairing and disconnect paths may both reach it.
    async fn shutdown(&self);
}

/// Creates runtimes. The pairing flow and the long-lived owner both go through
/// this, so tests can substitute [`FakeRuntime`] wholesale.
#[async_trait]
pub trait RuntimeLauncher: Send + Sync {
    async fn launch(
        &self,
        store: Arc<EncryptedProtocolStore>,
    ) -> Result<(Arc<dyn RuntimeHandle>, mpsc::Receiver<RuntimeEvent>), RuntimeError>;
}

/// The real runtime, bound to `whatsapp-rust`.
///
/// This is the only place the protocol client exists. It is never returned,
/// cloned out, or exposed through a command.
#[derive(Debug, Default)]
pub struct WhatsAppLauncher;

#[async_trait]
impl RuntimeLauncher for WhatsAppLauncher {
    async fn launch(
        &self,
        store: Arc<EncryptedProtocolStore>,
    ) -> Result<(Arc<dyn RuntimeHandle>, mpsc::Receiver<RuntimeEvent>), RuntimeError> {
        use whatsapp_rust::prelude::*;

        let (sender, receiver) = mpsc::channel(8);
        let qr_sender = sender.clone();
        let connected_sender = sender.clone();
        let logged_out_sender = sender.clone();

        let bot = Bot::builder()
            .with_backend_arc(store)
            // Decline history sync at the protocol level so blobs are never
            // received, rather than received and discarded.
            .skip_history_sync()
            // Plan 023 Step 6 caps sends at five per minute.
            .with_resend_rate_limit(5, 5)
            .on_qr_code(move |code, timeout| {
                let sender = qr_sender.clone();
                async move {
                    // The payload goes to the open modal and nowhere else. It is
                    // deliberately never logged.
                    let _ = sender
                        .send(RuntimeEvent::QrCode {
                            payload: code,
                            expires_in_seconds: timeout.as_secs(),
                        })
                        .await;
                }
            })
            .on_connected(move |client| {
                let sender = connected_sender.clone();
                async move {
                    if let Some(pn) = client.pn() {
                        let _ = sender
                            .send(RuntimeEvent::Connected {
                                own_jid: pn.to_string(),
                            })
                            .await;
                    }
                }
            })
            .on_logged_out(move |_info| {
                let sender = logged_out_sender.clone();
                async move {
                    let _ = sender.send(RuntimeEvent::LoggedOut).await;
                }
            })
            .build()
            .await
            .map_err(|_| RuntimeError::Start)?;

        // No `on_message` handler is registered anywhere: inbound content is
        // never observed, decoded, or persisted (Plan 023 Step 5).
        let handle = WhatsAppHandle {
            handle: std::sync::Mutex::new(Some(bot.spawn())),
        };
        Ok((Arc::new(handle), receiver))
    }
}

struct WhatsAppHandle {
    handle: std::sync::Mutex<Option<whatsapp_rust::prelude::BotHandle>>,
}

impl WhatsAppHandle {
    fn client(&self) -> Option<Arc<whatsapp_rust::prelude::Client>> {
        self.handle
            .lock()
            .ok()?
            .as_ref()
            .map(|handle| handle.client())
    }
}

/// Classifies a send failure by whether the stanza can already have left.
///
/// Only failures that demonstrably happened before dispatch are retryable; an
/// unknown stage is treated as possibly delivered, because retrying a
/// dispatched message is a Plan 023 STOP condition.
fn classify_send_error(error: &whatsapp_rust::prelude::SendError) -> RuntimeError {
    use whatsapp_rust::prelude::SendError;
    match error {
        // Validation rejected the request; nothing was ever sent.
        SendError::InvalidRequest(_) => RuntimeError::Rejected("invalid request".into()),
        // Not paired, or mid identity migration.
        SendError::NotLoggedIn => RuntimeError::LoggedOut,
        // The pre-send device-list query failed, so the message stanza never
        // went out.
        SendError::Iq(_) => RuntimeError::Disconnected,
        // Transport loss or an unclassified internal failure: the stanza may
        // already be on the wire.
        SendError::Client(_) | SendError::Internal(_) => RuntimeError::DeliveryUnknown,
        _ => RuntimeError::DeliveryUnknown,
    }
}

#[async_trait]
impl RuntimeHandle for WhatsAppHandle {
    fn own_jid(&self) -> Option<String> {
        self.client()?.pn().map(|pn| pn.to_string())
    }

    async fn send_self_message(&self, body: &str) -> Result<SendReceipt, RuntimeError> {
        use whatsapp_rust::prelude::*;

        let client = self.client().ok_or(RuntimeError::Disconnected)?;
        let pn = client.pn().ok_or(RuntimeError::LoggedOut)?;
        // The destination is derived here and nowhere else. No caller, workflow,
        // or frontend value can influence it.
        let self_jid: Jid = pn.to_non_ad();
        let masked_destination = super::provider::masked_account(&self_jid.to_string());

        let sent = client
            .send_message(self_jid, wa::Message::text(body))
            .await
            .map_err(|error| classify_send_error(&error))?;

        Ok(SendReceipt {
            message_id: sent.message_id,
            submitted_at: chrono::Utc::now().to_rfc3339(),
            masked_destination,
        })
    }

    async fn logout(&self) -> Result<(), RuntimeError> {
        let client = self.client().ok_or(RuntimeError::Disconnected)?;
        client.logout().await;
        Ok(())
    }

    async fn shutdown(&self) {
        let handle = self.handle.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            handle.shutdown().await;
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Scripted runtime for tests. Emits the events it is given and records
    /// every call, so a state machine can be driven through cancellation,
    /// expiry, logout, and ambiguous-send paths without a live account.
    #[derive(Default)]
    pub struct FakeRuntime {
        pub script: Mutex<Vec<RuntimeEvent>>,
        pub send_result: Mutex<Option<Result<SendReceipt, RuntimeError>>>,
        pub launch_fails: Mutex<bool>,
        pub calls: Arc<Mutex<Vec<String>>>,
    }

    impl FakeRuntime {
        pub fn with_script(events: Vec<RuntimeEvent>) -> Self {
            Self {
                script: Mutex::new(events),
                ..Default::default()
            }
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RuntimeLauncher for FakeRuntime {
        async fn launch(
            &self,
            _store: Arc<EncryptedProtocolStore>,
        ) -> Result<(Arc<dyn RuntimeHandle>, mpsc::Receiver<RuntimeEvent>), RuntimeError> {
            self.calls.lock().unwrap().push("launch".into());
            if *self.launch_fails.lock().unwrap() {
                return Err(RuntimeError::Start);
            }

            let events = std::mem::take(&mut *self.script.lock().unwrap());
            let own_jid = events.iter().find_map(|event| match event {
                RuntimeEvent::Connected { own_jid } => Some(own_jid.clone()),
                _ => None,
            });

            let (sender, receiver) = mpsc::channel(16);
            for event in events {
                // The channel is sized for every scripted event, so this cannot
                // block or drop.
                sender.try_send(event).expect("fake runtime channel");
            }

            // Dropping the sender lets the receiver drain the queued events and
            // then report end-of-stream, which is how a script that never links
            // reaches the "pairing incomplete" path instead of hanging.
            drop(sender);
            let handle = FakeHandle {
                own_jid,
                send_result: Mutex::new(self.send_result.lock().unwrap().clone()),
                calls: Arc::clone(&self.calls),
            };
            Ok((Arc::new(handle), receiver))
        }
    }

    pub struct FakeHandle {
        own_jid: Option<String>,
        send_result: Mutex<Option<Result<SendReceipt, RuntimeError>>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RuntimeHandle for FakeHandle {
        fn own_jid(&self) -> Option<String> {
            self.own_jid.clone()
        }

        async fn send_self_message(&self, _body: &str) -> Result<SendReceipt, RuntimeError> {
            self.calls.lock().unwrap().push("send".into());
            self.send_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(Err(RuntimeError::Disconnected))
        }

        async fn logout(&self) -> Result<(), RuntimeError> {
            self.calls.lock().unwrap().push("logout".into());
            Ok(())
        }

        async fn shutdown(&self) {
            self.calls.lock().unwrap().push("shutdown".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ambiguous_send_is_never_retryable() {
        assert!(!RuntimeError::DeliveryUnknown.is_retryable());
        assert!(!RuntimeError::LoggedOut.is_retryable());
    }

    #[test]
    fn definitive_pre_dispatch_failures_are_retryable() {
        assert!(RuntimeError::Disconnected.is_retryable());
        assert!(RuntimeError::Start.is_retryable());
        assert!(RuntimeError::Rejected("empty".into()).is_retryable());
    }

    #[test]
    fn runtime_errors_never_echo_a_message_body() {
        // `Rejected` carries a reason, so make sure the Display impl exposes the
        // category and not the caller's text.
        let error = RuntimeError::Rejected("SENTINEL-BODY".into());
        assert!(!error.to_string().contains("SENTINEL-BODY"));
    }
}
