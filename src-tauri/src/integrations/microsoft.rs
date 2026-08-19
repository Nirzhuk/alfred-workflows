//! Microsoft 365 connected-app provider (Plan 013).
//!
//! Native-app OAuth authorization-code + S256 PKCE with a loopback callback on
//! `127.0.0.1`. Only the public Entra client ID is compiled in; there is no
//! desktop client secret. Identity uses OpenID (`openid`, `profile`,
//! `offline_access`, `User.Read`). Mail send, mail metadata, and calendar write
//! are requested only when the user enables those capabilities. Mail bodies use
//! `Mail.ReadBasic` (`bodyPreview` only); `Mail.Read` is not requested.

use super::actions::{
    ActionArtifact, ActionCancellation, ActionDescriptor, ActionError, ActionErrorCode,
    ActionExecutor, ActionFieldDescriptor, ActionFieldKind, ActionFuture, ActionLimits,
    ActionRegistry, ActionResourceItem, ActionResourcePage, ActionResourcesFuture, ActionResult,
    TokenAccessCapability, ValidatedActionRequest,
};
use super::events::{
    AppEventAdapter, AppEventBatch, AppEventCancellation, AppEventDeliveryMode, AppEventDescriptor,
    AppEventError, AppEventErrorCode, AppEventFuture, AppEventRegistry, AppEventResourceItem,
    AppEventResourcePage, AppTriggerConfig, NormalizedAppEvent,
    NORMALIZED_APP_EVENT_SCHEMA_VERSION,
};
use super::models::{
    canonical_identity_key, AppConnection, AppConnectionDto, IntegrationCommandError,
    UpsertAppConnection,
};
use super::oauth_native::{NativeOAuthAttempt, NativeOAuthConfig, NativeOAuthError};
use super::refresh::{ProviderRefreshError, RefreshFuture, RefreshHandler};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use futures_util::StreamExt;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const LOGIN_ORIGIN: &str = "https://login.microsoftonline.com";
const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";
const MSA_TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";
const USER_AGENT: &str = "Alfred-Desktop";
const RESPONSE_LIMIT: usize = 512 * 1024;
const ERROR_HINT_LIMIT: usize = 16 * 1024;
const OAUTH_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_PAIRING_SESSIONS: usize = 8;
const MAX_RECIPIENTS: usize = 50;
const MAX_ADDRESS_CHARS: usize = 254;
const MAX_SUBJECT_CHARS: usize = 255;
const MAX_BODY_CHARS: usize = 32 * 1024;
const MAX_MAIL_RESULTS: usize = 25;
const MAX_PREVIEW_CHARS: usize = 255;
const MAX_GRAPH_ID_CHARS: usize = 512;
const MAX_DELTA_PAGES: usize = 5;
const IDENTITY_SCOPES: [&str; 4] = ["openid", "profile", "offline_access", "User.Read"];
pub const MAIL_SEND_SCOPE: &str = "Mail.Send";
pub const MAIL_READ_SCOPE: &str = "Mail.ReadBasic";
pub const CALENDAR_SCOPE: &str = "Calendars.ReadWrite";

pub fn is_configured() -> bool {
    option_env!("ALFRED_MICROSOFT_CLIENT_ID").is_some_and(|value| !value.trim().is_empty())
}

pub fn register(
    actions: &ActionRegistry,
    events: &AppEventRegistry,
    service: Arc<MicrosoftService>,
) -> Result<(), ActionError> {
    for descriptor in action_descriptors() {
        actions.register(descriptor, ActionLimits::default(), service.clone())?;
    }
    for descriptor in event_descriptors() {
        events
            .register(descriptor, service.clone())
            .map_err(|error| ActionError::new(map_event_registration_error(error.code)))?;
    }
    Ok(())
}

fn map_event_registration_error(code: AppEventErrorCode) -> ActionErrorCode {
    match code {
        AppEventErrorCode::ProviderUnavailable => ActionErrorCode::ProviderUnavailable,
        _ => ActionErrorCode::InvalidInput,
    }
}

pub struct MicrosoftService {
    client_id: Option<String>,
    tenant: String,
    login_base: String,
    graph_base: String,
    preferred_ports: Vec<u16>,
    sessions: Mutex<HashMap<String, MicrosoftPairingSession>>,
}

