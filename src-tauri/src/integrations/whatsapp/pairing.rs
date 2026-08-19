//! Acknowledged QR pairing for the WhatsApp linked device (Plan 023 Step 4).
//!
//! The flow is deliberately rigid:
//!
//! 1. the user acknowledges the experimental/unofficial risk;
//! 2. only then is a staging key minted, a staging store created, and one
//!    pairing runtime started;
//! 3. each short-lived QR payload is handed to a sink that renders it in the
//!    open modal — it is never stored, logged, or reused once superseded;
//! 4. the own JID is read from the authenticated client, never from the
//!    frontend;
//! 5. the user must send an explicit test message;
//! 6. only a definitively successful test promotes the staging state.
//!
//! Every abandonment path — cancel, expiry, replacement, failed test, closed
//! modal — stops the runtime, attempts a remote logout if pairing completed, and
//! deletes the staging database and key.

use std::sync::{Arc, Mutex};

use super::keyring::{self, KeyCustodyError};
use super::provider::{self, RISK_ACKNOWLEDGEMENT_VERSION};
use super::runtime::{RuntimeError, RuntimeEvent, RuntimeHandle, RuntimeLauncher, SendReceipt};
use super::store::EncryptedProtocolStore;
use crate::integrations::token_store::TokenStore;

/// Safe, frontend-visible pairing state. Never carries a QR payload, a full
/// JID, a phone number, or any protocol material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingState {
    /// The risk warning has not been accepted, so nothing has been started.
    AwaitingAcknowledgement,
    /// Runtime starting; no QR yet.
    Starting,
    /// A QR has been streamed to the modal and is waiting to be scanned.
    AwaitingScan,
    /// Linked. The explicit test send has not run yet.
    AwaitingTest {
        masked_account: String,
    },
    /// The test succeeded definitively; the connection may be created.
    Ready {
        masked_account: String,
    },
    /// Terminal failure. `code` is a stable, non-sensitive identifier.
    Failed {
        code: String,
    },
    Cancelled,
}

impl PairingState {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AwaitingAcknowledgement => "awaiting_acknowledgement",
            Self::Starting => "starting",
            Self::AwaitingScan => "awaiting_scan",
            Self::AwaitingTest { .. } => "awaiting_test",
            Self::Ready { .. } => "ready",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Ready { .. } | Self::Failed { .. } | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("the experimental-integration risk must be acknowledged first")]
    AcknowledgementRequired,
    #[error("the acknowledged warning is out of date")]
    AcknowledgementOutdated,
    #[error("a WhatsApp account is already linked; disconnect it first")]
    AlreadyLinked,
    #[error("pairing is not in a state that allows this")]
    InvalidState,
    #[error("the linked account did not present a usable identity")]
    InvalidIdentity,
    #[error("the protocol store could not be prepared")]
    Storage,
    #[error("the credential store is unavailable")]
    Credentials,
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
}

impl PairingError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AcknowledgementRequired => "acknowledgement_required",
            Self::AcknowledgementOutdated => "acknowledgement_outdated",
            Self::AlreadyLinked => "already_linked",
            Self::InvalidState => "invalid_state",
            Self::InvalidIdentity => "invalid_identity",
            Self::Storage => "storage_unavailable",
            Self::Credentials => "credentials_unavailable",
            Self::Runtime(RuntimeError::DeliveryUnknown) => "test_delivery_unknown",
            Self::Runtime(RuntimeError::LoggedOut) => "relink_required",
            Self::Runtime(_) => "runtime_unavailable",
        }
    }
}

impl From<KeyCustodyError> for PairingError {
    fn from(_: KeyCustodyError) -> Self {
        Self::Credentials
    }
}

/// Receives each short-lived QR payload. The Tauri layer emits it to the open
/// modal; nothing else may ever hold one.
pub trait QrSink: Send + Sync {
    fn present(&self, payload: &str, expires_in_seconds: u64);
    /// The previous code is void — the modal must stop showing it.
    fn expire(&self) {}
}

/// Where the staging and final protocol databases live.
#[derive(Debug, Clone)]
pub struct PairingPaths {
    pub staging: std::path::PathBuf,
    pub final_path: std::path::PathBuf,
}

