//! Command-facing surface for WhatsApp pairing (Plan 023 Step 4).
//!
//! Holds at most one pairing attempt at a time and translates it into DTOs the
//! frontend may see. Nothing here returns a QR payload, a full JID, a phone
//! number, a credential reference, or the protocol client.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::pairing::{PairedAccount, PairingError, PairingPaths, PairingSession, PairingState, QrSink};
use super::provider::{self, RISK_ACKNOWLEDGEMENT_VERSION};
use super::owner::{RuntimeStatus, RuntimeTarget, WhatsAppRuntimeOwner};
use super::runtime::{RuntimeLauncher, WhatsAppLauncher};
use crate::db::Db;
use crate::integrations::models::{
    AppConnectionDto, ConnectionStatus, IntegrationCommandError, UpsertAppConnection,
};
use crate::integrations::token_store::TokenStore;

/// Event carrying one short-lived QR payload to the open connect modal.
///
/// The payload is emitted and never stored, logged, or included in analytics.
/// `whatsapp://qr-expired` voids the previous code so the modal stops rendering
/// something that can no longer be scanned.
const QR_EVENT: &str = "whatsapp://qr";
const QR_EXPIRED_EVENT: &str = "whatsapp://qr-expired";

/// Safe pairing status for the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppPairingStateDto {
    pub state: String,
    pub masked_account: Option<String>,
    pub failure_code: Option<String>,
    pub acknowledgement_version: String,
}

impl From<PairingState> for WhatsAppPairingStateDto {
    fn from(state: PairingState) -> Self {
        let (masked_account, failure_code) = match &state {
            PairingState::AwaitingTest { masked_account }
            | PairingState::Ready { masked_account } => (Some(masked_account.clone()), None),
            PairingState::Failed { code } => (None, Some(code.clone())),
            _ => (None, None),
        };
        Self {
            state: state.code().to_string(),
            masked_account,
            failure_code,
            acknowledgement_version: RISK_ACKNOWLEDGEMENT_VERSION.to_string(),
        }
    }
}

/// What the explicit test send reports back. No body, no own JID, no receipt
/// detail.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppTestSendDto {
    pub message_id: String,
    pub submitted_at: String,
    pub masked_destination: String,
}

/// One renderable pairing code. The frontend receives an already-rendered SVG,
/// so the raw payload never exists as a JavaScript string.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppQrDto {
    pub svg: String,
    pub expires_in_seconds: u64,
}