impl Default for MicrosoftService {
    fn default() -> Self {
        Self {
            client_id: option_env!("ALFRED_MICROSOFT_CLIENT_ID")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            tenant: configured_tenant(),
            login_base: LOGIN_ORIGIN.into(),
            graph_base: GRAPH_API_BASE.into(),
            preferred_ports: option_env!("ALFRED_MICROSOFT_OAUTH_PORT")
                .and_then(|value| value.trim().parse::<u16>().ok())
                .map(|port| vec![port])
                .unwrap_or_default(),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

fn configured_tenant() -> String {
    option_env!("ALFRED_MICROSOFT_TENANT")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_tenant)
        .unwrap_or_else(|| "common".into())
}

fn normalize_tenant(value: &str) -> String {
    let trimmed = value.trim();
    if matches!(trimmed, "common" | "organizations" | "consumers") || valid_tenant_guid(trimmed) {
        trimmed.to_ascii_lowercase()
    } else {
        "common".into()
    }
}

fn valid_tenant_guid(value: &str) -> bool {
    let mut parts = value.split('-');
    let lengths = [8, 4, 4, 4, 12];
    lengths.into_iter().all(|length| {
        parts.next().is_some_and(|part| {
            part.len() == length && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    }) && parts.next().is_none()
}

impl MicrosoftService {
    pub fn refresh_handler(&self) -> Arc<dyn RefreshHandler> {
        Arc::new(MicrosoftRefreshHandler {
            client_id: self.client_id.clone(),
            tenant: self.tenant.clone(),
            login_base: self.login_base.clone(),
        })
    }

    pub fn prepare_authorization(
        &self,
        db: &Db,
        input: MicrosoftPrepareInput,
    ) -> Result<MicrosoftAuthorizationStarted, IntegrationCommandError> {
        self.remove_expired_sessions();
        let client_id = self.require_client_id()?;
        if self
            .sessions
            .lock()
            .map_err(|_| microsoft_state_error())?
            .len()
            >= MAX_PAIRING_SESSIONS
        {
            return Err(command_error(
                "microsoft_pairing_busy",
                "Too many Microsoft authorization attempts are active. Close another attempt and try again.",
                true,
            ));
        }
        let existing = if let Some(connection_id) = input.reconnect_connection_id.as_deref() {
            let connection = db
                .get_app_connection(connection_id)
                .map_err(|_| connection_store_error())?
                .ok_or_else(IntegrationCommandError::not_found)?;
            if connection.provider_id != "microsoft" {
                return Err(IntegrationCommandError::not_found());
            }
            Some(connection)
        } else {
            None
        };
        let scopes = requested_scopes(&input, existing.as_ref());
        let upgrading = existing.as_ref().is_some_and(|connection| {
            scopes
                .iter()
                .any(|scope| !connection.scopes.contains(scope))
        });
        let prompt = if upgrading {
            "consent"
        } else {
            "select_account"
        };
        let config = NativeOAuthConfig {
            authorization_endpoint: format!(
                "{}/{}/oauth2/v2.0/authorize",
                LOGIN_ORIGIN, self.tenant
            ),
            client_id: client_id.to_owned(),
            scopes: scopes.clone(),
            extra_params: BTreeMap::from([("prompt".into(), prompt.into())]),
            callback_path: "/oauth/callback".into(),
            timeout: OAUTH_TIMEOUT,
            include_nonce: true,
            preferred_ports: self.preferred_ports.clone(),
        };
        let attempt = NativeOAuthAttempt::start(config).map_err(map_native_oauth_error)?;
        let authorization_url = attempt.authorization_url().clone();
        if !valid_microsoft_auth_url(&authorization_url) {
            return Err(command_error(
                "microsoft_invalid_response",
                "Microsoft authorization could not be started.",
                false,
            ));
        }
        let expires_at =
            (Utc::now() + ChronoDuration::seconds(OAUTH_TIMEOUT.as_secs() as i64)).to_rfc3339();
        let session_id = Uuid::new_v4().to_string();
        self.sessions
            .lock()
            .map_err(|_| microsoft_state_error())?
            .insert(
                session_id.clone(),
                MicrosoftPairingSession {
                    attempt,
                    cancel: Arc::new(AtomicBool::new(false)),
                    expires_at: Instant::now() + OAUTH_TIMEOUT,
                    reconnect_connection_id: existing.map(|connection| connection.id),
                    requested_scopes: scopes,
                },
            );
        Ok(MicrosoftAuthorizationStarted {
            session_id,
            authorization_url: authorization_url.to_string(),
            expires_at,
        })
    }

    pub async fn complete_authorization(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        session_id: &str,
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| microsoft_state_error())?
            .remove(session_id)
            .ok_or_else(microsoft_pairing_expired)?;
        if Instant::now() >= session.expires_at {
            return Err(microsoft_pairing_expired());
        }
        let cancel = session.cancel.clone();
        let reconnect_connection_id = session.reconnect_connection_id.clone();
        let requested_scopes = session.requested_scopes.clone();
        let verified = tauri::async_runtime::spawn_blocking(move || {
            session.attempt.wait_for_callback_cancellable(cancel)
        })
        .await
        .map_err(|_| microsoft_state_error())?
        .map_err(map_native_oauth_error)?;
        self.exchange_and_save(
            db,
            store,
            verified,
            reconnect_connection_id.as_deref(),
            &requested_scopes,
        )
        .await
    }

    pub fn cancel_authorization(&self, session_id: &str) {
        if let Some(session) = self.sessions.lock().ok().and_then(|sessions| {
            sessions
                .get(session_id)
                .map(|session| session.cancel.clone())
        }) {
            session.store(true, Ordering::SeqCst);
        }
    }

    fn remove_expired_sessions(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            let now = Instant::now();
            sessions.retain(|_, session| session.expires_at > now);
        }
    }

    fn require_client_id(&self) -> Result<&str, IntegrationCommandError> {
        self.client_id.as_deref().ok_or_else(|| {
            command_error(
                "microsoft_not_configured",
                "This build does not include the public Microsoft OAuth client ID.",
                false,
            )
        })
    }

    async fn exchange_and_save(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        verified: super::oauth_native::VerifiedAuthorizationCode,
        reconnect_connection_id: Option<&str>,
        requested_scopes: &[String],
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let client_id = self.require_client_id()?;
        let nonce = verified.context.nonce.clone().ok_or_else(|| {
            command_error(
                "microsoft_invalid_id_token",
                "Microsoft did not complete OpenID authorization.",
                false,
            )
        })?;
        let response: MicrosoftTokenResponse = token_post_form(
            &self.token_url(),
            &[
                ("client_id", client_id),
                ("code", &verified.code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", &verified.context.redirect_uri),
                ("code_verifier", &verified.context.verifier),
                ("scope", &requested_scopes.join(" ")),
            ],
        )
        .await
        .map_err(map_token_exchange_error)?;
        let access_token = Zeroizing::new(response.access_token);
        if access_token.trim().is_empty()
            || !response
                .token_type
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
        {
            return Err(microsoft_invalid_response());
        }
        let refresh_token = Zeroizing::new(response.refresh_token.unwrap_or_default());
        if refresh_token.trim().is_empty() {
            return Err(command_error(
                "microsoft_offline_access_required",
                "Microsoft did not grant offline access. Authorize again.",
                false,
            ));
        }
        let id_token = response.id_token.ok_or_else(|| {
            command_error(
                "microsoft_invalid_id_token",
                "Microsoft did not return an ID token.",
                false,
            )
        })?;
        let claims = self
            .validate_id_token(&id_token, &nonce, client_id)
            .await
            .map_err(map_id_token_error)?;
        let identity = self
            .load_graph_identity(access_token.as_str(), &claims)
            .await?;
        self.enforce_tenant_policy(&identity.tenant_id)?;
        let identity_key = canonical_identity_key(
            "microsoft",
            "native_oauth",
            &[&identity.tenant_id, &identity.object_id],
        );
        if let Some(connection_id) = reconnect_connection_id {
            let existing = db
                .get_app_connection(connection_id)
                .map_err(|_| connection_store_error())?
                .ok_or_else(IntegrationCommandError::not_found)?;
            if existing.identity_key != identity_key {
                return Err(command_error(
                    "microsoft_account_mismatch",
                    "Reconnect the same Microsoft account. Disconnect first to link a different account.",
                    false,
                ));
            }
        }
        let existing = db
            .get_app_connection_by_identity("microsoft", "native_oauth", &identity_key)
            .map_err(|_| connection_store_error())?;
        let credential_ref = existing
            .as_ref()
            .map(|connection| connection.credential_ref.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let is_new = existing.is_none();
        let prior = if is_new {
            None
        } else {
            let prior_store = store.clone();
            let prior_ref = credential_ref.clone();
            tauri::async_runtime::spawn_blocking(move || prior_store.get(&prior_ref))
                .await
                .ok()
                .and_then(Result::ok)
        };
        let mut credential = CredentialEnvelope::new(access_token.as_str().to_owned());
        credential.refresh_token = Some(refresh_token.as_str().to_owned());
        credential.expires_at = response
            .expires_in
            .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
        let saved_store = store.clone();
        let saved_ref = credential_ref.clone();
        tauri::async_runtime::spawn_blocking(move || saved_store.put(&saved_ref, &credential))
            .await
            .map_err(|_| credential_write_error())?
            .map_err(map_token_store_connect_error)?;

        let scopes = sanitize_granted_scopes(response.scope.as_deref(), requested_scopes);
        let mut provider_metadata = BTreeMap::new();
        if let Some(email) = identity.email.clone() {
            provider_metadata.insert("email".into(), email);
        }
        let saved = db.upsert_app_connection(UpsertAppConnection {
            provider_id: "microsoft".into(),
            display_name: Some(identity.display_name.clone()),
            external_account_id: Some(identity.object_id.clone()),
            external_tenant_id: Some(identity.tenant_id.clone()),
            connection_mode: "native_oauth".into(),
            identity_key,
            scopes,
            provider_metadata,
            expires_at: response
                .expires_in
                .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339()),
            credential_ref: credential_ref.clone(),
        });
        match saved {
            Ok(connection) => Ok(AppConnectionDto::from(connection)),
            Err(_) => {
                if is_new {
                    let cleanup_store = store.clone();
                    let cleanup_ref = credential_ref.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        cleanup_store.delete(&cleanup_ref)
                    })
                    .await;
                } else if let Some(prior) = prior {
                    let rollback_store = store;
                    let rollback_ref = credential_ref;
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        rollback_store.put(&rollback_ref, &prior)
                    })
                    .await;
                }
                Err(connection_store_error())
            }
        }
    }

    fn token_url(&self) -> String {
        format!("{}/{}/oauth2/v2.0/token", self.login_base, self.tenant)
    }

    fn jwks_url(&self) -> String {
        format!("{}/{}/discovery/v2.0/keys", self.login_base, self.tenant)
    }

    fn enforce_tenant_policy(&self, tid: &str) -> Result<(), IntegrationCommandError> {
        let personal = tid.eq_ignore_ascii_case(MSA_TENANT);
        match self.tenant.as_str() {
            "organizations" if personal => Err(command_error(
                "microsoft_personal_account_blocked",
                "This Alfred build accepts only work or school Microsoft accounts.",
                false,
            )),
            "consumers" if !personal => Err(command_error(
                "microsoft_work_account_blocked",
                "This Alfred build accepts only personal Microsoft accounts.",
                false,
            )),
            tenant if valid_tenant_guid(tenant) && !tid.eq_ignore_ascii_case(tenant) => {
                Err(command_error(
                    "microsoft_account_mismatch",
                    "That Microsoft account belongs to a different organization.",
                    false,
                ))
            }
            _ => Ok(()),
        }
    }

    async fn validate_id_token(
        &self,
        token: &str,
        nonce: &str,
        client_id: &str,
    ) -> Result<MicrosoftIdClaims, ActionError> {
        let header =
            decode_header(token).map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?;
        if header.alg != Algorithm::RS256 {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let kid = header
            .kid
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
        let key = self.decoding_key(&kid).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[client_id]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<MicrosoftIdClaims>(token, &key, &validation)
            .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?
            .claims;
        if claims.nonce.as_deref() != Some(nonce)
            || claims
                .tid
                .as_deref()
                .is_none_or(|tid| !valid_tenant_guid(tid) && tid != MSA_TENANT)
            || claims
                .oid
                .as_deref()
                .unwrap_or(claims.sub.as_str())
                .is_empty()
        {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let tid = claims.tid.as_deref().unwrap_or_default();
        let expected_iss = format!("{LOGIN_ORIGIN}/{tid}/v2.0");
        if !claims.iss.eq_ignore_ascii_case(&expected_iss) {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        Ok(claims)
    }

    async fn decoding_key(&self, kid: &str) -> Result<DecodingKey, ActionError> {
        let jwks: MicrosoftJwks = get_unauthenticated_json(&self.jwks_url()).await?;
        let key = jwks
            .keys
            .into_iter()
            .find(|key| key.kid.as_deref() == Some(kid) && key.kty == "RSA")
            .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
        DecodingKey::from_rsa_components(
            key.n.as_deref().unwrap_or_default(),
            key.e.as_deref().unwrap_or_default(),
        )
        .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))
    }

    async fn load_graph_identity(
        &self,
        token: &str,
        claims: &MicrosoftIdClaims,
    ) -> Result<MicrosoftIdentity, IntegrationCommandError> {
        let object_id = claims
            .oid
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| claims.sub.clone());
        let tenant_id = claims.tid.clone().unwrap_or_default();
        let profile: Result<GraphUser, ActionError> = self
            .get_json(
                token,
                "/me",
                &[("$select", "id,displayName,userPrincipalName,mail")],
            )
            .await;
        let profile = profile.map_err(map_connect_action_error)?;
        if !profile.id.is_empty() && profile.id != object_id {
            return Err(command_error(
                "microsoft_identity_invalid",
                "Microsoft did not return a consistent account identity.",
                false,
            ));
        }
        let email = profile
            .mail
            .or(profile.user_principal_name.clone())
            .or_else(|| claims.preferred_username.clone())
            .filter(|value| valid_email_address(value));
        let display_name = profile
            .display_name
            .or_else(|| claims.name.clone())
            .or_else(|| email.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                command_error(
                    "microsoft_identity_invalid",
                    "Microsoft did not return a usable account name.",
                    false,
                )
            })?;
        Ok(MicrosoftIdentity {
            object_id,
            tenant_id,
            display_name,
            email,
        })
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, ActionError> {
        let request = graph_request(
            graph_client(),
            token,
            Method::GET,
            &format!("{}{path}", self.graph_base),
        )
        .query(query);
        send_graph_json(request, false)
            .await
            .map(|response| response.value)
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        body: &Value,
    ) -> Result<(T, Option<String>), ActionError> {
        let request = graph_request(
            graph_client(),
            token,
            Method::POST,
            &format!("{}{path}", self.graph_base),
        )
        .json(body);
        send_graph_json(request, true)
            .await
            .map(|response| (response.value, response.request_id))
    }

    async fn graph_get_raw(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
        prefer: Option<&str>,
    ) -> Result<Value, ActionError> {
        let mut request = graph_request(
            graph_client(),
            token,
            Method::GET,
            &format!("{}{path}", self.graph_base),
        )
        .query(query);
        if let Some(prefer) = prefer {
            request = request.header("Prefer", prefer);
        }
        send_graph_json(request, false)
            .await
            .map(|response| response.value)
    }
}

struct MicrosoftPairingSession {
    attempt: NativeOAuthAttempt,
    cancel: Arc<AtomicBool>,
    expires_at: Instant,
    reconnect_connection_id: Option<String>,
    requested_scopes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftPrepareInput {
    #[serde(default)]
    pub send_mail: bool,
    #[serde(default)]
    pub read_mail: bool,
    #[serde(default)]
    pub calendar: bool,
    pub reconnect_connection_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftAuthorizationStarted {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_at: String,
}

#[derive(Default, Deserialize)]
struct MicrosoftTokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftIdClaims {
    iss: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    oid: Option<String>,
    sub: String,
    #[serde(default)]
    tid: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct MicrosoftJwks {
    #[serde(default)]
    keys: Vec<MicrosoftJwk>,
}

#[derive(Deserialize)]
struct MicrosoftJwk {
    #[serde(default)]
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
}

#[derive(Deserialize)]
struct GraphUser {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default, rename = "userPrincipalName")]
    user_principal_name: Option<String>,
    #[serde(default)]
    mail: Option<String>,
}

struct MicrosoftIdentity {
    object_id: String,
    tenant_id: String,
    display_name: String,
    email: Option<String>,
}

#[derive(Deserialize, Default)]
struct GraphNoContent {}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct GraphCollection<T> {
    #[serde(default)]
    value: Vec<T>,
    #[serde(default, rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(default, rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

#[derive(Deserialize)]
struct GraphMailMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    from: Option<GraphRecipient>,
    #[serde(default, rename = "toRecipients")]
    to_recipients: Vec<GraphRecipient>,
    #[serde(default, rename = "receivedDateTime")]
    received_date_time: Option<String>,
    #[serde(default, rename = "webLink")]
    web_link: Option<String>,
    #[serde(default, rename = "isRead")]
    is_read: Option<bool>,
    #[serde(default, rename = "bodyPreview")]
    body_preview: Option<String>,
    #[serde(default, rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(default, rename = "@removed")]
    removed: Option<Value>,
}

#[derive(Deserialize)]
struct GraphRecipient {
    #[serde(default, rename = "emailAddress")]
    email_address: Option<GraphEmailAddress>,
}

#[derive(Deserialize)]
struct GraphEmailAddress {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    address: Option<String>,
}

#[derive(Deserialize)]
struct GraphCalendar {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct GraphMailFolder {
    id: String,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct GraphEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    start: Option<GraphDateTime>,
    #[serde(default)]
    end: Option<GraphDateTime>,
    #[serde(default)]
    organizer: Option<GraphRecipient>,
    #[serde(default, rename = "webLink")]
    web_link: Option<String>,
    #[serde(default, rename = "lastModifiedDateTime")]
    last_modified_date_time: Option<String>,
    #[serde(default, rename = "@removed")]
    removed: Option<Value>,
}

#[derive(Deserialize)]
struct GraphDateTime {
    #[serde(default, rename = "dateTime")]
    date_time: Option<String>,
    #[serde(default, rename = "timeZone")]
    time_zone: Option<String>,
}

struct MicrosoftRefreshHandler {
    client_id: Option<String>,
    tenant: String,
    login_base: String,
}

impl RefreshHandler for MicrosoftRefreshHandler {
    fn needs_refresh(&self, connection: &AppConnection, now: DateTime<Utc>) -> bool {
        connection
            .expires_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc) <= now + ChronoDuration::minutes(5))
            .unwrap_or(false)
    }