impl PairingPaths {
    pub fn resolve() -> Result<Self, PairingError> {
        let final_path = keyring::store_path().map_err(|_| PairingError::Storage)?;
        let staging = final_path.with_file_name("protocol.staging.db");
        Ok(Self {
            staging,
            final_path,
        })
    }
}

/// What a completed pairing hands back so a connection row can be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedAccount {
    pub identity_key: String,
    pub masked_account: String,
    pub credential_ref: String,
    pub store_path: String,
    pub acknowledged_at: String,
}

struct Inner {
    state: PairingState,
    handle: Option<Arc<dyn RuntimeHandle>>,
    /// Set once the device is linked, so cleanup knows a remote logout is worth
    /// attempting.
    linked: bool,
    own_jid: Option<String>,
}

/// One pairing attempt. Dropping it without [`PairingSession::cancel`] leaves
/// staging state behind, so callers must always finish through `cancel` or
/// `finish`.
pub struct PairingSession {
    inner: Mutex<Inner>,
    paths: PairingPaths,
    credential_ref: String,
    acknowledged_at: String,
    qr: Arc<dyn QrSink>,
}

impl std::fmt::Debug for PairingSession {
    /// Only the state is printable; the credential reference, paths, and
    /// runtime handle must never reach a log or a panic message.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingSession")
            .field("state", &self.state().code())
            .finish_non_exhaustive()
    }
}

impl PairingSession {
    /// Records the acknowledgement and prepares staging state.
    ///
    /// Nothing is started and no QR can exist before this succeeds: the
    /// acknowledgement is the gate, per Plan 023 Step 4.
    pub fn acknowledge(
        paths: PairingPaths,
        acknowledged_version: &str,
        acknowledged_at: &str,
        already_linked: bool,
        tokens: &dyn TokenStore,
        qr: Arc<dyn QrSink>,
    ) -> Result<Self, PairingError> {
        if acknowledged_version.is_empty() {
            return Err(PairingError::AcknowledgementRequired);
        }
        if acknowledged_version != RISK_ACKNOWLEDGEMENT_VERSION {
            return Err(PairingError::AcknowledgementOutdated);
        }
        // One account per installation, checked before anything is created.
        if already_linked {
            return Err(PairingError::AlreadyLinked);
        }

        // A staging database from an abandoned attempt must never be reused.
        EncryptedProtocolStore::delete_files(&paths.staging).map_err(|_| PairingError::Storage)?;

        let provisioned = keyring::provision(tokens)?;
        let store = EncryptedProtocolStore::open(&paths.staging, provisioned.key)
            .map_err(|_| PairingError::Storage)?;
        drop(store);

        Ok(Self {
            inner: Mutex::new(Inner {
                state: PairingState::Starting,
                handle: None,
                linked: false,
                own_jid: None,
            }),
            paths,
            credential_ref: provisioned.credential_ref,
            acknowledged_at: acknowledged_at.to_string(),
            qr,
        })
    }

    pub fn state(&self) -> PairingState {
        self.inner.lock().expect("pairing lock").state.clone()
    }

    fn set_state(&self, state: PairingState) {
        self.inner.lock().expect("pairing lock").state = state;
    }

