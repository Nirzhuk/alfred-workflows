//! The single WhatsApp runtime for Alfred's lifetime (Plan 023 Step 5).
//!
//! One client, owned here, started at application startup when a ready
//! connection exists and stopped during orderly exit. It stays up while the app
//! or its tray process runs, and Alfred sends nothing while it is not running.
//!
//! The owner never hands out the protocol client. Its whole surface is status,
//! reconnect, send-to-self, logout, and shutdown. Lifecycle transitions and
//! sends share one async lock, so pairing, reconnect, send, logout, and shutdown
//! cannot interleave.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

use super::keyring;
use super::provider;
use super::runtime::{RuntimeError, RuntimeEvent, RuntimeHandle, RuntimeLauncher, SendReceipt};
use super::store::EncryptedProtocolStore;
use crate::integrations::token_store::TokenStore;

/// Safe runtime status. Carries no full JID, phone number, or protocol detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeStatus {
    /// No ready connection, or Alfred shut the runtime down.
    Stopped,
    Connecting,
    Connected { masked_account: String },
    Reconnecting,
    /// WhatsApp unlinked the device. Requires an acknowledged relink; the owner
    /// never silently starts a pairing flow.
    RelinkRequired,
    Error { code: String },
}

impl RuntimeStatus {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Connecting => "connecting",
            Self::Connected { .. } => "connected",
            Self::Reconnecting => "reconnecting",
            Self::RelinkRequired => "relink_required",
            Self::Error { .. } => "error",
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// What the owner needs to bring a stored connection back up.
#[derive(Debug, Clone)]
pub struct RuntimeTarget {
    pub credential_ref: String,
    pub store_path: String,
}

pub struct WhatsAppRuntimeOwner {
    /// Serializes every lifecycle transition and every send.
    handle: AsyncMutex<Option<Arc<dyn RuntimeHandle>>>,
    status: Arc<Mutex<RuntimeStatus>>,
    target: Mutex<Option<RuntimeTarget>>,
    launcher: Arc<dyn RuntimeLauncher>,
    tokens: Arc<dyn TokenStore>,
}

impl WhatsAppRuntimeOwner {
    pub fn new(tokens: Arc<dyn TokenStore>, launcher: Arc<dyn RuntimeLauncher>) -> Self {
        Self {
            handle: AsyncMutex::new(None),
            status: Arc::new(Mutex::new(RuntimeStatus::Stopped)),
            target: Mutex::new(None),
            launcher,
            tokens,
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        self.status.lock().expect("status lock").clone()
    }

    fn set_status(&self, status: RuntimeStatus) {
        *self.status.lock().expect("status lock") = status;
    }

    /// Brings the runtime up for a stored connection.
    ///
    /// Safe to call when already running: the existing client is stopped first,
    /// so there is never more than one.
    pub async fn start(&self, target: RuntimeTarget) -> Result<(), RuntimeError> {
        let mut slot = self.handle.lock().await;
        if let Some(existing) = slot.take() {
            existing.shutdown().await;
        }
        *self.target.lock().expect("target lock") = Some(target.clone());
        self.set_status(RuntimeStatus::Connecting);

        let key = keyring::load(self.tokens.as_ref(), &target.credential_ref).map_err(|_| {
            self.set_status(RuntimeStatus::Error {
                code: "credentials_unavailable".into(),
            });
            RuntimeError::Start
        })?;

        let store = Arc::new(
            EncryptedProtocolStore::open(&target.store_path, key).map_err(|_| {
                self.set_status(RuntimeStatus::Error {
                    code: "storage_unavailable".into(),
                });
                RuntimeError::Start
            })?,
        );

        // Expired retry payloads are purged on every startup, not only on the
        // maintenance interval.
        let _ = store.purge_expired().await;

        let (handle, mut events) = match self.launcher.launch(store).await {
            Ok(started) => started,
            Err(error) => {
                self.set_status(RuntimeStatus::Error {
                    code: "runtime_unavailable".into(),
                });
                return Err(error);
            }
        };
        *slot = Some(handle);
        drop(slot);

        // Only lifecycle events are observed. No message, history, contact,
        // media, call, presence, or profile payload is ever matched, forwarded,
        // or stored — the runtime does not emit them at all.
        let status = Arc::clone(&self.status);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    RuntimeEvent::Connected { own_jid } => {
                        *status.lock().expect("status lock") = RuntimeStatus::Connected {
                            masked_account: provider::masked_account(&own_jid),
                        };
                    }
                    RuntimeEvent::LoggedOut => {
                        *status.lock().expect("status lock") = RuntimeStatus::RelinkRequired;
                        break;
                    }
                    RuntimeEvent::Stopped => {
                        let mut status = status.lock().expect("status lock");
                        // A logged-out session must not be downgraded to a
                        // transient reconnect: it needs an acknowledged relink.
                        if *status != RuntimeStatus::RelinkRequired {
                            *status = RuntimeStatus::Reconnecting;
                        }
                        break;
                    }
                    // A pairing code during normal operation means the stored
                    // session is gone. Never pair silently.
                    RuntimeEvent::QrCode { .. } => {
                        *status.lock().expect("status lock") = RuntimeStatus::RelinkRequired;
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// One bounded restart attempt against the stored target.
    pub async fn reconnect(&self) -> Result<(), RuntimeError> {
        if self.status() == RuntimeStatus::RelinkRequired {
            // A revoked session cannot be reconnected, only relinked.
            return Err(RuntimeError::LoggedOut);
        }
        let target = self
            .target
            .lock()
            .expect("target lock")
            .clone()
            .ok_or(RuntimeError::Disconnected)?;
        self.set_status(RuntimeStatus::Reconnecting);
        self.start(target).await
    }

    /// Sends to the paired account's own chat.
    ///
    /// Makes one bounded reconnect attempt when disconnected. A logged-out
    /// session is never reconnected — it needs a relink.
    pub async fn send_self_message(&self, body: &str) -> Result<SendReceipt, RuntimeError> {
        if self.status() == RuntimeStatus::RelinkRequired {
            return Err(RuntimeError::LoggedOut);
        }

        let handle = { self.handle.lock().await.clone() };
        let handle = match handle {
            Some(handle) => handle,
            None => {
                self.reconnect().await?;
                self.handle
                    .lock()
                    .await
                    .clone()
                    .ok_or(RuntimeError::Disconnected)?
            }
        };

        handle.send_self_message(body).await
    }

    /// Best-effort removal from WhatsApp's Linked Devices.
    pub async fn logout(&self) -> Result<(), RuntimeError> {
        let handle = { self.handle.lock().await.clone() };
        match handle {
            Some(handle) => handle.logout().await,
            None => Err(RuntimeError::Disconnected),
        }
    }

    /// Stops the client and flushes protocol state. Idempotent.
    pub async fn shutdown(&self) {
        let handle = self.handle.lock().await.take();
        if let Some(handle) = handle {
            handle.shutdown().await;
        }
        // A revoked session stays revoked across a shutdown.
        if self.status() != RuntimeStatus::RelinkRequired {
            self.set_status(RuntimeStatus::Stopped);
        }
        *self.target.lock().expect("target lock") = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::token_store::InMemoryTokenStore;
    use crate::integrations::whatsapp::crypto::StoreKey;
    use crate::integrations::whatsapp::runtime::fake::FakeRuntime;

    const OWN_JID: &str = "34600123456@s.whatsapp.net";

    struct Fixture {
        dir: std::path::PathBuf,
        tokens: Arc<InMemoryTokenStore>,
        target: RuntimeTarget,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "alfred-whatsapp-owner-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");

            let tokens = Arc::new(InMemoryTokenStore::default());
            let provisioned = keyring::provision(tokens.as_ref()).expect("provision");
            let store_path = dir.join("protocol.db");
            EncryptedProtocolStore::open(&store_path, provisioned.key).expect("store");

            Self {
                target: RuntimeTarget {
                    credential_ref: provisioned.credential_ref,
                    store_path: store_path.display().to_string(),
                },
                tokens,
                dir,
            }
        }

        fn owner(&self, launcher: Arc<FakeRuntime>) -> WhatsAppRuntimeOwner {
            WhatsAppRuntimeOwner::new(self.tokens.clone(), launcher)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn receipt() -> SendReceipt {
        SendReceipt {
            message_id: "3EB0".into(),
            submitted_at: "2026-08-19T00:00:00Z".into(),
            masked_destination: "***56@s.whatsapp.net".into(),
        }
    }

    #[tokio::test]
    async fn a_stopped_owner_reports_stopped() {
        let fixture = Fixture::new("stopped");
        let owner = fixture.owner(Arc::new(FakeRuntime::default()));
        assert_eq!(owner.status(), RuntimeStatus::Stopped);
    }

    #[tokio::test]
    async fn starting_unlocks_the_store_and_launches_once() {
        let fixture = Fixture::new("start");
        let launcher = Arc::new(FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: OWN_JID.into(),
        }]));
        let owner = fixture.owner(launcher.clone());

        owner.start(fixture.target.clone()).await.expect("starts");
        assert_eq!(
            launcher.calls().iter().filter(|c| *c == "launch").count(),
            1
        );

        owner.shutdown().await;
        assert_eq!(owner.status(), RuntimeStatus::Stopped);
        assert!(launcher.calls().contains(&"shutdown".to_string()));
    }

    /// Regression: the event pump originally updated a detached copy of the
    /// status, so a live connection never surfaced to callers.
    #[tokio::test]
    async fn lifecycle_events_reach_the_owners_status() {
        let fixture = Fixture::new("events");
        let launcher = Arc::new(FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: OWN_JID.into(),
        }]));
        let owner = fixture.owner(launcher);