    fn refresh<'a>(
        &'a self,
        _connection: &'a AppConnection,
        mut credential: CredentialEnvelope,
    ) -> RefreshFuture<'a> {
        Box::pin(async move {
            let client_id = self
                .client_id
                .as_deref()
                .ok_or_else(|| ProviderRefreshError::terminal("microsoft_not_configured"))?;
            let refresh_token = Zeroizing::new(
                credential
                    .refresh_token
                    .take()
                    .ok_or_else(|| ProviderRefreshError::terminal("microsoft_grant_revoked"))?,
            );
            let response: MicrosoftTokenResponse = token_post_form(
                &format!("{}/{}/oauth2/v2.0/token", self.login_base, self.tenant),
                &[
                    ("client_id", client_id),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.as_str()),
                ],
            )
            .await
            .map_err(|error| match error.code {
                ActionErrorCode::ProviderUnauthorized
                | ActionErrorCode::ScopeMissing
                | ActionErrorCode::InvalidInput => {
                    ProviderRefreshError::terminal("microsoft_grant_revoked")
                }
                _ => ProviderRefreshError::retryable("microsoft_unavailable"),
            })?;
            let access_token = response.access_token.trim().to_owned();
            if access_token.is_empty()
                || !response
                    .token_type
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
            {
                return Err(ProviderRefreshError::retryable(
                    "microsoft_invalid_response",
                ));
            }
            credential.access_token = access_token;
            credential.refresh_token = response
                .refresh_token
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(refresh_token.as_str().to_owned()));
            credential.expires_at = response
                .expires_in
                .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
            Ok(credential)
        })
    }
}

fn requested_scopes(
    input: &MicrosoftPrepareInput,
    existing: Option<&AppConnection>,
) -> Vec<String> {
    let mut scopes: HashSet<String> = IDENTITY_SCOPES
        .iter()
        .map(|scope| (*scope).to_owned())
        .collect();
    if let Some(existing) = existing {
        scopes.extend(existing.scopes.iter().cloned());
    }
    if input.send_mail {
        scopes.insert(MAIL_SEND_SCOPE.into());
    }
    if input.read_mail {
        scopes.insert(MAIL_READ_SCOPE.into());
    }
    if input.calendar {
        scopes.insert(CALENDAR_SCOPE.into());
    }
    let mut scopes = scopes.into_iter().collect::<Vec<_>>();
    scopes.sort();
    scopes
}

fn sanitize_granted_scopes(granted: Option<&str>, requested: &[String]) -> Vec<String> {
    let allowed: HashSet<&str> = IDENTITY_SCOPES
        .iter()
        .copied()
        .chain([MAIL_SEND_SCOPE, MAIL_READ_SCOPE, CALENDAR_SCOPE])
        .collect();
    let mut scopes = granted
        .unwrap_or_default()
        .split_whitespace()
        .filter(|scope| allowed.contains(*scope))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        scopes = requested.to_vec();
    }
    scopes.sort();
    scopes.dedup();
    scopes
}

fn action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            provider_id: "microsoft".into(),
            action_id: "microsoft.send_mail".into(),
            label: "Send Outlook email".into(),
            description: "Send email from the connected Microsoft account.".into(),
            fields: vec![
                text_field(
                    "to",
                    "To",
                    "Comma-separated recipient addresses.",
                    true,
                    true,
                ),
                text_field("subject", "Subject", "Email subject line.", true, true),
                textarea_field(
                    "body",
                    "Body",
                    "Email body. HTML is escaped unless enabled below.",
                    true,
                ),
                boolean_field(
                    "html",
                    "Send as HTML",
                    "Escape the body and send it as HTML. Raw HTML tags are not executed.",
                    false,
                ),
            ],
            required_scopes: vec![MAIL_SEND_SCOPE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "microsoft".into(),
            action_id: "microsoft.list_recent_mail".into(),
            label: "List recent Outlook mail".into(),
            description: "List recent message metadata from a mail folder.".into(),
            fields: vec![
                folder_field(),
                text_field(
                    "max_results",
                    "Max results",
                    "Number of messages to return, from 1 to 25.",
                    false,
                    false,
                ),
            ],
            required_scopes: vec![MAIL_READ_SCOPE.into()],
            output_schema_version: 1,
            output_is_untrusted: true,
        },
        ActionDescriptor {
            provider_id: "microsoft".into(),
            action_id: "microsoft.get_mail".into(),
            label: "Get Outlook message".into(),
            description:
                "Fetch bounded message metadata and a preview. Attachments are never returned."
                    .into(),
            fields: vec![text_field(
                "message_id",
                "Message ID",
                "The Graph message ID to fetch.",
                true,
                true,
            )],
            required_scopes: vec![MAIL_READ_SCOPE.into()],
            output_schema_version: 1,
            output_is_untrusted: true,
        },
        ActionDescriptor {
            provider_id: "microsoft".into(),
            action_id: "microsoft.create_calendar_event".into(),
            label: "Create Outlook calendar event".into(),
            description: "Create a calendar event on the connected Microsoft account.".into(),
            fields: vec![
                calendar_field(),
                text_field("subject", "Subject", "Event title.", true, true),
                text_field(
                    "start",
                    "Start",
                    "Local start time as YYYY-MM-DDTHH:MM:SS.",
                    true,
                    true,
                ),
                text_field(
                    "end",
                    "End",
                    "Local end time as YYYY-MM-DDTHH:MM:SS.",
                    true,
                    true,
                ),
                text_field(
                    "time_zone",
                    "Time zone",
                    "IANA time zone, for example America/New_York.",
                    true,
                    false,
                ),
                text_field("location", "Location", "Optional location.", false, true),
                text_field(
                    "attendees",
                    "Attendees",
                    "Optional comma-separated attendee addresses.",
                    false,
                    true,
                ),
                textarea_field("description", "Description", "Optional event notes.", false),
            ],
            required_scopes: vec![CALENDAR_SCOPE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
    ]
}

fn event_descriptors() -> Vec<AppEventDescriptor> {
    vec![
        AppEventDescriptor {
            provider_id: "microsoft".into(),
            event_type: "microsoft.new_mail".into(),
            label: "New Outlook mail".into(),
            description:
                "Run when new mail arrives in a selected folder while Alfred is open. Relays are not used."
                    .into(),
            required_scopes: vec![MAIL_READ_SCOPE.into()],
            delivery_modes: vec![AppEventDeliveryMode::Polling],
            filter_fields: vec![
                folder_field(),
                text_field(
                    "senderContains",
                    "Sender contains",
                    "Optional sender address substring.",
                    false,
                    false,
                ),
                text_field(
                    "subjectContains",
                    "Subject contains",
                    "Optional subject substring.",
                    false,
                    false,
                ),
                boolean_field(
                    "includePreview",
                    "Include preview",
                    "Include Graph's bounded bodyPreview. Full bodies are never stored.",
                    false,
                ),
            ],
            fetches_resource_content: false,
            descriptor_version: 1,
            external_event_id_required: true,
            allowed_attribute_keys: vec![
                "folder".into(),
                "senderAddress".into(),
                "senderContains".into(),
                "subjectContains".into(),
                "includePreview".into(),
                "isRead".into(),
            ],
            poll_interval_seconds: 60,
            pending_cap: 100,
        },
        AppEventDescriptor {
            provider_id: "microsoft".into(),
            event_type: "microsoft.calendar_event_changed".into(),
            label: "Outlook calendar change".into(),
            description:
                "Run when a calendar event is created or updated while Alfred is open.".into(),
            required_scopes: vec![CALENDAR_SCOPE.into()],
            delivery_modes: vec![AppEventDeliveryMode::Polling],
            filter_fields: vec![
                calendar_field(),
                text_field(
                    "subjectContains",
                    "Subject contains",
                    "Optional subject substring.",
                    false,
                    false,
                ),
            ],
            fetches_resource_content: false,
            descriptor_version: 1,
            external_event_id_required: true,
            allowed_attribute_keys: vec![
                "calendar".into(),
                "subjectContains".into(),
                "organizerAddress".into(),
            ],
            poll_interval_seconds: 60,
            pending_cap: 100,
        },
    ]
}

fn folder_field() -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: "folder".into(),
        label: "Folder".into(),
        description: "Mail folder. Defaults to Inbox.".into(),
        kind: ActionFieldKind::ResourceSelector,
        required: false,
        default: Some(Value::String("inbox".into())),
        secret: false,
        option_source: Some("mail_folders".into()),
        options: vec![],
        supports_interpolation: false,
    }
}

fn calendar_field() -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: "calendar".into(),
        label: "Calendar".into(),
        description: "Calendar. Defaults to the account calendar.".into(),
        kind: ActionFieldKind::ResourceSelector,
        required: false,
        default: None,
        secret: false,
        option_source: Some("calendars".into()),
        options: vec![],
        supports_interpolation: false,
    }
}

fn text_field(
    key: &str,
    label: &str,
    description: &str,
    required: bool,
    supports_interpolation: bool,
) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::Text,
        required,
        default: (!required).then(|| Value::String(String::new())),
        secret: false,
        option_source: None,
        options: vec![],
        supports_interpolation,
    }
}

fn textarea_field(
    key: &str,
    label: &str,
    description: &str,
    required: bool,
) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::Textarea,
        required,
        default: (!required).then(|| Value::String(String::new())),
        secret: false,
        option_source: None,
        options: vec![],
        supports_interpolation: true,
    }
}

fn boolean_field(
    key: &str,
    label: &str,
    description: &str,
    default: bool,
) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::Boolean,
        required: false,
        default: Some(Value::Bool(default)),
        secret: false,
        option_source: None,
        options: vec![],
        supports_interpolation: false,
    }
}

impl ActionExecutor for MicrosoftService {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let token = Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            match request.action_id.as_str() {
                "microsoft.send_mail" => self.send_mail(token.as_str(), &request.input).await,
                "microsoft.list_recent_mail" => {
                    self.list_recent_mail(token.as_str(), &request.input).await
                }
                "microsoft.get_mail" => self.get_mail(token.as_str(), &request.input).await,
                "microsoft.create_calendar_event" => {
                    self.create_calendar_event(token.as_str(), &request.input)
                        .await
                }
                _ => Err(ActionError::new(ActionErrorCode::ActionNotFound)),
            }
        })
    }

    fn list_resources<'a>(
        &'a self,
        source: &'a str,
        _field_key: &'a str,
        query: &'a str,
        _page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let token = Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            let needle = query.trim().to_ascii_lowercase();
            match source {
                "mail_folders" => {
                    let folders: GraphCollection<GraphMailFolder> = self
                        .get_json(
                            token.as_str(),
                            "/me/mailFolders",
                            &[("$select", "id,displayName"), ("$top", "50")],
                        )
                        .await?;
                    Ok(ActionResourcePage {
                        items: folders
                            .value
                            .into_iter()
                            .filter(|folder| valid_graph_id(&folder.id))
                            .map(|folder| ActionResourceItem {
                                label: folder
                                    .display_name
                                    .clone()
                                    .filter(|value| !value.is_empty())
                                    .unwrap_or_else(|| folder.id.clone()),
                                id: folder.id,
                            })
                            .filter(|item| {
                                needle.is_empty()
                                    || item.label.to_ascii_lowercase().contains(&needle)
                            })
                            .take(50)
                            .collect(),
                        next_page_token: None,
                    })
                }
                "calendars" => {
                    let calendars: GraphCollection<GraphCalendar> = self
                        .get_json(
                            token.as_str(),
                            "/me/calendars",
                            &[("$select", "id,name"), ("$top", "50")],
                        )
                        .await?;
                    Ok(ActionResourcePage {
                        items: calendars
                            .value
                            .into_iter()
                            .filter(|calendar| valid_graph_id(&calendar.id))
                            .map(|calendar| ActionResourceItem {
                                label: calendar
                                    .name
                                    .clone()
                                    .filter(|value| !value.is_empty())
                                    .unwrap_or_else(|| calendar.id.clone()),
                                id: calendar.id,
                            })
                            .filter(|item| {
                                needle.is_empty()
                                    || item.label.to_ascii_lowercase().contains(&needle)
                            })
                            .take(50)
                            .collect(),
                        next_page_token: None,
                    })
                }
                _ => Err(ActionError::new(ActionErrorCode::InvalidInput)),
            }
        })
    }
}