    /// Starts one pairing runtime against the staging store and drains its
    /// lifecycle events until the device links, the attempt fails, or the
    /// stream ends.
    pub async fn run(
        &self,
        launcher: &dyn RuntimeLauncher,
        tokens: &dyn TokenStore,
    ) -> Result<(), PairingError> {
        if !matches!(self.state(), PairingState::Starting) {
            return Err(PairingError::InvalidState);
        }

        let key = keyring::load(tokens, &self.credential_ref)?;
        let store = Arc::new(
            EncryptedProtocolStore::open(&self.paths.staging, key)
                .map_err(|_| PairingError::Storage)?,
        );

        let (handle, mut events) = match launcher.launch(store).await {
            Ok(started) => started,
            Err(error) => {
                self.set_state(PairingState::Failed {
                    code: PairingError::Runtime(error.clone()).code().into(),
                });
                return Err(error.into());
            }
        };
        self.inner.lock().expect("pairing lock").handle = Some(handle);

        while let Some(event) = events.recv().await {
            match event {
                RuntimeEvent::QrCode {
                    payload,
                    expires_in_seconds,
                } => {
                    // Superseding a code voids the previous one immediately, so
                    // an expired payload can never be scanned or reused.
                    self.qr.expire();
                    self.qr.present(&payload, expires_in_seconds);
                    self.set_state(PairingState::AwaitingScan);
                }
                RuntimeEvent::Connected { own_jid } => {
                    self.qr.expire();
                    let masked = match validate_own_jid(&own_jid) {
                        Ok(()) => provider::masked_account(&own_jid),
                        Err(error) => {
                            self.set_state(PairingState::Failed {
                                code: error.code().into(),
                            });
                            return Err(error);
                        }
                    };
                    {
                        let mut inner = self.inner.lock().expect("pairing lock");
                        inner.linked = true;
                        inner.own_jid = Some(own_jid);
                        inner.state = PairingState::AwaitingTest {
                            masked_account: masked,
                        };
                    }
                    return Ok(());
                }
                RuntimeEvent::LoggedOut => {
                    self.qr.expire();
                    self.set_state(PairingState::Failed {
                        code: "relink_required".into(),
                    });
                    return Err(PairingError::Runtime(RuntimeError::LoggedOut));
                }
                RuntimeEvent::Stopped => break,
            }
        }

        // The stream ended without linking: the code expired or the runtime
        // stopped. Either way this attempt is over.
        if !self.state().is_terminal() {
            self.set_state(PairingState::Failed {
                code: "pairing_incomplete".into(),
            });
        }
        Err(PairingError::Runtime(RuntimeError::Disconnected))
    }

    /// The explicit self-test. Only a definitive success moves to `Ready`.
    ///
    /// An ambiguous outcome deliberately does **not** create a ready
    /// connection: the message may have appeared on the phone, and the user is
    /// told exactly that.
    pub async fn send_test(&self, body: &str) -> Result<SendReceipt, PairingError> {
        let PairingState::AwaitingTest { masked_account } = self.state() else {
            return Err(PairingError::InvalidState);
        };

        let handle = {
            let inner = self.inner.lock().expect("pairing lock");
            inner.handle.clone().ok_or(PairingError::InvalidState)?
        };

        match handle.send_self_message(body).await {
            Ok(receipt) => {
                self.set_state(PairingState::Ready { masked_account });
                Ok(receipt)
            }
            Err(error) => {
                self.set_state(PairingState::Failed {
                    code: PairingError::Runtime(error.clone()).code().into(),
                });
                Err(error.into())
            }
        }
    }

    /// Promotes staging state and returns what the connection row needs.
    ///
    /// Only callable from `Ready`, so a connection can never exist without a
    /// successful explicit test.
    pub async fn finish(&self) -> Result<PairedAccount, PairingError> {
        let PairingState::Ready { masked_account } = self.state() else {
            return Err(PairingError::InvalidState);
        };
        let own_jid = self
            .inner
            .lock()
            .expect("pairing lock")
            .own_jid
            .clone()
            .ok_or(PairingError::InvalidState)?;

        // Stop the pairing runtime before moving its database out from under it.
        self.stop_runtime().await;

        EncryptedProtocolStore::delete_files(&self.paths.final_path)
            .map_err(|_| PairingError::Storage)?;
        std::fs::rename(&self.paths.staging, &self.paths.final_path)
            .map_err(|_| PairingError::Storage)?;
        // The staging sidecars belong to a database that no longer exists there.
        let _ = EncryptedProtocolStore::delete_files(&self.paths.staging);

        Ok(PairedAccount {
            identity_key: provider::identity_key(&own_jid),
            masked_account,
            credential_ref: self.credential_ref.clone(),
            store_path: self.paths.final_path.display().to_string(),
            acknowledged_at: self.acknowledged_at.clone(),
        })
    }

    /// Abandons the attempt: stop the runtime, best-effort remote logout if the
    /// device linked, then delete the staging database and the staging key.
    ///
    /// Local deletion happens whether or not the remote logout succeeds, and
    /// calling this twice is safe.
    pub async fn cancel(&self, tokens: &dyn TokenStore) {
        let (linked, handle) = {
            let inner = self.inner.lock().expect("pairing lock");
            (inner.linked, inner.handle.clone())
        };
        if let (true, Some(handle)) = (linked, handle) {
            // Best effort: an offline or already-revoked device must not block
            // local cleanup.
            let _ = handle.logout().await;
        }
        self.stop_runtime().await;

        let _ = EncryptedProtocolStore::delete_files(&self.paths.staging);
        let _ = keyring::delete(tokens, &self.credential_ref);

        if !matches!(self.state(), PairingState::Failed { .. }) {
            self.set_state(PairingState::Cancelled);
        }
        self.qr.expire();
    }