        owner.start(fixture.target.clone()).await.expect("starts");
        wait_for(&owner, |status| status.is_connected()).await;

        assert_eq!(
            owner.status(),
            RuntimeStatus::Connected {
                masked_account: "***56@s.whatsapp.net".into()
            }
        );
        owner.shutdown().await;
    }

    #[tokio::test]
    async fn a_remote_unlink_moves_the_owner_to_relink_required() {
        let fixture = Fixture::new("unlink");
        let launcher = Arc::new(FakeRuntime::with_script(vec![RuntimeEvent::LoggedOut]));
        let owner = fixture.owner(launcher);

        owner.start(fixture.target.clone()).await.expect("starts");
        wait_for(&owner, |status| *status == RuntimeStatus::RelinkRequired).await;

        // And it never silently pairs again.
        assert_eq!(owner.reconnect().await.unwrap_err(), RuntimeError::LoggedOut);
    }

    #[tokio::test]
    async fn an_unexpected_pairing_code_is_treated_as_a_relink() {
        let fixture = Fixture::new("stray-qr");
        let launcher = Arc::new(FakeRuntime::with_script(vec![RuntimeEvent::QrCode {
            payload: "QR".into(),
            expires_in_seconds: 60,
        }]));
        let owner = fixture.owner(launcher);

        owner.start(fixture.target.clone()).await.expect("starts");
        wait_for(&owner, |status| *status == RuntimeStatus::RelinkRequired).await;
    }

    /// Polls the owner's status until `predicate` holds. The pump runs on its
    /// own task, so a direct assertion would race it.
    async fn wait_for(
        owner: &WhatsAppRuntimeOwner,
        predicate: impl Fn(&RuntimeStatus) -> bool,
    ) {
        for _ in 0..200 {
            if predicate(&owner.status()) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("status never satisfied the predicate: {:?}", owner.status());
    }

    #[tokio::test]
    async fn starting_twice_never_leaves_two_clients() {
        let fixture = Fixture::new("restart");
        let launcher = Arc::new(FakeRuntime::default());
        let owner = fixture.owner(launcher.clone());

        owner.start(fixture.target.clone()).await.expect("first");
        owner.start(fixture.target.clone()).await.expect("second");

        // The first client is shut down before the second launches.
        assert_eq!(
            launcher.calls().iter().filter(|c| *c == "shutdown").count(),
            1
        );
        owner.shutdown().await;
    }

    #[tokio::test]
    async fn a_missing_credential_reports_an_error_rather_than_panicking() {
        let fixture = Fixture::new("nocred");
        let owner = fixture.owner(Arc::new(FakeRuntime::default()));

        let error = owner
            .start(RuntimeTarget {
                credential_ref: "whatsapp-protocol-store/missing".into(),
                store_path: fixture.target.store_path.clone(),
            })
            .await
            .expect_err("no key means no runtime");

        assert_eq!(error, RuntimeError::Start);
        assert_eq!(
            owner.status(),
            RuntimeStatus::Error {
                code: "credentials_unavailable".into()
            }
        );
    }

    #[tokio::test]
    async fn a_failed_launch_surfaces_as_an_error_state() {
        let fixture = Fixture::new("launchfail");
        let launcher = Arc::new(FakeRuntime::default());
        *launcher.launch_fails.lock().unwrap() = true;
        let owner = fixture.owner(launcher);

        owner
            .start(fixture.target.clone())
            .await
            .expect_err("launch fails");
        assert_eq!(
            owner.status(),
            RuntimeStatus::Error {
                code: "runtime_unavailable".into()
            }
        );
    }

    #[tokio::test]
    async fn sending_reconnects_once_when_no_client_is_up() {
        let fixture = Fixture::new("sendreconnect");
        let launcher = Arc::new(FakeRuntime::default());
        *launcher.send_result.lock().unwrap() = Some(Ok(receipt()));
        let owner = fixture.owner(launcher.clone());

        // Prime the target without leaving a client running.
        owner.start(fixture.target.clone()).await.expect("start");
        *owner.handle.lock().await = None;

        owner.send_self_message("hello").await.expect("sends");
        assert!(launcher.calls().contains(&"send".to_string()));
    }

    #[tokio::test]
    async fn a_send_without_a_target_fails_instead_of_pairing() {
        let fixture = Fixture::new("notarget");
        let owner = fixture.owner(Arc::new(FakeRuntime::default()));

        let error = owner.send_self_message("hello").await.unwrap_err();
        assert_eq!(error, RuntimeError::Disconnected);
    }

    #[tokio::test]
    async fn a_relink_required_runtime_refuses_to_send_or_reconnect() {
        let fixture = Fixture::new("relink");
        let owner = fixture.owner(Arc::new(FakeRuntime::default()));
        owner.set_status(RuntimeStatus::RelinkRequired);

        assert_eq!(
            owner.send_self_message("hello").await.unwrap_err(),
            RuntimeError::LoggedOut
        );
        assert_eq!(owner.reconnect().await.unwrap_err(), RuntimeError::LoggedOut);
        // And it survives a shutdown, so restart does not silently re-pair.
        owner.shutdown().await;
        assert_eq!(owner.status(), RuntimeStatus::RelinkRequired);
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let fixture = Fixture::new("idempotent");
        let owner = fixture.owner(Arc::new(FakeRuntime::default()));
        owner.start(fixture.target.clone()).await.expect("start");

        owner.shutdown().await;
        owner.shutdown().await;
        assert_eq!(owner.status(), RuntimeStatus::Stopped);
    }

    #[tokio::test]
    async fn status_serializes_without_identity() {
        let status = RuntimeStatus::Connected {
            masked_account: provider::masked_account(OWN_JID),
        };
        let json = serde_json::to_string(&status).unwrap();

        assert!(json.contains("connected"));
        assert!(json.contains("***56@s.whatsapp.net"));
        assert!(!json.contains("34600123456"));
    }

    #[test]
    fn every_status_has_a_stable_code() {
        for (status, code) in [
            (RuntimeStatus::Stopped, "stopped"),
            (RuntimeStatus::Connecting, "connecting"),
            (
                RuntimeStatus::Connected {
                    masked_account: "***56@s.whatsapp.net".into(),
                },
                "connected",
            ),
            (RuntimeStatus::Reconnecting, "reconnecting"),
            (RuntimeStatus::RelinkRequired, "relink_required"),
            (
                RuntimeStatus::Error {
                    code: "runtime_unavailable".into(),
                },
                "error",
            ),
        ] {
            assert_eq!(status.code(), code);
        }
    }
}