impl MicrosoftService {
    async fn send_mail(
        &self,
        token: &str,
        input: &BTreeMap<String, Value>,
    ) -> Result<ActionResult, ActionError> {
        let to = required_recipients(input, "to")?;
        let subject = bounded_single_line(input, "subject", MAX_SUBJECT_CHARS)?;
        let body = bounded_text(input, "body", MAX_BODY_CHARS)?;
        if body.trim().is_empty() {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let html = input.get("html").and_then(Value::as_bool).unwrap_or(false);
        let (content_type, content) = if html {
            ("HTML", html_body(&body))
        } else {
            ("Text", body.clone())
        };
        let payload = serde_json::json!({
            "message": {
                "subject": subject,
                "body": { "contentType": content_type, "content": content },
                "toRecipients": to.iter().map(|address| {
                    serde_json::json!({ "emailAddress": { "address": address } })
                }).collect::<Vec<_>>(),
            },
            "saveToSentItems": true,
        });
        let (_, provider_request_id): (GraphNoContent, _) =
            self.post_json(token, "/me/sendMail", &payload).await?;
        Ok(ActionResult {
            summary: if to.len() == 1 {
                "Sent Outlook email".into()
            } else {
                format!("Sent Outlook email to {} recipients", to.len())
            },
            output: serde_json::json!({
                "schemaVersion": 1,
                "recipientCount": to.len(),
                "html": html,
            }),
            artifacts: vec![],
            provider_request_id,
        })
    }

    async fn list_recent_mail(
        &self,
        token: &str,
        input: &BTreeMap<String, Value>,
    ) -> Result<ActionResult, ActionError> {
        let folder = optional_graph_id(input, "folder")?.unwrap_or_else(|| "inbox".into());
        let raw = input
            .get("max_results")
            .and_then(Value::as_str)
            .unwrap_or("10")
            .trim();
        let max_results = if raw.is_empty() {
            10
        } else {
            raw.parse::<usize>()
                .ok()
                .filter(|value| (1..=MAX_MAIL_RESULTS).contains(value))
                .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?
        };
        let top = max_results.to_string();
        let page: GraphCollection<GraphMailMessage> = self
            .get_json(
                token,
                &format!("/me/mailFolders/{folder}/messages"),
                &[
                    ("$top", top.as_str()),
                    (
                        "$select",
                        "id,subject,from,receivedDateTime,webLink,isRead,conversationId,bodyPreview",
                    ),
                    ("$orderby", "receivedDateTime desc"),
                ],
            )
            .await?;
        let messages = page
            .value
            .into_iter()
            .filter_map(|message| summarize_mail(&message, false))
            .take(max_results)
            .collect::<Vec<_>>();
        Ok(ActionResult {
            summary: format!("Listed {} Outlook messages", messages.len()),
            output: serde_json::json!({
                "schemaVersion": 1,
                "folder": folder,
                "messages": messages,
            }),
            artifacts: vec![],
            provider_request_id: None,
        })
    }

    async fn get_mail(
        &self,
        token: &str,
        input: &BTreeMap<String, Value>,
    ) -> Result<ActionResult, ActionError> {
        let message_id = required_graph_id(input, "message_id")?;
        let message: GraphMailMessage = self
            .get_json(
                token,
                &format!("/me/messages/{message_id}"),
                &[(
                    "$select",
                    "id,subject,from,toRecipients,receivedDateTime,webLink,isRead,conversationId,bodyPreview",
                )],
            )
            .await?;
        let summary = summarize_mail(&message, true)
            .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
        let web_link = summary
            .get("webLink")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(ActionResult {
            summary: "Fetched Outlook message".into(),
            output: serde_json::json!({
                "schemaVersion": 1,
                "message": summary,
            }),
            artifacts: web_link
                .map(|uri| {
                    vec![ActionArtifact {
                        kind: "url".into(),
                        label: "Open in Outlook".into(),
                        uri,
                    }]
                })
                .unwrap_or_default(),
            provider_request_id: None,
        })
    }

    async fn create_calendar_event(
        &self,
        token: &str,
        input: &BTreeMap<String, Value>,
    ) -> Result<ActionResult, ActionError> {
        let subject = bounded_single_line(input, "subject", MAX_SUBJECT_CHARS)?;
        let time_zone = bounded_single_line(input, "time_zone", 64)?;
        let start = bounded_single_line(input, "start", 32)?;
        let end = bounded_single_line(input, "end", 32)?;
        validate_event_window(&start, &end, &time_zone)?;
        let location = optional_bounded_text(input, "location", 256)?;
        let description = optional_bounded_text(input, "description", MAX_BODY_CHARS)?;
        let attendees = optional_recipients(input, "attendees")?;
        let mut event = serde_json::json!({
            "subject": subject,
            "start": { "dateTime": start, "timeZone": time_zone },
            "end": { "dateTime": end, "timeZone": time_zone },
        });
        if !location.is_empty() {
            event["location"] = serde_json::json!({ "displayName": location });
        }
        if !description.is_empty() {
            event["body"] = serde_json::json!({ "contentType": "Text", "content": description });
        }
        if !attendees.is_empty() {
            event["attendees"] = Value::Array(
                attendees
                    .iter()
                    .map(|address| {
                        serde_json::json!({
                            "emailAddress": { "address": address },
                            "type": "required",
                        })
                    })
                    .collect(),
            );
        }
        let calendar = optional_graph_id(input, "calendar")?;
        let path = match calendar.as_deref() {
            Some(calendar_id) => format!("/me/calendars/{calendar_id}/events"),
            None => "/me/events".into(),
        };
        let (created, provider_request_id): (GraphEvent, _) =
            self.post_json(token, &path, &event).await?;
        if !valid_graph_id(&created.id) {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
        let web_link = created
            .web_link
            .as_deref()
            .filter(|value| valid_outlook_url(value))
            .map(str::to_owned);
        Ok(ActionResult {
            summary: "Created Outlook calendar event".into(),
            output: serde_json::json!({
                "schemaVersion": 1,
                "eventId": created.id,
                "webLink": web_link,
            }),
            artifacts: web_link
                .map(|uri| {
                    vec![ActionArtifact {
                        kind: "url".into(),
                        label: "Open event".into(),
                        uri,
                    }]
                })
                .unwrap_or_default(),
            provider_request_id,
        })
    }
}

impl AppEventAdapter for MicrosoftService {
    fn poll<'a>(
        &'a self,
        config: &'a AppTriggerConfig,
        _connection: &'a AppConnection,
        cursor: Option<&'a str>,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            match config.event_type.as_str() {
                "microsoft.new_mail" => {
                    self.poll_mail_delta(config, cursor, token.as_str(), cancellation)
                        .await
                }
                "microsoft.calendar_event_changed" => {
                    self.poll_calendar_delta(config, cursor, token.as_str(), cancellation)
                        .await
                }
                _ => Err(AppEventError::new(AppEventErrorCode::EventNotFound)),
            }
        })
    }

    fn list_filter_resources<'a>(
        &'a self,
        field_key: &'a str,
        query: &'a str,
        page_token: Option<&'a str>,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async move {
            let source = match field_key {
                "folder" => "mail_folders",
                "calendar" => "calendars",
                _ => return Err(AppEventError::new(AppEventErrorCode::InvalidInput)),
            };
            let page = self
                .list_resources(
                    source,
                    field_key,
                    query,
                    page_token,
                    connection,
                    tokens,
                    ActionCancellation::never(),
                )
                .await
                .map_err(map_action_error_to_event)?;
            if cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            Ok(AppEventResourcePage {
                items: page
                    .items
                    .into_iter()
                    .map(|item| AppEventResourceItem {
                        id: item.id,
                        label: item.label,
                    })
                    .collect(),
                next_page_token: page.next_page_token,
            })
        })
    }
}