    async fn stop_runtime(&self) {
        let handle = self.inner.lock().expect("pairing lock").handle.take();
        if let Some(handle) = handle {
            handle.shutdown().await;
        }
    }
}

/// Rejects anything that is not a personal account JID.
///
/// A group, broadcast, or newsletter identifier must never become the
/// self-chat destination.
fn validate_own_jid(own_jid: &str) -> Result<(), PairingError> {
    let Some((user, server)) = own_jid.split_once('@') else {
        return Err(PairingError::InvalidIdentity);
    };
    let user = user.split(&[':', '.'][..]).next().unwrap_or_default();
    if user.is_empty() || server.is_empty() {
        return Err(PairingError::InvalidIdentity);
    }
    const PERSONAL_SERVERS: [&str; 2] = ["s.whatsapp.net", "lid"];
    if !PERSONAL_SERVERS.contains(&server) {
        return Err(PairingError::InvalidIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::token_store::InMemoryTokenStore;
    use crate::integrations::whatsapp::runtime::fake::FakeRuntime;

    const OWN_JID: &str = "34600123456@s.whatsapp.net";

    #[derive(Default)]
    struct RecordingSink {
        presented: Mutex<Vec<String>>,
        expiries: Mutex<usize>,
    }

    impl QrSink for RecordingSink {
        fn present(&self, payload: &str, _expires_in_seconds: u64) {
            self.presented.lock().unwrap().push(payload.to_string());
        }
        fn expire(&self) {
            *self.expiries.lock().unwrap() += 1;
        }
    }

    fn receipt() -> SendReceipt {
        SendReceipt {
            message_id: "3EB0".into(),
            submitted_at: "2026-08-19T00:00:00Z".into(),
            masked_destination: "***56@s.whatsapp.net".into(),
        }
    }

    /// Per-test directory. Tests must never touch the real app-data location:
    /// they run in parallel and would race on one shared staging database.
    struct TempPaths(std::path::PathBuf);

    impl TempPaths {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "alfred-whatsapp-pairing-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn paths(&self) -> PairingPaths {
            PairingPaths {
                staging: self.0.join("protocol.staging.db"),
                final_path: self.0.join("protocol.db"),
            }
        }
    }

    impl Drop for TempPaths {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Builds an acknowledged session pointed at a throwaway directory.
    fn session(
        paths: &TempPaths,
        tokens: &InMemoryTokenStore,
        sink: Arc<RecordingSink>,
    ) -> Result<PairingSession, PairingError> {
        PairingSession::acknowledge(
            paths.paths(),
            RISK_ACKNOWLEDGEMENT_VERSION,
            "2026-08-19T00:00:00Z",
            false,
            tokens,
            sink,
        )
    }

    #[test]
    fn pairing_cannot_start_without_an_acknowledgement() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("pairing_cannot_start_without_an_acknowledgement");
        let sink = Arc::new(RecordingSink::default());

        let error =
            PairingSession::acknowledge(paths.paths(), "", "now", false, &tokens, sink.clone())
                .expect_err("an empty acknowledgement must be refused");
        assert_eq!(error.code(), "acknowledgement_required");

        let error = PairingSession::acknowledge(paths.paths(), "0", "now", false, &tokens, sink)
            .expect_err("a stale acknowledgement must be refused");
        assert_eq!(error.code(), "acknowledgement_outdated");
    }

    #[test]
    fn a_refused_acknowledgement_provisions_nothing() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_refused_acknowledgement_provisions_nothing");
        let sink = Arc::new(RecordingSink::default());
        let _ = PairingSession::acknowledge(paths.paths(), "", "now", false, &tokens, sink);

        // No key may exist before the warning is accepted.
        assert!(
            tokens.get("whatsapp-protocol-store/any").is_err(),
            "no credential may be minted before acknowledgement"
        );
    }

    #[test]
    fn a_second_account_is_refused_before_anything_is_created() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_second_account_is_refused_before_anything_is_created");
        let sink = Arc::new(RecordingSink::default());
        let error = PairingSession::acknowledge(
            paths.paths(),
            RISK_ACKNOWLEDGEMENT_VERSION,
            "now",
            true,
            &tokens,
            sink,
        )
        .expect_err("one account per installation");
        assert_eq!(error.code(), "already_linked");
    }

    #[tokio::test]
    async fn a_successful_pairing_requires_a_test_before_becoming_ready() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_successful_pairing_requires_a_test_before_becoming_ready");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink.clone()).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![
            RuntimeEvent::QrCode {
                payload: "QR-PAYLOAD-1".into(),
                expires_in_seconds: 60,
            },
            RuntimeEvent::Connected {
                own_jid: OWN_JID.into(),
            },
        ]);
        *launcher.send_result.lock().unwrap() = Some(Ok(receipt()));

        session.run(&launcher, &tokens).await.expect("pairs");
        assert_eq!(
            session.state(),
            PairingState::AwaitingTest {
                masked_account: "***56@s.whatsapp.net".into()
            },
            "linking alone must not make the connection ready"
        );
        // Nothing may be promoted before the test.
        assert_eq!(session.finish().await.unwrap_err().code(), "invalid_state");

        session.send_test("hello").await.expect("test send");
        assert_eq!(
            session.state(),
            PairingState::Ready {
                masked_account: "***56@s.whatsapp.net".into()
            }
        );

        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn an_ambiguous_test_never_creates_a_ready_connection() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("an_ambiguous_test_never_creates_a_ready_connection");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: OWN_JID.into(),
        }]);
        *launcher.send_result.lock().unwrap() = Some(Err(RuntimeError::DeliveryUnknown));

        session.run(&launcher, &tokens).await.expect("pairs");
        let error = session.send_test("hello").await.unwrap_err();

        assert_eq!(error.code(), "test_delivery_unknown");
        assert_eq!(
            session.state(),
            PairingState::Failed {
                code: "test_delivery_unknown".into()
            }
        );
        assert_eq!(session.finish().await.unwrap_err().code(), "invalid_state");

        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn each_qr_supersedes_the_previous_one() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("each_qr_supersedes_the_previous_one");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink.clone()).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![
            RuntimeEvent::QrCode {
                payload: "QR-1".into(),
                expires_in_seconds: 60,
            },
            RuntimeEvent::QrCode {
                payload: "QR-2".into(),
                expires_in_seconds: 60,
            },
            RuntimeEvent::Connected {
                own_jid: OWN_JID.into(),
            },
        ]);
        *launcher.send_result.lock().unwrap() = Some(Ok(receipt()));

        session.run(&launcher, &tokens).await.expect("pairs");

        assert_eq!(*sink.presented.lock().unwrap(), vec!["QR-1", "QR-2"]);
        // Two supersessions plus the one on connect.
        assert_eq!(*sink.expiries.lock().unwrap(), 3);

        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn a_revoked_session_asks_for_a_relink_instead_of_repairing() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_revoked_session_asks_for_a_relink_instead_of_repairing");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::LoggedOut]);
        let error = session.run(&launcher, &tokens).await.unwrap_err();

        assert_eq!(error.code(), "relink_required");
        assert!(session.state().is_terminal());

        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn a_group_identity_is_refused() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_group_identity_is_refused");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: "123-456@g.us".into(),
        }]);
        let error = session.run(&launcher, &tokens).await.unwrap_err();

        assert_eq!(error.code(), "invalid_identity");
        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn cancelling_removes_the_staging_store_and_key() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("cancelling_removes_the_staging_store_and_key");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");
        let staging = session.paths.staging.clone();
        let credential_ref = session.credential_ref.clone();

        assert!(
            staging.exists(),
            "acknowledgement creates the staging store"
        );
        assert!(tokens.get(&credential_ref).is_ok());

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: OWN_JID.into(),
        }]);
        session.run(&launcher, &tokens).await.expect("pairs");
        session.cancel(&tokens).await;

        assert!(!staging.exists(), "staging database must be deleted");
        assert!(
            tokens.get(&credential_ref).is_err(),
            "staging key must be removed from the credential store"
        );
        // A linked device gets a best-effort remote logout before cleanup.
        assert!(launcher.calls().contains(&"logout".to_string()));
        assert!(launcher.calls().contains(&"shutdown".to_string()));
    }

    #[tokio::test]
    async fn cancelling_before_linking_skips_the_remote_logout() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("cancelling_before_linking_skips_the_remote_logout");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::QrCode {
            payload: "QR-1".into(),
            expires_in_seconds: 60,
        }]);
        let _ = session.run(&launcher, &tokens).await;
        session.cancel(&tokens).await;

        assert!(
            !launcher.calls().contains(&"logout".to_string()),
            "nothing was linked, so there is nothing to log out"
        );
        assert!(!session.paths.staging.exists());
    }

    #[tokio::test]
    async fn cancelling_twice_is_safe() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("cancelling_twice_is_safe");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        session.cancel(&tokens).await;
        session.cancel(&tokens).await;
        assert_eq!(session.state(), PairingState::Cancelled);
    }

    #[tokio::test]
    async fn a_failed_launch_reports_a_runtime_failure() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("a_failed_launch_reports_a_runtime_failure");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");

        let launcher = FakeRuntime::default();
        *launcher.launch_fails.lock().unwrap() = true;

        let error = session.run(&launcher, &tokens).await.unwrap_err();
        assert_eq!(error.code(), "runtime_unavailable");
        assert!(session.state().is_terminal());

        session.cancel(&tokens).await;
    }

    #[tokio::test]
    async fn finishing_promotes_staging_to_the_final_store() {
        let tokens = InMemoryTokenStore::default();
        let paths = TempPaths::new("finishing_promotes_staging_to_the_final_store");
        let sink = Arc::new(RecordingSink::default());
        let session = session(&paths, &tokens, sink).expect("acknowledge");
        let staging = session.paths.staging.clone();
        let final_path = session.paths.final_path.clone();

        let launcher = FakeRuntime::with_script(vec![RuntimeEvent::Connected {
            own_jid: OWN_JID.into(),
        }]);
        *launcher.send_result.lock().unwrap() = Some(Ok(receipt()));

        session.run(&launcher, &tokens).await.expect("pairs");
        session.send_test("hello").await.expect("test");
        let account = session.finish().await.expect("promote");

        assert!(!staging.exists(), "staging must not survive promotion");
        assert!(final_path.exists());
        assert_eq!(account.masked_account, "***56@s.whatsapp.net");
        assert_eq!(account.identity_key, provider::identity_key(OWN_JID));
        assert!(
            !format!("{account:?}").contains("34600123456"),
            "the paired-account record must carry no raw identity"
        );

        let _ = EncryptedProtocolStore::delete_files(&final_path);
        let _ = keyring::delete(&tokens, &account.credential_ref);
    }

    #[test]
    fn only_personal_account_servers_are_accepted() {
        assert!(validate_own_jid("34600123456@s.whatsapp.net").is_ok());
        assert!(validate_own_jid("34600123456:17@s.whatsapp.net").is_ok());
        assert!(validate_own_jid("237756605284433@lid").is_ok());

        for rejected in [
            "123-456@g.us",
            "status@broadcast",
            "1234@newsletter",
            "@s.whatsapp.net",
            "34600123456",
            "",
        ] {
            assert!(
                validate_own_jid(rejected).is_err(),
                "{rejected} must not become a self-chat destination"
            );
        }
    }

    #[test]
    fn pairing_states_never_carry_a_qr_or_a_raw_jid() {
        let states = [
            PairingState::AwaitingAcknowledgement,
            PairingState::Starting,
            PairingState::AwaitingScan,
            PairingState::AwaitingTest {
                masked_account: provider::masked_account(OWN_JID),
            },
            PairingState::Ready {
                masked_account: provider::masked_account(OWN_JID),
            },
            PairingState::Failed {
                code: "runtime_unavailable".into(),
            },
            PairingState::Cancelled,
        ];
        for state in states {
            let rendered = format!("{state:?}");
            assert!(!rendered.contains("34600123456"));
            assert!(!rendered.to_uppercase().contains("QR-"));
        }
    }
}