/// Renders a pairing payload as a self-contained SVG.
fn render_qr(payload: &str) -> Option<String> {
    use qrcode::render::svg;
    use qrcode::{EcLevel, QrCode};

    // `M` matches what WhatsApp Web itself uses and keeps the module count low
    // enough to stay legible at modal size.
    let code = QrCode::with_error_correction_level(payload, EcLevel::M).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// Emits each QR to the app window and nowhere else.
struct WindowQrSink {
    app: AppHandle,
}

impl QrSink for WindowQrSink {
    fn present(&self, payload: &str, expires_in_seconds: u64) {
        // Rendering here means the payload is never logged, never persisted, and
        // never handed to the frontend in scannable text form.
        let Some(svg) = render_qr(payload) else {
            return;
        };
        let _ = self.app.emit(
            QR_EVENT,
            WhatsAppQrDto {
                svg,
                expires_in_seconds,
            },
        );
    }

    fn expire(&self) {
        let _ = self.app.emit(QR_EXPIRED_EVENT, ());
    }
}

fn command_error(error: &PairingError) -> IntegrationCommandError {
    IntegrationCommandError::new(
        error.code(),
        &error.to_string(),
        // Only a state or acknowledgement problem is worth immediately retrying;
        // an ambiguous test explicitly is not.
        matches!(
            error,
            PairingError::AcknowledgementOutdated | PairingError::InvalidState
        ),
    )
}

pub struct WhatsAppService {
    session: Mutex<Option<Arc<PairingSession>>>,
    launcher: Arc<dyn RuntimeLauncher>,
    tokens: Arc<dyn TokenStore>,
    owner: Arc<WhatsAppRuntimeOwner>,
}

impl WhatsAppService {
    pub fn new(tokens: Arc<dyn TokenStore>) -> Self {
        Self::with_launcher(tokens, Arc::new(WhatsAppLauncher))
    }

    pub fn with_launcher(tokens: Arc<dyn TokenStore>, launcher: Arc<dyn RuntimeLauncher>) -> Self {
        Self {
            session: Mutex::new(None),
            owner: Arc::new(WhatsAppRuntimeOwner::new(tokens.clone(), launcher.clone())),
            launcher,
            tokens,
        }
    }

    /// Starts the long-lived runtime for a stored, connected account.
    ///
    /// Called once during application startup. A missing or revoked connection
    /// is not an error: Alfred simply runs without WhatsApp.
    pub async fn start_stored_runtime(&self, db: &Db) {
        let Some(target) = stored_target(db) else {
            return;
        };
        // A failure leaves the owner in an error state the settings UI can show;
        // it must never prevent Alfred from starting.
        let _ = self.owner.start(target).await;
    }

    pub fn runtime_status(&self) -> RuntimeStatus {
        self.owner.status()
    }

    /// One bounded reconnect, triggered from the settings UI.
    pub async fn reconnect_runtime(&self) -> Result<RuntimeStatus, IntegrationCommandError> {
        match self.owner.reconnect().await {
            Ok(()) => Ok(self.owner.status()),
            Err(error) => Err(IntegrationCommandError::new(
                if matches!(error, super::runtime::RuntimeError::LoggedOut) {
                    "relink_required"
                } else {
                    "runtime_unavailable"
                },
                &error.to_string(),
                error.is_retryable(),
            )),
        }
    }

    /// Stops the runtime during orderly application exit.
    pub async fn shutdown_runtime(&self) {
        self.owner.shutdown().await;
    }

    fn current(&self) -> Option<Arc<PairingSession>> {
        self.session.lock().expect("whatsapp session lock").clone()
    }

    /// Records the risk acknowledgement and starts one pairing attempt.
    ///
    /// Any previous attempt is cancelled first, so a reopened modal can never
    /// leave a second runtime or a stale staging store behind.
    pub async fn begin_pairing(
        &self,
        db: &Db,
        app: AppHandle,
        acknowledged_version: &str,
    ) -> Result<WhatsAppPairingStateDto, IntegrationCommandError> {
        self.cancel_pairing().await;

        let already_linked = db
            .list_app_connections()
            .map_err(|_| IntegrationCommandError::new("storage_unavailable", "Alfred could not read its connections.", true))?
            .iter()
            .any(|connection| connection.provider_id == provider::PROVIDER_ID);

        let paths = PairingPaths::resolve().map_err(|error| command_error(&error))?;
        let session = PairingSession::acknowledge(
            paths,
            acknowledged_version,
            &chrono::Utc::now().to_rfc3339(),
            already_linked,
            self.tokens.as_ref(),
            Arc::new(WindowQrSink { app }),
        )
        .map_err(|error| command_error(&error))?;

        let session = Arc::new(session);
        *self.session.lock().expect("whatsapp session lock") = Some(session.clone());

        // The runtime drains lifecycle events in the background; the modal polls
        // `pairing_state` and listens for QR events.
        let launcher = self.launcher.clone();
        let tokens = self.tokens.clone();
        let running = session.clone();
        tokio::spawn(async move {
            let _ = running.run(launcher.as_ref(), tokens.as_ref()).await;
        });

        Ok(session.state().into())
    }

    pub fn pairing_state(&self) -> WhatsAppPairingStateDto {
        self.current()
            .map(|session| session.state())
            .unwrap_or(PairingState::AwaitingAcknowledgement)
            .into()
    }

    /// The explicit self-test. Required before a connection can exist.
    pub async fn send_pairing_test(
        &self,
        body: &str,
    ) -> Result<WhatsAppTestSendDto, IntegrationCommandError> {
        let session = self.current().ok_or_else(|| {
            command_error(&PairingError::InvalidState)
        })?;

        let receipt = session
            .send_test(body)
            .await
            .map_err(|error| command_error(&error))?;

        Ok(WhatsAppTestSendDto {
            message_id: receipt.message_id,
            submitted_at: receipt.submitted_at,
            masked_destination: receipt.masked_destination,
        })
    }

    /// Promotes the staging store and creates the connection row.
    pub async fn complete_pairing(
        &self,
        db: &Db,
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let session = self
            .current()
            .ok_or_else(|| command_error(&PairingError::InvalidState))?;

        let account = session
            .finish()
            .await
            .map_err(|error| command_error(&error))?;

        let connection = db
            .upsert_app_connection(upsert_for(&account))
            .map_err(|error| {
                IntegrationCommandError::new("connection_failed", &error.to_string(), false)
            })?;

        *self.session.lock().expect("whatsapp session lock") = None;

        // Hand the freshly paired account straight to the long-lived runtime so
        // the user does not have to restart Alfred to use it.
        self.owner
            .start(RuntimeTarget {
                credential_ref: account.credential_ref.clone(),
                store_path: account.store_path.clone(),
            })
            .await
            .ok();

        Ok(connection.into())
    }

    /// Abandons any in-flight attempt. Safe to call when nothing is running.
    pub async fn cancel_pairing(&self) {
        let session = self.session.lock().expect("whatsapp session lock").take();
        if let Some(session) = session {
            session.cancel(self.tokens.as_ref()).await;
        }
    }
}

/// Finds the stored, still-connected WhatsApp account, if any.
fn stored_target(db: &Db) -> Option<RuntimeTarget> {
    let connections = db.list_app_connections().ok()?;
    let connection = connections.into_iter().find(|connection| {
        connection.provider_id == provider::PROVIDER_ID
            && connection.status == ConnectionStatus::Connected
    })?;
    let store_path = connection
        .provider_metadata
        .get(provider::metadata_key::STORE_PATH)?
        .clone();
    Some(RuntimeTarget {
        credential_ref: connection.credential_ref,
        store_path,
    })
}

fn upsert_for(account: &PairedAccount) -> UpsertAppConnection {
    UpsertAppConnection {
        provider_id: provider::PROVIDER_ID.to_string(),
        display_name: Some(provider::display_name()),
        // No account or tenant identifier reaches Alfred's main database.
        external_account_id: None,
        external_tenant_id: None,
        connection_mode: provider::CONNECTION_MODE.to_string(),
        identity_key: account.identity_key.clone(),
        scopes: Vec::new(),
        provider_metadata: provider::connection_metadata_from(
            &account.masked_account,
            &account.store_path,
            &account.acknowledged_at,
        ),
        expires_at: None,
        credential_ref: account.credential_ref.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_dto_exposes_no_payload_or_raw_identity() {
        let dto: WhatsAppPairingStateDto = PairingState::AwaitingTest {
            masked_account: "***56@s.whatsapp.net".into(),
        }
        .into();

        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("awaiting_test"));
        assert!(json.contains("***56@s.whatsapp.net"));
        assert!(!json.contains("34600123456"));
    }

    #[test]
    fn a_failed_state_reports_only_a_stable_code() {
        let dto: WhatsAppPairingStateDto = PairingState::Failed {
            code: "test_delivery_unknown".into(),
        }
        .into();

        assert_eq!(dto.failure_code.as_deref(), Some("test_delivery_unknown"));
        assert!(dto.masked_account.is_none());
    }

    #[test]
    fn an_ambiguous_test_is_not_marked_recoverable() {
        use super::super::runtime::RuntimeError;
        let error = command_error(&PairingError::Runtime(RuntimeError::DeliveryUnknown));
        assert_eq!(error.code, "test_delivery_unknown");
        assert!(
            !error.recoverable,
            "a possibly delivered message must never invite an automatic retry"
        );
    }

    #[test]
    fn a_pairing_payload_renders_to_a_self_contained_svg() {
        let svg = render_qr("2@abc123,def456==,ghi789==").expect("renders");

        // The renderer emits an XML prolog ahead of the root element.
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        // A strict CSP applies to the app window: nothing may be fetched. The
        // `xmlns` namespace URI is a declaration, not a request, so the check
        // targets the constructs that actually load something.
        for forbidden in ["<script", "<image", "xlink:href", "href=\"http", "url(http"] {
            assert!(!svg.contains(forbidden), "SVG must not contain {forbidden}");
        }
        // The scannable text form must not survive into the markup.
        assert!(!svg.contains("2@abc123"));
    }

    #[test]
    fn an_unrenderable_payload_is_dropped_rather_than_leaked() {
        // Beyond QR capacity: the sink must emit nothing rather than fall back
        // to shipping the raw string.
        let oversized = "x".repeat(8_000);
        assert!(render_qr(&oversized).is_none());
    }

    #[test]
    fn the_upsert_carries_no_raw_identity() {
        let account = PairedAccount {
            identity_key: provider::identity_key("34600123456@s.whatsapp.net"),
            masked_account: "***56@s.whatsapp.net".into(),
            credential_ref: "whatsapp-protocol-store/abc".into(),
            store_path: "/tmp/whatsapp/protocol.db".into(),
            acknowledged_at: "2026-08-19T00:00:00Z".into(),
        };
        let upsert = upsert_for(&account);

        assert!(upsert.external_account_id.is_none());
        assert!(upsert.external_tenant_id.is_none());
        let rendered = format!("{:?}{:?}", upsert.provider_metadata, upsert.identity_key);
        assert!(!rendered.contains("34600123456"));
    }
}