impl MicrosoftService {
    async fn poll_mail_delta(
        &self,
        config: &AppTriggerConfig,
        cursor: Option<&str>,
        token: &str,
        cancellation: AppEventCancellation,
    ) -> Result<AppEventBatch, AppEventError> {
        let folder = config
            .filters
            .get("folder")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("inbox");
        if folder != "inbox" && !valid_graph_id(folder) {
            return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
        }
        let include_preview = config
            .filters
            .get("includePreview")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let sender_contains = filter_text(config, "senderContains");
        let subject_contains = filter_text(config, "subjectContains");
        let mut state = decode_delta_cursor(cursor)?;
        let emit = state.established;
        let mut events = Vec::new();
        let mut pages = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            pages += 1;
            if pages > MAX_DELTA_PAGES {
                break;
            }
            let mut query = vec![(
                "$select",
                "id,subject,from,receivedDateTime,webLink,isRead,conversationId,bodyPreview",
            )];
            if let Some(token) = state.token.as_deref() {
                query.push((state.token_kind.as_str(), token));
            }
            let value = match self
                .graph_get_raw(
                    token,
                    &format!("/me/mailFolders/{folder}/messages/delta"),
                    &query,
                    Some("odata.maxpagesize=25"),
                )
                .await
            {
                Ok(value) => value,
                Err(error) if error.code == ActionErrorCode::InvalidInput && state.established => {
                    state = DeltaCursor::default();
                    continue;
                }
                Err(error) => return Err(map_action_error_to_event(error)),
            };
            let page: GraphCollection<GraphMailMessage> = serde_json::from_value(value)
                .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
            if emit {
                for message in page.value {
                    if message.removed.is_some() {
                        continue;
                    }
                    if let Some(event) = normalize_mail_event(
                        config,
                        folder,
                        &message,
                        include_preview,
                        sender_contains,
                        subject_contains,
                    ) {
                        events.push(event);
                    }
                }
            }
            if let Some(next) = page.next_link.as_deref().and_then(extract_skip_token) {
                state.token = Some(next);
                state.token_kind = "$skiptoken".into();
                continue;
            }
            if let Some(delta) = page.delta_link.as_deref().and_then(extract_delta_token) {
                state.token = Some(delta);
                state.token_kind = "$deltatoken".into();
                state.established = true;
            }
            break;
        }
        Ok(AppEventBatch {
            events,
            cursor: Some(encode_delta_cursor(&state)?),
            ..Default::default()
        })
    }

    async fn poll_calendar_delta(
        &self,
        config: &AppTriggerConfig,
        cursor: Option<&str>,
        token: &str,
        cancellation: AppEventCancellation,
    ) -> Result<AppEventBatch, AppEventError> {
        let calendar = config
            .filters
            .get("calendar")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if calendar.is_some_and(|value| !valid_graph_id(value)) {
            return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
        }
        let subject_contains = filter_text(config, "subjectContains");
        let mut state = decode_delta_cursor(cursor)?;
        let emit = state.established;
        let path = match calendar {
            Some(calendar_id) => format!("/me/calendars/{calendar_id}/events/delta"),
            None => "/me/events/delta".into(),
        };
        let mut events = Vec::new();
        let mut pages = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            pages += 1;
            if pages > MAX_DELTA_PAGES {
                break;
            }
            let mut query = vec![(
                "$select",
                "id,subject,start,end,organizer,webLink,lastModifiedDateTime",
            )];
            if let Some(token) = state.token.as_deref() {
                query.push((state.token_kind.as_str(), token));
            }
            let value = match self
                .graph_get_raw(token, &path, &query, Some("odata.maxpagesize=25"))
                .await
            {
                Ok(value) => value,
                Err(error) if error.code == ActionErrorCode::InvalidInput && state.established => {
                    state = DeltaCursor::default();
                    continue;
                }
                Err(error) => return Err(map_action_error_to_event(error)),
            };
            let page: GraphCollection<GraphEvent> = serde_json::from_value(value)
                .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
            if emit {
                for event in page.value {
                    if event.removed.is_some() {
                        continue;
                    }
                    if let Some(event) =
                        normalize_calendar_event(config, calendar, &event, subject_contains)
                    {
                        events.push(event);
                    }
                }
            }
            if let Some(next) = page.next_link.as_deref().and_then(extract_skip_token) {
                state.token = Some(next);
                state.token_kind = "$skiptoken".into();
                continue;
            }
            if let Some(delta) = page.delta_link.as_deref().and_then(extract_delta_token) {
                state.token = Some(delta);
                state.token_kind = "$deltatoken".into();
                state.established = true;
            }
            break;
        }
        Ok(AppEventBatch {
            events,
            cursor: Some(encode_delta_cursor(&state)?),
            ..Default::default()
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DeltaCursor {
    #[serde(default)]
    established: bool,
    #[serde(default)]
    token: Option<String>,
    #[serde(default = "delta_kind")]
    token_kind: String,
}

fn delta_kind() -> String {
    "$deltatoken".into()
}

fn encode_delta_cursor(cursor: &DeltaCursor) -> Result<String, AppEventError> {
    serde_json::to_vec(cursor)
        .map(|value| URL_SAFE_NO_PAD.encode(value))
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn decode_delta_cursor(value: Option<&str>) -> Result<DeltaCursor, AppEventError> {
    let Some(value) = value else {
        return Ok(DeltaCursor::default());
    };
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::InvalidInput))
}

fn extract_delta_token(link: &str) -> Option<String> {
    extract_query_token(link, "$deltatoken").or_else(|| extract_query_token(link, "deltatoken"))
}

fn extract_skip_token(link: &str) -> Option<String> {
    extract_query_token(link, "$skiptoken").or_else(|| extract_query_token(link, "skiptoken"))
}

fn extract_query_token(link: &str, name: &str) -> Option<String> {
    let url = Url::parse(link).ok()?;
    let token = url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())?;
    (token.len() <= 2048 && !token.is_empty()).then_some(token)
}

fn filter_text<'a>(config: &'a AppTriggerConfig, key: &str) -> Option<&'a str> {
    config
        .filters
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn summarize_mail(message: &GraphMailMessage, include_preview: bool) -> Option<Value> {
    if !valid_graph_id(&message.id) {
        return None;
    }
    let received = message.received_date_time.as_deref()?;
    DateTime::parse_from_rfc3339(received).ok()?;
    let web_link = message
        .web_link
        .as_deref()
        .filter(|value| valid_outlook_url(value));
    let sender = message
        .from
        .as_ref()
        .and_then(|value| value.email_address.as_ref());
    let mut body = serde_json::json!({
        "id": message.id,
        "subject": message.subject.as_deref().map(|value| bounded(value, MAX_SUBJECT_CHARS)),
        "receivedAt": received,
        "isRead": message.is_read.unwrap_or(false),
        "senderName": sender.and_then(|value| value.name.clone()),
        "senderAddress": sender.and_then(|value| value.address.clone()).filter(|value| valid_email_address(value)),
        "webLink": web_link,
        "conversationId": message.conversation_id.clone().filter(|value| valid_graph_id(value)),
    });
    if include_preview {
        body["preview"] = Value::String(
            message
                .body_preview
                .as_deref()
                .map(|value| bounded(value, MAX_PREVIEW_CHARS))
                .unwrap_or_default(),
        );
    }
    if !message.to_recipients.is_empty() {
        body["to"] = Value::Array(
            message
                .to_recipients
                .iter()
                .filter_map(|recipient| {
                    recipient
                        .email_address
                        .as_ref()
                        .and_then(|value| value.address.clone())
                        .filter(|value| valid_email_address(value))
                        .map(Value::String)
                })
                .take(MAX_RECIPIENTS)
                .collect(),
        );
    }
    Some(body)
}

fn normalize_mail_event(
    config: &AppTriggerConfig,
    folder: &str,
    message: &GraphMailMessage,
    include_preview: bool,
    sender_contains: Option<&str>,
    subject_contains: Option<&str>,
) -> Option<NormalizedAppEvent> {
    let summary = summarize_mail(message, include_preview)?;
    let sender = summary
        .get("senderAddress")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let subject = summary
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if sender_contains.is_some_and(|needle| {
        !sender
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }) {
        return None;
    }
    if subject_contains.is_some_and(|needle| {
        !subject
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }) {
        return None;
    }
    let received = summary.get("receivedAt")?.as_str()?.to_owned();
    let actor = summary
        .get("senderName")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or(if sender.is_empty() {
            None
        } else {
            Some(sender)
        })
        .map(str::to_owned);
    let mut attributes = BTreeMap::from([
        ("folder".into(), Value::String(folder.into())),
        (
            "isRead".into(),
            Value::Bool(message.is_read.unwrap_or(false)),
        ),
        ("includePreview".into(), Value::Bool(include_preview)),
    ]);
    if !sender.is_empty() {
        attributes.insert("senderAddress".into(), Value::String(sender.into()));
    }
    if let Some(filter) = sender_contains {
        attributes.insert("senderContains".into(), Value::String(filter.into()));
    }
    if let Some(filter) = subject_contains {
        attributes.insert("subjectContains".into(), Value::String(filter.into()));
    }
    Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "microsoft".into(),
        event_type: config.event_type.clone(),
        connection_id: config.connection_id.clone(),
        external_event_id: message.id.clone(),
        occurred_at: received,
        subject: (!subject.is_empty()).then(|| subject.to_owned()),
        actor,
        resource_url: summary
            .get("webLink")
            .and_then(Value::as_str)
            .map(str::to_owned),
        preview: include_preview
            .then(|| {
                summary
                    .get("preview")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten(),
        attributes,
    })
}

fn normalize_calendar_event(
    config: &AppTriggerConfig,
    calendar: Option<&str>,
    event: &GraphEvent,
    subject_contains: Option<&str>,
) -> Option<NormalizedAppEvent> {
    if !valid_graph_id(&event.id) {
        return None;
    }
    let subject = event
        .subject
        .as_deref()
        .map(|value| bounded(value, MAX_SUBJECT_CHARS))
        .unwrap_or_default();
    if subject_contains.is_some_and(|needle| {
        !subject
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }) {
        return None;
    }
    let occurred = event.last_modified_date_time.as_deref().or(event
        .start
        .as_ref()
        .and_then(|value| value.date_time.as_deref()))?;
    let occurred_at = DateTime::parse_from_rfc3339(occurred)
        .ok()
        .map(|value| value.to_rfc3339())
        .or_else(|| Some(format!("{occurred}Z")))?;
    let organizer = event
        .organizer
        .as_ref()
        .and_then(|value| value.email_address.as_ref());
    let mut attributes = BTreeMap::new();
    if let Some(calendar) = calendar {
        attributes.insert("calendar".into(), Value::String(calendar.into()));
    }
    if let Some(filter) = subject_contains {
        attributes.insert("subjectContains".into(), Value::String(filter.into()));
    }
    if let Some(address) = organizer
        .and_then(|value| value.address.clone())
        .filter(|value| valid_email_address(value))
    {
        attributes.insert("organizerAddress".into(), Value::String(address));
    }
    Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "microsoft".into(),
        event_type: config.event_type.clone(),
        connection_id: config.connection_id.clone(),
        external_event_id: event.id.clone(),
        occurred_at,
        subject: (!subject.is_empty()).then_some(subject),
        actor: organizer.and_then(|value| value.name.clone()),
        resource_url: event
            .web_link
            .as_deref()
            .filter(|value| valid_outlook_url(value))
            .map(str::to_owned),
        preview: None,
        attributes,
    })
}

fn valid_microsoft_auth_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("login.microsoftonline.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().ends_with("/oauth2/v2.0/authorize")
}

fn valid_outlook_url(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && matches!(
                url.host_str(),
                Some("outlook.office.com")
                    | Some("outlook.office365.com")
                    | Some("outlook.live.com")
            )
    })
}

fn valid_graph_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_GRAPH_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'+'))
}

fn valid_email_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !value.is_empty()
        && value.len() <= MAX_ADDRESS_CHARS
        && !local.is_empty()
        && local.len() <= 64
        && !domain.is_empty()
        && domain.len() <= 253
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b'@')
        })
}

fn required_recipients(
    input: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Vec<String>, ActionError> {
    let recipients = optional_recipients(input, key)?;
    if recipients.is_empty() {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    } else {
        Ok(recipients)
    }
}

fn optional_recipients(
    input: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Vec<String>, ActionError> {
    let raw = input.get(key).and_then(Value::as_str).unwrap_or_default();
    if raw.len() > 8 * 1024 || raw.chars().any(|character| character == '\0') {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    let recipients = raw
        .split([',', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if recipients.len() > MAX_RECIPIENTS
        || recipients
            .iter()
            .any(|recipient| !valid_email_address(recipient))
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(recipients)
}

fn bounded_single_line(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ActionError> {
    let value = bounded_text(input, key, max_chars)?;
    if value
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '\0'))
        || value.trim().is_empty()
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(value)
}

fn bounded_text(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ActionError> {
    let value = input.get(key).and_then(Value::as_str).unwrap_or_default();
    if value.chars().count() > max_chars || value.chars().any(|character| character == '\0') {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(value.to_owned())
}

fn optional_bounded_text(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ActionError> {
    bounded_text(input, key, max_chars)
}

fn required_graph_id(input: &BTreeMap<String, Value>, key: &str) -> Result<String, ActionError> {
    optional_graph_id(input, key)?.ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))
}

fn optional_graph_id(
    input: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<String>, ActionError> {
    let value = input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match value {
        Some(value) if valid_graph_id(value) || value == "inbox" => Ok(Some(value.to_owned())),
        Some(_) => Err(ActionError::new(ActionErrorCode::InvalidInput)),
        None => Ok(None),
    }
}

fn html_body(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!("<p>{}</p>", escaped.replace('\n', "<br>"))
}

fn bounded(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_owned()
    } else {
        format!("{}…", value.chars().take(max_chars).collect::<String>())
    }
}

fn validate_event_window(start: &str, end: &str, time_zone: &str) -> Result<(), ActionError> {
    let tz: Tz = time_zone
        .parse()
        .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?;
    let start = parse_local_datetime(start, tz)?;
    let end = parse_local_datetime(end, tz)?;
    if end <= start {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(())
}

fn parse_local_datetime(value: &str, tz: Tz) -> Result<chrono::DateTime<Tz>, ActionError> {
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f"))
        .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?;
    match naive.and_local_timezone(tz) {
        chrono::LocalResult::Single(value) | chrono::LocalResult::Ambiguous(value, _) => Ok(value),
        chrono::LocalResult::None => Err(ActionError::new(ActionErrorCode::InvalidInput)),
    }
}

async fn token_post_form<T: DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
) -> Result<T, ActionError> {
    let request = graph_client()
        .post(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .form(form);
    send_graph_json(request, false)
        .await
        .map(|response| response.value)
}

async fn get_unauthenticated_json<T: DeserializeOwned>(url: &str) -> Result<T, ActionError> {
    let request = graph_client()
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    send_graph_json(request, false)
        .await
        .map(|response| response.value)
}

fn graph_request(client: Client, token: &str, method: Method, url: &str) -> RequestBuilder {
    client
        .request(method, url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
}

fn graph_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("Microsoft Graph HTTP client")
}

async fn send_graph_json<T: DeserializeOwned>(
    request: RequestBuilder,
    mutation: bool,
) -> Result<GraphJsonResponse<T>, ActionError> {
    let response = request.send().await.map_err(|error| {
        if mutation && (error.is_timeout() || !error.is_connect()) {
            ActionError::new(ActionErrorCode::DeliveryUnknown)
        } else {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
    })?;
    parse_graph_response(response, mutation).await
}

async fn parse_graph_response<T: DeserializeOwned>(
    response: Response,
    mutation: bool,
) -> Result<GraphJsonResponse<T>, ActionError> {
    let status = response.status();
    let request_id = response
        .headers()
        .get("request-id")
        .or_else(|| response.headers().get("client-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let retry_after = graph_retry_after(&response);
    if !status.is_success() {
        let graph_code = graph_error_code(response).await;
        let code = match status {
            StatusCode::UNAUTHORIZED => ActionErrorCode::ProviderUnauthorized,
            StatusCode::TOO_MANY_REQUESTS => ActionErrorCode::RateLimited,
            StatusCode::FORBIDDEN => match graph_code.as_deref() {
                Some("ErrorAccessDenied") | Some("Authorization_RequestDenied") => {
                    ActionErrorCode::ScopeMissing
                }
                Some("TooManyRetries") => ActionErrorCode::RateLimited,
                _ => ActionErrorCode::ScopeMissing,
            },
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::GONE => {
                ActionErrorCode::InvalidInput
            }
            status if status.is_server_error() && mutation => ActionErrorCode::DeliveryUnknown,
            status if status.is_server_error() => ActionErrorCode::ProviderUnavailable,
            _ if mutation => ActionErrorCode::DeliveryUnknown,
            _ => ActionErrorCode::ProviderUnavailable,
        };
        let mut error = if code == ActionErrorCode::RateLimited {
            ActionError::rate_limited(retry_after)
        } else {
            ActionError::new(code)
        };
        if let Some(request_id) = request_id.as_deref() {
            error = error.with_request_id(request_id);
        }
        return Err(error);
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > RESPONSE_LIMIT)
    {
        return Err(ActionError::new(if mutation {
            ActionErrorCode::DeliveryUnknown
        } else {
            ActionErrorCode::OutputTooLarge
        }));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::ProviderUnavailable
            })
        })?;
        if bytes.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::OutputTooLarge
            }));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = if bytes.is_empty() {
        serde_json::from_value(serde_json::json!({}))
            .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?
    } else {
        serde_json::from_slice(&bytes).map_err(|_| {
            ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::OutputInvalid
            })
        })?
    };
    Ok(GraphJsonResponse { value, request_id })
}

async fn graph_error_code(response: Response) -> Option<String> {
    if response
        .content_length()
        .is_some_and(|length| length as usize > ERROR_HINT_LIMIT)
    {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return None;
        };
        if bytes.len().saturating_add(chunk.len()) > ERROR_HINT_LIMIT {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let code = value.get("error")?.get("code")?.as_str()?;
    (code.len() <= 80).then(|| code.to_owned())
}

struct GraphJsonResponse<T> {
    value: T,
    request_id: Option<String>,
}

fn graph_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.min(86_400))
}

fn map_action_error_to_event(error: ActionError) -> AppEventError {
    let code = match error.code {
        ActionErrorCode::ConnectionRequired => AppEventErrorCode::ConnectionRequired,
        ActionErrorCode::ScopeMissing => AppEventErrorCode::ScopeMissing,
        ActionErrorCode::RateLimited => AppEventErrorCode::RateLimited,
        ActionErrorCode::ProviderUnauthorized => AppEventErrorCode::ProviderUnauthorized,
        ActionErrorCode::ProviderUnavailable | ActionErrorCode::DeliveryUnknown => {
            AppEventErrorCode::ProviderUnavailable
        }
        ActionErrorCode::TimedOut => AppEventErrorCode::TimedOut,
        ActionErrorCode::Cancelled => AppEventErrorCode::Cancelled,
        _ => AppEventErrorCode::EventInvalid,
    };
    let mut mapped = AppEventError::new(code);
    if let Some(seconds) = error.retry_after_seconds {
        mapped = mapped.retry_after(seconds);
    }
    mapped
}

fn map_native_oauth_error(error: NativeOAuthError) -> IntegrationCommandError {
    match error {
        NativeOAuthError::Cancelled => command_error(
            "microsoft_pairing_cancelled",
            "The Microsoft authorization attempt was cancelled.",
            false,
        ),
        NativeOAuthError::Timeout => microsoft_pairing_expired(),
        NativeOAuthError::AuthorizationDenied => command_error(
            "microsoft_authorization_denied",
            "Microsoft authorization was not completed.",
            false,
        ),
        NativeOAuthError::CallbackUnavailable => command_error(
            "microsoft_unavailable",
            "The local authorization callback could not be opened.",
            true,
        ),
        NativeOAuthError::StateMismatch => command_error(
            "microsoft_invalid_response",
            "Microsoft authorization could not be verified.",
            false,
        ),
        _ => command_error(
            "microsoft_invalid_response",
            "Microsoft returned an invalid authorization callback.",
            false,
        ),
    }
}

fn map_token_exchange_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized
        | ActionErrorCode::ScopeMissing
        | ActionErrorCode::InvalidInput => command_error(
            "microsoft_authorization_expired",
            "Microsoft rejected the authorization code. Start a new connection attempt.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Microsoft is rate limiting authorization attempts. Try again later.",
            true,
        ),
        _ => command_error(
            "microsoft_unavailable",
            "Microsoft authorization is temporarily unavailable.",
            true,
        ),
    }
}

fn map_id_token_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Microsoft is rate limiting authorization attempts. Try again later.",
            true,
        ),
        _ => command_error(
            "microsoft_invalid_id_token",
            "Microsoft returned an ID token that could not be verified.",
            false,
        ),
    }
}

fn map_connect_action_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized => command_error(
            "microsoft_authorization_expired",
            "Microsoft rejected the authorization. Start a new connection attempt.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Microsoft is rate limiting requests. Try again later.",
            true,
        ),
        _ => command_error(
            "microsoft_unavailable",
            "Microsoft could not validate this connection. Try again.",
            true,
        ),
    }
}

fn map_token_store_connect_error(error: TokenStoreError) -> IntegrationCommandError {
    match error {
        TokenStoreError::Locked => command_error(
            "credential_store_locked",
            "Unlock the system credential store and try again.",
            true,
        ),
        _ => credential_write_error(),
    }
}

fn command_error(code: &str, message: &str, recoverable: bool) -> IntegrationCommandError {
    IntegrationCommandError::new(code, message, recoverable)
}

fn microsoft_state_error() -> IntegrationCommandError {
    command_error(
        "microsoft_pairing_failed",
        "The Microsoft authorization attempt could not be updated.",
        true,
    )
}

fn microsoft_pairing_expired() -> IntegrationCommandError {
    command_error(
        "microsoft_pairing_expired",
        "This Microsoft authorization attempt expired. Start again.",
        false,
    )
}

fn microsoft_invalid_response() -> IntegrationCommandError {
    command_error(
        "microsoft_invalid_response",
        "Microsoft returned an invalid authorization response.",
        true,
    )
}

fn connection_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "Microsoft was authorized, but the connection metadata could not be saved.",
        true,
    )
}

fn credential_write_error() -> IntegrationCommandError {
    command_error(
        "microsoft_connection_failed",
        "The Microsoft credential could not be saved securely.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use tiny_http::{Header as TinyHeader, Response as TinyResponse, Server};

    const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCbD1DmXwA2l5zq
y+Z1XxsFcEx637gCVhQuPmR4jc+vJkUDOMj1jmpGq/4yWqUI+8IIl3qwRim2yxe8
Ht/nIlV07FZArg8DhKLfS9h9SlCk9eIq5r1yD052h7JiBgl53eOzcl1q2CHXm47z
3VvVTD6Cn4AVMbzqn5iGImJA2eh5hovvJSiMxWbogY+qIelzMwxSHyRqx4SqeDDR
mrJ2IZ0WugGjdJ0SJQ0W7GbARdPL+p4rprsaHc5SOpLD8sqsJygtW0NBIWeOXWUp
AORMYRI682FnxYxZxy6AQ1MzpuNqqlpvma3Qm7VmzuA6/TtlAataOhby7w0yEsdy
bUkKEhCFAgMBAAECggEAJjSVIpoER68+IOUqDL/o/MNReNIQOPUkJkvFviV052tG
xFc0vro/KdgdeyM1+Dtt8Od/+ZhkFU+/sqCp2v2s8DD+RJQOi3eeGOZLen151tdl
yVvOkGDAnLhtJbVmltIrFU8nwNhtqD4CMEiZpLnTSiSKLs6xRV8a+evVaTS30N9J
LplypY+acAk10RKcfno9TDAsoVVh1vPM8jK6jGDogWwP6KKrK4GmJd4mTjdEj3fF
UnRoQjJ6m532R1PBUXO1ih1Qfva3GZ5zdfxbOZXwtOnfpOPMd1Fk3bG8ogRx31M7
9wYpBBGEhveWknlhB/Cv0yrkNomQJAVLOdKwNmpBeQKBgQDYRsJOo27GDs3qE3VD
F9W80ZmYSnH9J2vOSlu22IPQslW3ClO7QZ9BGcXS/NEIzCq+jX7VKCWvlVR7kpSs
Ufy5U6jBSnBXGGN0tf8WLDMbse4Wa2bajv3fTcc+uzq74vHeRXkr/ZnLj8QuX313
OKrQJMEiuSIpBBdnfkYJGeoIWQKBgQC3iizPcKYdRW67yoBAMeNE5aoEepKWNPSM
89s1UjOVx3jD2e7KsHUn8YZ5D2vrIn6TWrwaLFip5jdUf4NzlPRWMWHFfdom4GKI
OYZYi179V7wJVfmxiFyrbZfct5kMspdPi2IEfW7ju47HRk8+B+NuECdUZRWQvKAx
YMF9J7hEDQKBgExj0cXM3BeAqyJ+dPCZvpjOv52WzeRIxD887GAM4aIZG0VnlGOT
rhhkbgcz3PFqi756Y84OPCFkcU6kW3byDn23GugKzts0dgyHK+489mBV3G52yQFx
eCIjarixkPFEG4ISr9Xl4SiRQw3OFJbDoTGbicwl7/bkxw96/mnAiXUhAoGBAIzQ
hSquKbRhdeC8L4ORAuX0Mmn3RInbnRibaz5Qj+VFQgE5LfzyPyBjLKGq1Eh1kZkq
TxDhnzDSwPaiUk8WJBQRFQs5UGrtUotjXxCF9V33tvuOq+CqVzbrAU1EyzazumU7
8fqx5abxWkzHQ7q6wKHL4PDeERqXrWvU6P5FwBjtAoGBAIb0JFyQkIP5vodB0Eih
lUc1zoNRz4jmliZYuCmizju36OJFw5irEPxxkMR6ZJ/Fi136oR0EUyLWNx0AZ9Oj
B7yVX3VgB0sXp9JXTDisdK+irll+cHuW5WnU1QvtNlOy6TQ0lTHcZO2PeArGIRe/
71lQJtKbveio4rh2B9ZjcYmH
-----END PRIVATE KEY-----";
    const TEST_N: &str = "mw9Q5l8ANpec6svmdV8bBXBMet-4AlYULj5keI3PryZFAzjI9Y5qRqv-MlqlCPvCCJd6sEYptssXvB7f5yJVdOxWQK4PA4Si30vYfUpQpPXiKua9cg9OdoeyYgYJed3js3Jdatgh15uO891b1Uw-gp-AFTG86p-YhiJiQNnoeYaL7yUojMVm6IGPqiHpczMMUh8kaseEqngw0ZqydiGdFroBo3SdEiUNFuxmwEXTy_qeK6a7Gh3OUjqSw_LKrCcoLVtDQSFnjl1lKQDkTGESOvNhZ8WMWccugENTM6bjaqpab5mt0Ju1Zs7gOv07ZQGrWjoW8u8NMhLHcm1JChIQhQ";
    const TEST_CLIENT: &str = "public-client-id";
    const TEST_OID: &str = "11111111-2222-3333-4444-555555555555";
    const TEST_TID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    fn test_service(base: String) -> MicrosoftService {
        MicrosoftService {
            client_id: Some(TEST_CLIENT.into()),
            tenant: "common".into(),
            login_base: base.clone(),
            graph_base: format!("{base}/graph"),
            preferred_ports: vec![],
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn connection() -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "microsoft".into(),
            display_name: Some("Ada Lovelace".into()),
            external_account_id: Some(TEST_OID.into()),
            external_tenant_id: Some(TEST_TID.into()),
            connection_mode: "native_oauth".into(),
            identity_key: canonical_identity_key(
                "microsoft",
                "native_oauth",
                &[TEST_TID, TEST_OID],
            ),
            scopes: vec![
                MAIL_SEND_SCOPE.into(),
                MAIL_READ_SCOPE.into(),
                CALENDAR_SCOPE.into(),
            ],
            provider_metadata: BTreeMap::from([("email".into(), "ada@example.com".into())]),
            status: ConnectionStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "credential".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    async fn token_capability() -> TokenAccessCapability {
        let store = Arc::new(InMemoryTokenStore::default());
        store
            .put(
                "credential",
                &CredentialEnvelope::new("ms-access-fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    fn json_header() -> TinyHeader {
        TinyHeader::from_bytes("Content-Type", "application/json").expect("header")
    }

    fn jwks_body() -> String {
        format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":"{TEST_N}","e":"AQAB"}}]}}"#
        )
    }

    fn sign_id_token(nonce: &str, tid: &str, oid: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key".into());
        let claims = serde_json::json!({
            "aud": TEST_CLIENT,
            "iss": format!("{LOGIN_ORIGIN}/{tid}/v2.0"),
            "exp": Utc::now().timestamp() + 600,
            "nbf": Utc::now().timestamp() - 10,
            "sub": oid,
            "oid": oid,
            "tid": tid,
            "nonce": nonce,
            "preferred_username": "ada@example.com",
            "name": "Ada Lovelace",
            "ver": "2.0",
        });
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(TEST_PEM.as_bytes()).expect("encoding key"),
        )
        .expect("sign")
    }

    fn send_callback(authorization_url: &str, code: &str) {
        let url = Url::parse(authorization_url).expect("authorization url");
        let redirect: Url = url
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .expect("redirect uri")
            .1
            .parse()
            .expect("redirect");
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .expect("state")
            .1
            .to_string();
        let address = SocketAddr::from(([127, 0, 0, 1], redirect.port().expect("port")));
        let mut stream = loop {
            if let Ok(stream) = TcpStream::connect(address) {
                break stream;
            }
            std::thread::yield_now();
        };
        write!(
            stream,
            "GET /oauth/callback?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        )
        .expect("write callback");
    }

    fn query_value(url: &str, name: &str) -> String {
        Url::parse(url)
            .expect("url")
            .query_pairs()
            .find(|(key, _)| key == name)
            .expect("query")
            .1
            .to_string()
    }

    #[test]
    fn descriptors_are_secret_free_and_use_mail_read_basic() {
        let descriptors = action_descriptors();
        assert_eq!(descriptors.len(), 4);
        assert!(descriptors.iter().all(|descriptor| {
            descriptor.provider_id == "microsoft"
                && descriptor.fields.iter().all(|field| {
                    !field.secret
                        && field.key.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        })
                })
        }));
        assert!(descriptors.iter().any(|descriptor| {
            descriptor.action_id == "microsoft.send_mail"
                && descriptor.required_scopes == vec![MAIL_SEND_SCOPE]
        }));
        assert!(descriptors.iter().any(|descriptor| {
            descriptor.action_id == "microsoft.get_mail"
                && descriptor.required_scopes == vec![MAIL_READ_SCOPE]
        }));
        assert!(!descriptors
            .iter()
            .flat_map(|descriptor| descriptor.required_scopes.iter())
            .any(|scope| scope == "Mail.Read" || scope.contains("Mail.ReadWrite")));
        let events = event_descriptors();
        assert!(events
            .iter()
            .all(|descriptor| descriptor.delivery_modes == vec![AppEventDeliveryMode::Polling]));
    }

    #[test]
    fn html_bodies_are_escaped_and_timezones_are_dst_aware() {
        assert_eq!(
            html_body("<script>alert(1)</script>\nhello"),
            "<p>&lt;script&gt;alert(1)&lt;/script&gt;<br>hello</p>"
        );
        validate_event_window(
            "2026-04-15T09:00:00",
            "2026-04-15T10:00:00",
            "Pacific/Auckland",
        )
        .expect("valid window");
        assert!(validate_event_window(
            "2026-04-15T10:00:00",
            "2026-04-15T09:00:00",
            "Pacific/Auckland"
        )
        .is_err());
        assert!(validate_event_window(
            "2026-03-08T02:30:00",
            "2026-03-08T03:30:00",
            "America/New_York"
        )
        .is_err());
        assert!(
            validate_event_window("2026-04-15T09:00:00", "2026-04-15T10:00:00", "Not/AZone")
                .is_err()
        );
    }

    #[test]
    fn tenant_policy_blocks_personal_accounts_for_organizations() {
        let mut service = test_service("http://127.0.0.1".into());
        service.tenant = "organizations".into();
        let error = service.enforce_tenant_policy(MSA_TENANT).expect_err("msa");
        assert_eq!(error.code, "microsoft_personal_account_blocked");
        service.tenant = TEST_TID.into();
        let error = service
            .enforce_tenant_policy("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")
            .expect_err("other tenant");
        assert_eq!(error.code, "microsoft_account_mismatch");
        service.tenant = "common".into();
        service.enforce_tenant_policy(TEST_TID).expect("work");
        service.enforce_tenant_policy(MSA_TENANT).expect("personal");
    }

    #[tokio::test]
    async fn native_pkce_validates_id_token_and_does_not_leak_secrets() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .expect("loopback test gate");
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let nonce_slot = Arc::new(Mutex::new(None::<String>));
        let nonce_writer = nonce_slot.clone();
        let responder = std::thread::spawn(move || {
            let nonce = loop {
                if let Some(nonce) = nonce_writer.lock().expect("nonce").clone() {
                    break nonce;
                }
                std::thread::yield_now();
            };
            let mut token_request = server.recv().expect("token");
            assert!(token_request.url().ends_with("/oauth2/v2.0/token"));
            let mut body = String::new();
            token_request
                .as_reader()
                .read_to_string(&mut body)
                .expect("body");
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("code_verifier="));
            assert!(!body.contains("client_secret"));
            let id_token = sign_id_token(&nonce, TEST_TID, TEST_OID);
            token_request
                .respond(
                    TinyResponse::from_string(format!(
                        r#"{{"access_token":"ms-access-fixture","token_type":"Bearer","expires_in":3599,"refresh_token":"ms-refresh-fixture","id_token":"{id_token}","scope":"openid profile offline_access User.Read Mail.Send"}}"#
                    ))
                    .with_header(json_header()),
                )
                .expect("token response");
            let jwks = server.recv().expect("jwks");
            assert!(jwks.url().contains("/discovery/v2.0/keys"));
            jwks.respond(TinyResponse::from_string(jwks_body()).with_header(json_header()))
                .expect("jwks response");
            let me = server.recv().expect("me");
            assert!(me.url().starts_with("/graph/me?"));
            me.respond(
                TinyResponse::from_string(format!(
                    r#"{{"id":"{TEST_OID}","displayName":"Ada Lovelace","userPrincipalName":"ada@example.com","mail":"ada@example.com"}}"#
                ))
                .with_header(json_header()),
            )
            .expect("me response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let prepared = service
            .prepare_authorization(
                &Db::open_in_memory().expect("db"),
                MicrosoftPrepareInput {
                    send_mail: true,
                    ..MicrosoftPrepareInput::default()
                },
            )
            .expect("prepare");
        let url = Url::parse(&prepared.authorization_url).expect("authorization url");
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "code_challenge_method" && value == "S256"));
        assert!(url.query_pairs().any(|(key, _)| key == "nonce"));
        *nonce_slot.lock().expect("nonce") =
            Some(query_value(&prepared.authorization_url, "nonce"));
        let session_id = prepared.session_id.clone();
        let callback_url = prepared.authorization_url.clone();
        let waiter = std::thread::spawn(move || send_callback(&callback_url, "authorization-code"));
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let result = service
            .complete_authorization(&db, store.clone(), &session_id)
            .await
            .expect("complete");
        waiter.join().expect("callback thread");
        responder.join().expect("responder");
        assert_eq!(result.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(result.external_tenant_id.as_deref(), Some(TEST_TID));
        assert!(result.scopes.contains(&MAIL_SEND_SCOPE.to_string()));
        let saved = db
            .list_app_connections()
            .expect("connections")
            .pop()
            .expect("saved");
        let credential = store.get(&saved.credential_ref).expect("credential");
        assert_eq!(credential.access_token, "ms-access-fixture");
        assert_eq!(
            credential.refresh_token.as_deref(),
            Some("ms-refresh-fixture")
        );
        let serialized = serde_json::to_string(&result).expect("dto");
        assert!(!serialized.contains("ms-access-fixture"));
        assert!(!serialized.contains("ms-refresh-fixture"));
        assert!(!serialized.contains("authorization-code"));
    }

    #[tokio::test]
    async fn nonce_mismatch_does_not_create_a_connection() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .expect("loopback test gate");
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let token_request = server.recv().expect("token");
            let id_token = sign_id_token("wrong-nonce", TEST_TID, TEST_OID);
            token_request
                .respond(
                    TinyResponse::from_string(format!(
                        r#"{{"access_token":"ms-access-fixture","token_type":"Bearer","expires_in":3599,"refresh_token":"ms-refresh-fixture","id_token":"{id_token}","scope":"openid profile offline_access User.Read"}}"#
                    ))
                    .with_header(json_header()),
                )
                .expect("token response");
            let jwks = server.recv().expect("jwks");
            jwks.respond(TinyResponse::from_string(jwks_body()).with_header(json_header()))
                .expect("jwks");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let prepared = service
            .prepare_authorization(&db, MicrosoftPrepareInput::default())
            .expect("prepare");
        let session_id = prepared.session_id.clone();
        let callback_url = prepared.authorization_url.clone();
        let waiter = std::thread::spawn(move || send_callback(&callback_url, "authorization-code"));
        let error = service
            .complete_authorization(&db, store, &session_id)
            .await
            .expect_err("nonce");
        waiter.join().expect("callback");
        responder.join().expect("responder");
        assert_eq!(error.code, "microsoft_invalid_id_token");
        assert!(db.list_app_connections().expect("connections").is_empty());
    }

    #[tokio::test]
    async fn cancelling_an_attempt_stops_the_blocking_wait() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .expect("loopback test gate");
        let service = test_service("http://127.0.0.1:0".into());
        let db = Db::open_in_memory().expect("database");
        let prepared = service
            .prepare_authorization(&db, MicrosoftPrepareInput::default())
            .expect("prepare");
        let session_id = prepared.session_id.clone();
        service.cancel_authorization(&session_id);
        let store = Arc::new(InMemoryTokenStore::default());
        let error = service
            .complete_authorization(&db, store, &session_id)
            .await
            .expect_err("cancelled");
        assert_eq!(error.code, "microsoft_pairing_cancelled");
    }

    fn mail_request(action_id: &str, input: BTreeMap<String, Value>) -> ValidatedActionRequest {
        ValidatedActionRequest {
            connection_id: "connection".into(),
            provider_id: "microsoft".into(),
            action_id: action_id.into(),
            input,
        }
    }

    #[tokio::test]
    async fn send_mail_escapes_html_and_maps_ambiguous_failures() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut send = server.recv().expect("send");
            assert_eq!(send.url(), "/graph/me/sendMail");
            let mut body = String::new();
            send.as_reader().read_to_string(&mut body).expect("body");
            assert!(!body.contains("ms-access-fixture"));
            assert!(body.contains("&lt;script&gt;"));
            send.respond(
                TinyResponse::empty(202)
                    .with_header(TinyHeader::from_bytes("request-id", "req-send-1").expect("id")),
            )
            .expect("send response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &mail_request(
                    "microsoft.send_mail",
                    BTreeMap::from([
                        ("to".into(), Value::String("other@example.com".into())),
                        ("subject".into(), Value::String("Hello".into())),
                        ("body".into(), Value::String("<script>x</script>".into())),
                        ("html".into(), Value::Bool(true)),
                    ]),
                ),
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("send");
        responder.join().expect("responder");
        assert_eq!(result.output["html"], true);
        assert_eq!(result.provider_request_id.as_deref(), Some("req-send-1"));
        let serialized = serde_json::to_string(&result).expect("result");
        assert!(!serialized.contains("ms-access-fixture"));
        assert!(!serialized.contains("<script>"));

        let server_two = Server::http(("127.0.0.1", 0)).expect("server");
        let port_two = server_two.server_addr().to_ip().expect("address").port();
        let responder_two = std::thread::spawn(move || {
            let send = server_two.recv().expect("send");
            send.respond(TinyResponse::empty(502)).expect("502");
        });
        let service = test_service(format!("http://127.0.0.1:{port_two}"));
        let error = service
            .execute(
                &mail_request(
                    "microsoft.send_mail",
                    BTreeMap::from([
                        ("to".into(), Value::String("other@example.com".into())),
                        ("subject".into(), Value::String("Hello".into())),
                        ("body".into(), Value::String("Hi".into())),
                    ]),
                ),
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect_err("unknown");
        responder_two.join().expect("responder two");
        assert_eq!(error.code, ActionErrorCode::DeliveryUnknown);
    }

    #[tokio::test]
    async fn graph_errors_map_401_403_and_429() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let unauthorized = server.recv().expect("401");
            unauthorized.respond(TinyResponse::empty(401)).expect("401");
            let forbidden = server.recv().expect("403");
            forbidden
                .respond(
                    TinyResponse::from_string(r#"{"error":{"code":"ErrorAccessDenied"}}"#)
                        .with_status_code(403)
                        .with_header(json_header()),
                )
                .expect("403");
            let limited = server.recv().expect("429");
            limited
                .respond(
                    TinyResponse::empty(429)
                        .with_header(TinyHeader::from_bytes("Retry-After", "12").expect("retry")),
                )
                .expect("429");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let unauthorized = service
            .get_json::<Value>("ms-access-fixture", "/me", &[])
            .await
            .expect_err("401");
        assert_eq!(unauthorized.code, ActionErrorCode::ProviderUnauthorized);
        let forbidden = service
            .get_json::<Value>("ms-access-fixture", "/me", &[])
            .await
            .expect_err("403");
        assert_eq!(forbidden.code, ActionErrorCode::ScopeMissing);
        let limited = service
            .get_json::<Value>("ms-access-fixture", "/me", &[])
            .await
            .expect_err("429");
        responder.join().expect("responder");
        assert_eq!(limited.code, ActionErrorCode::RateLimited);
        assert_eq!(limited.retry_after_seconds, Some(12));
    }

    #[tokio::test]
    async fn list_and_get_mail_return_metadata_without_full_bodies() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let list = server.recv().expect("list");
            assert!(list.url().contains("/graph/me/mailFolders/inbox/messages?"));
            assert!(list.url().contains("bodyPreview"));
            assert!(!list.url().contains("uniqueBody"));
            list.respond(
                TinyResponse::from_string(
                    r#"{"value":[{"id":"msg1","subject":"Hello","from":{"emailAddress":{"name":"Ada","address":"ada@example.com"}},"receivedDateTime":"2026-08-19T10:00:00Z","webLink":"https://outlook.office.com/mail/id/msg1","isRead":false,"conversationId":"conv1","bodyPreview":"Preview only","body":{"content":"FULL BODY SECRET"}}]}"#,
                )
                .with_header(json_header()),
            )
            .expect("list");
            let get = server.recv().expect("get");
            assert!(get.url().starts_with("/graph/me/messages/msg1?"));
            get.respond(
                TinyResponse::from_string(
                    r#"{"id":"msg1","subject":"Hello","from":{"emailAddress":{"name":"Ada","address":"ada@example.com"}},"toRecipients":[{"emailAddress":{"address":"other@example.com"}}],"receivedDateTime":"2026-08-19T10:00:00Z","webLink":"https://outlook.office.com/mail/id/msg1","isRead":true,"conversationId":"conv1","bodyPreview":"Preview only"}"#,
                )
                .with_header(json_header()),
            )
            .expect("get");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let listed = service
            .execute(
                &mail_request(
                    "microsoft.list_recent_mail",
                    BTreeMap::from([("max_results".into(), Value::String("10".into()))]),
                ),
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("list");
        let fetched = service
            .execute(
                &mail_request(
                    "microsoft.get_mail",
                    BTreeMap::from([("message_id".into(), Value::String("msg1".into()))]),
                ),
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("get");
        responder.join().expect("responder");
        let listed_json = serde_json::to_string(&listed).expect("listed");
        let fetched_json = serde_json::to_string(&fetched).expect("fetched");
        assert!(!listed_json.contains("FULL BODY SECRET"));
        assert!(!fetched_json.contains("FULL BODY SECRET"));
        assert!(fetched_json.contains("Preview only"));
        assert!(fetched.output["message"]["id"] == "msg1");
    }

    #[tokio::test]
    async fn create_calendar_event_posts_iana_timezone() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut create = server.recv().expect("create");
            assert_eq!(create.url(), "/graph/me/events");
            let mut body = String::new();
            create.as_reader().read_to_string(&mut body).expect("body");
            assert!(body.contains("Pacific/Auckland"));
            assert!(body.contains("2026-04-15T09:00:00"));
            create
                .respond(
                    TinyResponse::from_string(
                        r#"{"id":"evt1","subject":"Sync","webLink":"https://outlook.office.com/calendar/item/evt1"}"#,
                    )
                    .with_status_code(201)
                    .with_header(json_header()),
                )
                .expect("created");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &mail_request(
                    "microsoft.create_calendar_event",
                    BTreeMap::from([
                        ("subject".into(), Value::String("Sync".into())),
                        ("start".into(), Value::String("2026-04-15T09:00:00".into())),
                        ("end".into(), Value::String("2026-04-15T10:00:00".into())),
                        ("time_zone".into(), Value::String("Pacific/Auckland".into())),
                    ]),
                ),
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("create");
        responder.join().expect("responder");
        assert_eq!(result.output["eventId"], "evt1");
    }

    #[tokio::test]
    async fn new_mail_delta_establishes_a_checkpoint_then_emits_without_bodies() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let first = server.recv().expect("first delta");
            assert!(first.url().contains("/messages/delta"));
            first
                .respond(
                    TinyResponse::from_string(
                        r#"{"value":[{"id":"old","subject":"Old","from":{"emailAddress":{"address":"ada@example.com"}},"receivedDateTime":"2026-08-19T09:00:00Z","webLink":"https://outlook.office.com/mail/id/old","bodyPreview":"old preview"}],"@odata.deltaLink":"https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=token-1"}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("first");
            let second = server.recv().expect("second delta");
            assert!(
                second.url().contains("%24deltatoken=token-1")
                    || second.url().contains("$deltatoken=token-1")
            );
            second
                .respond(
                    TinyResponse::from_string(
                        r#"{"value":[{"id":"new1","subject":"Hello","from":{"emailAddress":{"name":"Ada","address":"ada@example.com"}},"receivedDateTime":"2026-08-19T10:00:00Z","webLink":"https://outlook.office.com/mail/id/new1","isRead":false,"bodyPreview":"preview"},{"id":"gone","@removed":{"reason":"deleted"}}],"@odata.deltaLink":"https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=token-2"}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("second");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "microsoft".into(),
            event_type: "microsoft.new_mail".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::new(),
            descriptor_version: 1,
        };
        let first = service
            .poll(
                &config,
                &connection(),
                None,
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("first poll");
        assert!(first.events.is_empty());
        let second = service
            .poll(
                &config,
                &connection(),
                first.cursor.as_deref(),
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("second poll");
        responder.join().expect("responder");
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].external_event_id, "new1");
        assert!(second.events[0].preview.is_none());
        let serialized = serde_json::to_string(&second.events).expect("events");
        assert!(!serialized.contains("old preview"));
        assert!(!serialized.contains("ms-access-fixture"));
    }

    #[test]
    fn malformed_mail_and_calendar_input_is_rejected() {
        let mut input = BTreeMap::from([("to".into(), Value::String("not-an-email".into()))]);
        assert!(required_recipients(&input, "to").is_err());
        input.insert("message_id".into(), Value::String("../evil".into()));
        assert!(required_graph_id(&input, "message_id").is_err());
        input.insert("subject".into(), Value::String("line\nfeed".into()));
        assert!(bounded_single_line(&input, "subject", 40).is_err());
    }
}
