//! Gmail connected-app provider (Plan 014, Phase 1: send-only).
//!
//! Authentication uses Google's native-app OAuth authorization-code flow with
//! S256 PKCE and a loopback callback on `127.0.0.1`. Only the public OAuth
//! client ID is compiled into Alfred; no web-client secret exists in the
//! desktop. The requested grant is identity basics plus the `gmail.send`
//! scope; no read scope is ever requested. The provider stays hidden until a
//! build-time client ID is present, which is the capability gate that keeps
//! the send-only phase behind Google verification (Plan 014 Step 4).

use super::actions::{
    ActionArtifact, ActionCancellation, ActionDescriptor, ActionError, ActionErrorCode,
    ActionExecutor, ActionFieldDescriptor, ActionFieldKind, ActionFuture, ActionLimits,
    ActionRegistry, ActionResult, TokenAccessCapability, ValidatedActionRequest,
};
use super::models::{
    canonical_identity_key, AppConnection, AppConnectionDto, IntegrationCommandError,
    UpsertAppConnection,
};
use super::oauth_native::{
    NativeOAuthAttempt, NativeOAuthConfig, NativeOAuthError,
};
use super::refresh::{ProviderRefreshError, RefreshFuture, RefreshHandler};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use base64::{engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD}, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::Zeroizing;

pub const GMAIL_SEND_SCOPE: &str = "https://www.googleapis.com/auth/gmail.send";

const GMAIL_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GMAIL_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const GMAIL_USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const GMAIL_API_BASE: &str = "https://gmail.googleapis.com";
const GMAIL_USER_AGENT: &str = "Alfred-Desktop";
const GMAIL_RESPONSE_LIMIT: usize = 512 * 1024;
const GMAIL_ERROR_HINT_LIMIT: usize = 16 * 1024;
const GMAIL_OAUTH_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_PAIRING_SESSIONS: usize = 8;
const MAX_RECIPIENTS: usize = 100;
const MAX_ADDRESS_CHARS: usize = 254;
const MAX_SUBJECT_CHARS: usize = 998;
const MAX_BODY_CHARS: usize = 64 * 1024;
const REQUESTED_SCOPES: [&str; 4] = ["openid", "email", "profile", GMAIL_SEND_SCOPE];

pub fn is_configured() -> bool {
    option_env!("ALFRED_GMAIL_CLIENT_ID").is_some_and(|value| !value.trim().is_empty())
}

pub fn register(
    actions: &ActionRegistry,
    service: Arc<GmailService>,
) -> Result<(), ActionError> {
    for descriptor in action_descriptors() {
        actions.register(descriptor, ActionLimits::default(), service.clone())?;
    }
    Ok(())
}

pub struct GmailService {
    client_id: Option<String>,
    auth_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    api_base: String,
    preferred_ports: Vec<u16>,
    sessions: Mutex<HashMap<String, GmailPairingSession>>,
}

impl Default for GmailService {
    fn default() -> Self {
        Self {
            client_id: option_env!("ALFRED_GMAIL_CLIENT_ID")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            auth_endpoint: GMAIL_AUTH_ENDPOINT.into(),
            token_endpoint: GMAIL_TOKEN_ENDPOINT.into(),
            userinfo_endpoint: GMAIL_USERINFO_ENDPOINT.into(),
            api_base: GMAIL_API_BASE.into(),
            preferred_ports: option_env!("ALFRED_GMAIL_OAUTH_PORT")
                .and_then(|value| value.trim().parse::<u16>().ok())
                .map(|port| vec![port])
                .unwrap_or_default(),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl GmailService {
    pub fn refresh_handler(&self) -> Arc<dyn RefreshHandler> {
        Arc::new(GmailRefreshHandler {
            client_id: self.client_id.clone(),
            token_endpoint: self.token_endpoint.clone(),
        })
    }

    pub fn prepare_authorization(
        &self,
    ) -> Result<GmailAuthorizationStarted, IntegrationCommandError> {
        self.remove_expired_sessions();
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            command_error(
                "gmail_not_configured",
                "This build does not include the public Gmail OAuth client ID.",
                false,
            )
        })?;
        if self
            .sessions
            .lock()
            .map_err(|_| gmail_state_error())?
            .len()
            >= MAX_PAIRING_SESSIONS
        {
            return Err(command_error(
                "gmail_pairing_busy",
                "Too many Gmail authorization attempts are active. Close another attempt and try again.",
                true,
            ));
        }
        let config = NativeOAuthConfig {
            authorization_endpoint: self.auth_endpoint.clone(),
            client_id: client_id.to_owned(),
            scopes: REQUESTED_SCOPES.iter().map(|scope| (*scope).to_owned()).collect(),
            extra_params: BTreeMap::from([
                ("access_type".into(), "offline".into()),
                ("prompt".into(), "consent".into()),
            ]),
            callback_path: "/oauth/callback".into(),
            timeout: GMAIL_OAUTH_TIMEOUT,
            include_nonce: false,
            preferred_ports: self.preferred_ports.clone(),
        };
        let attempt = NativeOAuthAttempt::start(config).map_err(map_native_oauth_error)?;
        let authorization_url = attempt.authorization_url().to_string();
        let expires_at =
            (Utc::now() + ChronoDuration::seconds(GMAIL_OAUTH_TIMEOUT.as_secs() as i64))
                .to_rfc3339();
        let session_id = Uuid::new_v4().to_string();
        self.sessions
            .lock()
            .map_err(|_| gmail_state_error())?
            .insert(
                session_id.clone(),
                GmailPairingSession {
                    attempt,
                    cancel: Arc::new(AtomicBool::new(false)),
                    expires_at: Instant::now() + GMAIL_OAUTH_TIMEOUT,
                },
            );
        Ok(GmailAuthorizationStarted {
            session_id,
            authorization_url,
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
            .map_err(|_| gmail_state_error())?
            .remove(session_id)
            .ok_or_else(gmail_pairing_expired)?;
        if Instant::now() >= session.expires_at {
            return Err(gmail_pairing_expired());
        }
        let cancel = session.cancel.clone();
        let verified = tauri::async_runtime::spawn_blocking(move || {
            session.attempt.wait_for_callback_cancellable(cancel)
        })
        .await
        .map_err(|_| gmail_state_error())?
        .map_err(map_native_oauth_error)?;
        self.exchange_and_save(db, store, verified).await
    }

    pub fn cancel_authorization(&self, session_id: &str) {
        if let Some(session) = self
            .sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(session_id).map(|session| session.cancel.clone()))
        {
            session.store(true, Ordering::SeqCst);
        }
    }

    fn remove_expired_sessions(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            let now = Instant::now();
            sessions.retain(|_, session| session.expires_at > now);
        }
    }

    async fn exchange_and_save(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        verified: super::oauth_native::VerifiedAuthorizationCode,
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            command_error(
                "gmail_not_configured",
                "This build does not include the public Gmail OAuth client ID.",
                false,
            )
        })?;
        let response: GmailTokenResponse = token_post_form(
            &self.token_endpoint,
            &[
                ("client_id", client_id),
                ("code", &verified.code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", &verified.context.redirect_uri),
                ("code_verifier", &verified.context.verifier),
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
            return Err(gmail_invalid_response());
        }
        let refresh_token = Zeroizing::new(response.refresh_token.unwrap_or_default());
        if refresh_token.trim().is_empty() {
            return Err(command_error(
                "gmail_offline_access_required",
                "Google did not grant offline access. Authorize again.",
                false,
            ));
        }
        let user: GmailUser = get_json(
            &self.userinfo_endpoint,
            access_token.as_str(),
            &[],
        )
        .await
        .map_err(map_connect_action_error)?;
        if user.sub.trim().is_empty()
            || user.sub.len() > 128
            || !valid_email_address(&user.email)
            || !user.email_verified
        {
            return Err(command_error(
                "gmail_account_invalid",
                "Google did not return a valid, verified Gmail account identity.",
                false,
            ));
        }
        let identity_key =
            canonical_identity_key("gmail", "native_oauth", &[&user.sub]);
        let existing = db
            .get_app_connection_by_identity("gmail", "native_oauth", &identity_key)
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

        let scopes = sanitize_granted_scopes(response.scope.as_deref());
        let provider_metadata = BTreeMap::from([("email".into(), user.email.clone())]);
        let saved = db.upsert_app_connection(UpsertAppConnection {
            provider_id: "gmail".into(),
            display_name: Some(user.email.clone()),
            external_account_id: Some(user.sub.clone()),
            external_tenant_id: None,
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

    fn client(&self) -> Client {
        gmail_client()
    }
}

struct GmailPairingSession {
    attempt: NativeOAuthAttempt,
    cancel: Arc<AtomicBool>,
    expires_at: Instant,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailAuthorizationStarted {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_at: String,
}

#[derive(Default, Deserialize)]
struct GmailTokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct GmailUser {
    sub: String,
    email: String,
    #[serde(default)]
    email_verified: bool,
}

#[derive(Deserialize)]
struct GmailSentMessage {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: String,
}

struct GmailRefreshHandler {
    client_id: Option<String>,
    token_endpoint: String,
}

impl RefreshHandler for GmailRefreshHandler {
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
                .ok_or_else(|| ProviderRefreshError::terminal("gmail_not_configured"))?;
            let refresh_token = Zeroizing::new(
                credential
                    .refresh_token
                    .take()
                    .ok_or_else(|| ProviderRefreshError::terminal("gmail_grant_revoked"))?,
            );
            let response: GmailTokenResponse = token_post_form(
                &self.token_endpoint,
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
                    ProviderRefreshError::terminal("gmail_grant_revoked")
                }
                _ => ProviderRefreshError::retryable("gmail_unavailable"),
            })?;
            let access_token = response
                .access_token
                .trim()
                .to_owned();
            if access_token.is_empty()
                || !response
                    .token_type
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
            {
                return Err(ProviderRefreshError::retryable("gmail_invalid_response"));
            }
            credential.access_token = access_token;
            credential.refresh_token = response.refresh_token;
            credential.expires_at = response
                .expires_in
                .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
            Ok(credential)
        })
    }
}

fn action_descriptors() -> Vec<ActionDescriptor> {
    vec![ActionDescriptor {
        provider_id: "gmail".into(),
        action_id: "gmail.send_email".into(),
        label: "Send Gmail message".into(),
        description: "Send a plain-text email from your connected Gmail account.".into(),
        fields: vec![
            text_field(
                "to",
                "To",
                "Comma-separated recipient addresses.",
                true,
                true,
            ),
            text_field(
                "cc",
                "Cc",
                "Optional comma-separated CC addresses.",
                false,
                true,
            ),
            text_field(
                "bcc",
                "Bcc",
                "Optional comma-separated BCC addresses.",
                false,
                true,
            ),
            text_field(
                "subject",
                "Subject",
                "Email subject line.",
                true,
                true,
            ),
            textarea_field("body", "Body", "Plain-text email body.", true),
        ],
        required_scopes: vec![GMAIL_SEND_SCOPE.into()],
        output_schema_version: 1,
        output_is_untrusted: false,
    }]
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

fn textarea_field(key: &str, label: &str, description: &str, required: bool) -> ActionFieldDescriptor {
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

impl ActionExecutor for GmailService {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        connection: &'a AppConnection,
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
            let from = connection
                .provider_metadata
                .get("email")
                .filter(|email| valid_email_address(email))
                .cloned()
                .ok_or_else(|| ActionError::new(ActionErrorCode::ConnectionRequired))?;
            let to = required_recipients(&request.input, "to")?;
            let cc = optional_recipients(&request.input, "cc")?;
            let bcc = optional_recipients(&request.input, "bcc")?;
            if to.len().saturating_add(cc.len()).saturating_add(bcc.len()) > MAX_RECIPIENTS {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
            let subject = bounded_single_line(&request.input, "subject", MAX_SUBJECT_CHARS)?;
            let body = bounded_text(&request.input, "body", MAX_BODY_CHARS)?;
            if body.trim().is_empty() {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
            let raw = URL_SAFE_NO_PAD.encode(build_raw_message(&from, &to, &cc, &bcc, &subject, &body));
            let (message, _): (GmailSentMessage, _) = self
                .post_json(
                    token.as_str(),
                    "/gmail/v1/users/me/messages/send",
                    &serde_json::json!({ "raw": raw }),
                )
                .await?;
            if !valid_gmail_id(&message.id) || !valid_gmail_id(&message.thread_id) {
                return Err(ActionError::new(ActionErrorCode::OutputInvalid));
            }
            let recipient_count = to.len() + cc.len() + bcc.len();
            Ok(ActionResult {
                summary: if recipient_count == 1 {
                    "Sent Gmail message".into()
                } else {
                    format!("Sent Gmail message to {recipient_count} recipients")
                },
                output: serde_json::json!({
                    "schemaVersion": 1,
                    "messageId": message.id,
                    "threadId": message.thread_id,
                }),
                artifacts: vec![ActionArtifact {
                    kind: "url".into(),
                    label: "Open message in Gmail".into(),
                    uri: format!("https://mail.google.com/mail/u/0/#all/{}", message.id),
                }],
                provider_request_id: None,
            })
        })
    }
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
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if recipients.len() > MAX_RECIPIENTS
        || recipients.iter().any(|recipient| !valid_email_address(recipient))
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
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    if value.trim().is_empty() {
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
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b'@'))
}

fn valid_gmail_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn build_raw_message(
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    subject: &str,
    body: &str,
) -> String {
    let mut message = String::new();
    message.push_str(&format!("From: {from}\r\n"));
    message.push_str(&format!("To: {}\r\n", to.join(", ")));
    if !cc.is_empty() {
        message.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    if !bcc.is_empty() {
        message.push_str(&format!("Bcc: {}\r\n", bcc.join(", ")));
    }
    message.push_str(&format!("Subject: {}\r\n", encode_subject(subject)));
    message.push_str("MIME-Version: 1.0\r\n");
    message.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
    message.push_str("Content-Transfer-Encoding: base64\r\n");
    message.push_str("\r\n");
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let encoded = BASE64_STANDARD.encode(normalized.as_bytes());
    for chunk in encoded.as_bytes().chunks(76) {
        message.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        message.push_str("\r\n");
    }
    message
}

fn encode_subject(subject: &str) -> String {
    if subject
        .bytes()
        .all(|byte| (32..=126).contains(&byte) && byte != b'\r' && byte != b'\n')
    {
        return subject.to_owned();
    }
    let bytes = subject.as_bytes();
    let mut encoded_words = Vec::new();
    for chunk in bytes.chunks(45) {
        let encoded = BASE64_STANDARD.encode(chunk);
        encoded_words.push(format!("=?UTF-8?B?{encoded}?="));
    }
    encoded_words.join("\r\n ")
}

fn sanitize_granted_scopes(granted: Option<&str>) -> Vec<String> {
    let Some(granted) = granted else {
        return REQUESTED_SCOPES.iter().map(|scope| (*scope).to_owned()).collect();
    };
    let mut scopes = granted
        .split_whitespace()
        .filter(|scope| REQUESTED_SCOPES.contains(scope))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() {
        REQUESTED_SCOPES.iter().map(|scope| (*scope).to_owned()).collect()
    } else {
        scopes
    }
}

impl GmailService {
    async fn post_json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        body: &Value,
    ) -> Result<(T, Option<String>), ActionError> {
        let request = gmail_request(
            self.client(),
            token,
            Method::POST,
            &format!("{}{}", self.api_base, path),
        )
        .json(body);
        send_gmail_json(request, true)
            .await
            .map(|response| (response.value, response.request_id))
    }
}

async fn token_post_form<T: DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
) -> Result<T, ActionError> {
    let request = gmail_client()
        .post(url)
        .header("Accept", "application/json")
        .header("User-Agent", GMAIL_USER_AGENT)
        .form(form);
    send_gmail_json(request, false)
        .await
        .map(|response| response.value)
}

async fn get_json<T: DeserializeOwned>(
    url: &str,
    token: &str,
    query: &[(&str, &str)],
) -> Result<T, ActionError> {
    let request = gmail_client()
        .get(url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", GMAIL_USER_AGENT)
        .query(query);
    send_gmail_json(request, false)
        .await
        .map(|response| response.value)
}

fn gmail_request(client: Client, token: &str, method: Method, url: &str) -> RequestBuilder {
    client
        .request(method, url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", GMAIL_USER_AGENT)
}

fn gmail_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("Gmail HTTP client")
}

async fn send_gmail_json<T: DeserializeOwned>(
    request: RequestBuilder,
    mutation: bool,
) -> Result<GmailJsonResponse<T>, ActionError> {
    let response = request.send().await.map_err(|error| {
        if mutation && (error.is_timeout() || !error.is_connect()) {
            ActionError::new(ActionErrorCode::DeliveryUnknown)
        } else {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
    })?;
    parse_gmail_response(response, mutation).await
}

async fn parse_gmail_response<T: DeserializeOwned>(
    response: Response,
    mutation: bool,
) -> Result<GmailJsonResponse<T>, ActionError> {
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let retry_after = gmail_retry_after(&response);
    if !status.is_success() {
        let code = match status {
            StatusCode::UNAUTHORIZED => ActionErrorCode::ProviderUnauthorized,
            StatusCode::TOO_MANY_REQUESTS => ActionErrorCode::RateLimited,
            StatusCode::BAD_REQUEST => ActionErrorCode::InvalidInput,
            StatusCode::FORBIDDEN => {
                match gmail_error_reason(response).await.as_deref() {
                    Some("quotaExceeded")
                    | Some("rateLimitExceeded")
                    | Some("userRateLimitExceeded")
                    | Some("dailyLimitExceeded") => ActionErrorCode::RateLimited,
                    _ => ActionErrorCode::ScopeMissing,
                }
            }
            StatusCode::NOT_FOUND => ActionErrorCode::InvalidInput,
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
        .is_some_and(|length| length as usize > GMAIL_RESPONSE_LIMIT)
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
        if bytes.len().saturating_add(chunk.len()) > GMAIL_RESPONSE_LIMIT {
            return Err(ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::OutputTooLarge
            }));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| {
        ActionError::new(if mutation {
            ActionErrorCode::DeliveryUnknown
        } else {
            ActionErrorCode::OutputInvalid
        })
    })?;
    Ok(GmailJsonResponse { value, request_id })
}

async fn gmail_error_reason(response: Response) -> Option<String> {
    if response
        .content_length()
        .is_some_and(|length| length as usize > GMAIL_ERROR_HINT_LIMIT)
    {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return None;
        };
        if bytes.len().saturating_add(chunk.len()) > GMAIL_ERROR_HINT_LIMIT {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let reason = value
        .get("error")?
        .get("errors")?
        .as_array()?
        .first()?
        .get("reason")?
        .as_str()?;
    (reason.len() <= 64).then(|| reason.to_owned())
}

struct GmailJsonResponse<T> {
    value: T,
    request_id: Option<String>,
}

fn gmail_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.min(86_400))
}

fn map_native_oauth_error(error: NativeOAuthError) -> IntegrationCommandError {
    match error {
        NativeOAuthError::Cancelled => command_error(
            "gmail_pairing_cancelled",
            "The Gmail authorization attempt was cancelled.",
            false,
        ),
        NativeOAuthError::Timeout => gmail_pairing_expired(),
        NativeOAuthError::AuthorizationDenied => command_error(
            "gmail_authorization_denied",
            "Google authorization was not completed.",
            false,
        ),
        NativeOAuthError::CallbackUnavailable => command_error(
            "gmail_unavailable",
            "The local authorization callback could not be opened.",
            true,
        ),
        _ => command_error(
            "gmail_invalid_response",
            "Google returned an invalid authorization callback.",
            false,
        ),
    }
}

fn map_token_exchange_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized
        | ActionErrorCode::ScopeMissing
        | ActionErrorCode::InvalidInput => command_error(
            "gmail_authorization_expired",
            "Google rejected the authorization code. Start a new connection attempt.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Google is rate limiting authorization attempts. Try again later.",
            true,
        ),
        _ => command_error(
            "gmail_unavailable",
            "Google authorization is temporarily unavailable.",
            true,
        ),
    }
}

fn map_connect_action_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized => command_error(
            "gmail_authorization_expired",
            "Google rejected the authorization. Start a new connection attempt.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Google is rate limiting requests. Try again later.",
            true,
        ),
        _ => command_error(
            "gmail_unavailable",
            "Google could not validate this connection. Try again.",
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

fn gmail_state_error() -> IntegrationCommandError {
    command_error(
        "gmail_pairing_failed",
        "The Gmail authorization attempt could not be updated.",
        true,
    )
}

fn gmail_pairing_expired() -> IntegrationCommandError {
    command_error(
        "gmail_pairing_expired",
        "This Gmail authorization attempt expired. Start again.",
        false,
    )
}

fn gmail_invalid_response() -> IntegrationCommandError {
    command_error(
        "gmail_invalid_response",
        "Google returned an invalid authorization response.",
        true,
    )
}

fn connection_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "Gmail was authorized, but the connection metadata could not be saved.",
        true,
    )
}

fn credential_write_error() -> IntegrationCommandError {
    command_error(
        "gmail_connection_failed",
        "The Gmail credential could not be saved securely.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    use tiny_http::{Header, Response as TinyResponse, Server};

    fn test_service(base: String) -> GmailService {
        GmailService {
            client_id: Some("public-client-id.apps.googleusercontent.com".into()),
            auth_endpoint: GMAIL_AUTH_ENDPOINT.into(),
            token_endpoint: format!("{base}/token"),
            userinfo_endpoint: format!("{base}/userinfo"),
            api_base: base,
            preferred_ports: vec![],
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn connection() -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "gmail".into(),
            display_name: Some("user@example.com".into()),
            external_account_id: Some("sub-fixture".into()),
            external_tenant_id: None,
            connection_mode: "native_oauth".into(),
            identity_key: "identity".into(),
            scopes: vec![GMAIL_SEND_SCOPE.into()],
            provider_metadata: BTreeMap::from([("email".into(), "user@example.com".into())]),
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
                &CredentialEnvelope::new("ya29.access-fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    fn json_header() -> Header {
        Header::from_bytes("Content-Type", "application/json").expect("header")
    }

    fn send_callback(authorization_url: &str, code: &str) {
        let url = url::Url::parse(authorization_url).expect("authorization url");
        let redirect: url::Url = url
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

    #[test]
    fn descriptors_are_secret_free_send_only_and_request_no_read_scope() {
        let descriptors = action_descriptors();
        assert_eq!(descriptors.len(), 1);
        let descriptor = &descriptors[0];
        assert_eq!(descriptor.action_id, "gmail.send_email");
        assert_eq!(descriptor.required_scopes, vec![GMAIL_SEND_SCOPE]);
        assert!(descriptor
            .fields
            .iter()
            .all(|field| !field.secret && field.key != "attachment"));
        assert!(descriptors
            .iter()
            .flat_map(|descriptor| descriptor.required_scopes.iter())
            .all(|scope| !scope.contains("read")));
    }

    #[test]
    fn mime_builder_encodes_unicode_and_guards_headers() {
        let raw = build_raw_message(
            "user@example.com",
            &["other@example.com".into()],
            &["cc@example.com".into()],
            &["hidden@example.com".into()],
            "Café ☕",
            "Hello\r\nline2",
        );
        assert!(raw.starts_with("From: user@example.com\r\n"));
        assert!(raw.contains("To: other@example.com\r\n"));
        assert!(raw.contains("Cc: cc@example.com\r\n"));
        assert!(raw.contains("Bcc: hidden@example.com\r\n"));
        assert!(raw.contains("Subject: =?UTF-8?B?Q2Fmw6kg4piV?=\r\n"));
        assert!(raw.contains("Content-Type: text/plain; charset=UTF-8"));
        assert!(raw.contains("Content-Transfer-Encoding: base64"));
        let payload = raw.split("\r\n\r\n").nth(1).expect("body");
        let decoded = BASE64_STANDARD
            .decode(payload.replace("\r\n", ""))
            .expect("decode");
        assert_eq!(String::from_utf8_lossy(&decoded), "Hello\nline2");

        let ascii = encode_subject("Plain subject");
        assert_eq!(ascii, "Plain subject");
        assert!(!ascii.contains("=?UTF-8?"));
    }

    #[test]
    fn recipient_validation_rejects_injection_and_oversize_lists() {
        assert!(valid_email_address("user@example.com"));
        assert!(valid_email_address("user+tag@sub.example.co"));
        assert!(!valid_email_address(""));
        assert!(!valid_email_address("user@"));
        assert!(!valid_email_address("@example.com"));
        assert!(!valid_email_address("no-at-sign"));
        assert!(!valid_email_address("user@nodot"));
        assert!(!valid_email_address("a\r\nBcc: evil@example.com@example.com"));
        assert!(!valid_email_address(&"a".repeat(255)));
        let mut input = BTreeMap::new();
        input.insert(
            "to".into(),
            Value::String("a\r\nBcc: evil@example.com".into()),
        );
        assert!(required_recipients(&input, "to").is_err());
        let many = (0..MAX_RECIPIENTS + 1)
            .map(|index| format!("user{index}@example.com"))
            .collect::<Vec<_>>()
            .join(",");
        input.insert("to".into(), Value::String(many));
        assert!(required_recipients(&input, "to").is_err());
        input.insert("to".into(), Value::String(String::new()));
        assert!(required_recipients(&input, "to").is_err());
    }

    #[test]
    fn subject_and_body_limits_reject_oversize_and_multiline_subjects() {
        let mut input = BTreeMap::new();
        input.insert("subject".into(), Value::String("line\r\ninjected".into()));
        assert!(bounded_single_line(&input, "subject", MAX_SUBJECT_CHARS).is_err());
        input.insert(
            "subject".into(),
            Value::String("x".repeat(MAX_SUBJECT_CHARS + 1)),
        );
        assert!(bounded_single_line(&input, "subject", MAX_SUBJECT_CHARS).is_err());
        input.insert(
            "body".into(),
            Value::String("x".repeat(MAX_BODY_CHARS + 1)),
        );
        assert!(bounded_text(&input, "body", MAX_BODY_CHARS).is_err());
    }

    #[test]
    fn granted_scopes_are_whitelisted_to_the_requested_union() {
        assert_eq!(
            sanitize_granted_scopes(Some(&format!(
                "openid email profile {} https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/calendar",
                GMAIL_SEND_SCOPE
            ))),
            vec!["email", GMAIL_SEND_SCOPE, "openid", "profile"]
        );
    }

    #[tokio::test]
    async fn complete_authorization_exchanges_pkce_and_saves_the_verified_account() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut token_request = server.recv().expect("token request");
            assert_eq!(token_request.url(), "/token");
            assert_eq!(token_request.method().as_str(), "POST");
            let mut body = String::new();
            token_request.as_reader().read_to_string(&mut body).expect("body");
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("code=authorization-code"));
            assert!(body.contains("code_verifier="));
            assert!(body.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"));
            assert!(!body.contains("client_secret"));
            token_request
                .respond(
                    TinyResponse::from_string(
                        r#"{"access_token":"ya29.access-fixture","token_type":"Bearer","expires_in":3599,"refresh_token":"refresh-fixture","scope":"openid email https://www.googleapis.com/auth/gmail.send"}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("token response");
            let userinfo = server.recv().expect("userinfo request");
            assert_eq!(userinfo.url(), "/userinfo");
            userinfo
                .respond(
                    TinyResponse::from_string(
                        r#"{"sub":"1234567890","email":"user@example.com","email_verified":true}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("userinfo response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let prepared = service.prepare_authorization().expect("prepare");
        let url = url::Url::parse(&prepared.authorization_url).expect("authorization url");
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        assert_eq!(url.scheme(), "https");
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "code_challenge_method" && value == "S256"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "access_type" && value == "offline"));
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
        assert_eq!(result.display_name.as_deref(), Some("user@example.com"));
        assert!(result.scopes.contains(&GMAIL_SEND_SCOPE.into()));
        assert!(!result.scopes.iter().any(|scope| scope.contains("read")));
        let saved = db
            .list_app_connections()
            .expect("connections")
            .pop()
            .expect("saved connection");
        let credential = store.get(&saved.credential_ref).expect("credential");
        assert_eq!(credential.access_token, "ya29.access-fixture");
        assert_eq!(credential.refresh_token.as_deref(), Some("refresh-fixture"));
        let serialized = serde_json::to_string(&result).expect("DTO");
        assert!(!serialized.contains("ya29.access-fixture"));
        assert!(!serialized.contains("refresh-fixture"));
        assert!(!serialized.contains("authorization-code"));
    }

    #[tokio::test]
    async fn complete_authorization_rejects_unverified_accounts_and_missing_offline_access() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let token_request = server.recv().expect("token request");
            assert_eq!(token_request.url(), "/token");
            token_request
                .respond(
                    TinyResponse::from_string(
                        r#"{"access_token":"ya29.access-fixture","token_type":"Bearer","expires_in":3599}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("missing refresh response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());

        let prepared = service.prepare_authorization().expect("prepare");
        let session_id = prepared.session_id.clone();
        let callback_url = prepared.authorization_url.clone();
        let waiter = std::thread::spawn(move || send_callback(&callback_url, "authorization-code"));
        let error = service
            .complete_authorization(&db, store.clone(), &session_id)
            .await
            .expect_err("offline access required");
        waiter.join().expect("callback thread");
        assert_eq!(error.code, "gmail_offline_access_required");

        let server_two = Server::http(("127.0.0.1", 0)).expect("server");
        let port_two = server_two.server_addr().to_ip().expect("address").port();
        let responder_two = std::thread::spawn(move || {
            let token_request = server_two.recv().expect("token request");
            token_request
                .respond(
                    TinyResponse::from_string(
                        r#"{"access_token":"ya29.access-fixture","token_type":"Bearer","expires_in":3599,"refresh_token":"refresh-fixture","scope":"openid email https://www.googleapis.com/auth/gmail.send"}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("token response");
            let userinfo = server_two.recv().expect("userinfo request");
            userinfo
                .respond(
                    TinyResponse::from_string(
                        r#"{"sub":"1234567890","email":"user@example.com","email_verified":false}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("unverified userinfo response");
        });
        let service = test_service(format!("http://127.0.0.1:{port_two}"));
        let prepared = service.prepare_authorization().expect("prepare");
        let session_id = prepared.session_id.clone();
        let callback_url = prepared.authorization_url.clone();
        let waiter = std::thread::spawn(move || send_callback(&callback_url, "authorization-code"));
        let error = service
            .complete_authorization(&db, store, &session_id)
            .await
            .expect_err("unverified account");
        waiter.join().expect("callback thread");
        responder_two.join().expect("responder two");
        responder.join().expect("responder one");
        assert_eq!(error.code, "gmail_account_invalid");
    }

    #[tokio::test]
    async fn cancelling_an_attempt_stops_the_blocking_wait() {
        let _gate = super::super::oauth_native::LOOPBACK_TEST_GATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let service = test_service("http://127.0.0.1:0".into());
        let prepared = service.prepare_authorization().expect("prepare");
        let session_id = prepared.session_id.clone();
        service.cancel_authorization(&session_id);
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let error = service
            .complete_authorization(&db, store, &session_id)
            .await
            .expect_err("cancelled");
        assert_eq!(error.code, "gmail_pairing_cancelled");
    }

    #[tokio::test]
    async fn send_email_posts_base64url_mime_and_maps_ambiguous_failures() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut send = server.recv().expect("send request");
            assert_eq!(send.url(), "/gmail/v1/users/me/messages/send");
            assert_eq!(send.method().as_str(), "POST");
            let mut body = String::new();
            send.as_reader().read_to_string(&mut body).expect("body");
            assert!(!body.contains("ya29.access-fixture"));
            let value: Value = serde_json::from_str(&body).expect("json");
            let raw = value.get("raw").and_then(Value::as_str).expect("raw");
            let decoded = URL_SAFE_NO_PAD.decode(raw).expect("decode");
            let message = String::from_utf8(decoded).expect("utf8");
            assert!(message.starts_with("From: user@example.com\r\n"));
            assert!(message.contains("To: other@example.com"));
            assert!(message.contains("Subject: Release notes"));
            send.respond(
                TinyResponse::from_string(
                    r#"{"id":"18a1b2c3d4e5","threadId":"18a1b2c3d4e5","labelIds":["SENT"]}"#,
                )
                .with_header(json_header()),
            )
            .expect("send response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "gmail".into(),
                    action_id: "gmail.send_email".into(),
                    input: BTreeMap::from([
                        ("to".into(), Value::String("other@example.com".into())),
                        ("subject".into(), Value::String("Release notes".into())),
                        ("body".into(), Value::String("Hello world".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action");
        responder.join().expect("responder");
        assert_eq!(result.output["messageId"], "18a1b2c3d4e5");
        let serialized = serde_json::to_string(&result).expect("result");
        assert!(!serialized.contains("ya29.access-fixture"));
        assert!(!serialized.contains("Hello world"));

        let server_two = Server::http(("127.0.0.1", 0)).expect("server");
        let port_two = server_two.server_addr().to_ip().expect("address").port();
        let responder_two = std::thread::spawn(move || {
            let send = server_two.recv().expect("send request");
            send.respond(TinyResponse::empty(502))
                .expect("gateway failure");
        });
        let service = test_service(format!("http://127.0.0.1:{port_two}"));
        let error = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "gmail".into(),
                    action_id: "gmail.send_email".into(),
                    input: BTreeMap::from([
                        ("to".into(), Value::String("other@example.com".into())),
                        ("subject".into(), Value::String("Release notes".into())),
                        ("body".into(), Value::String("Hello world".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect_err("ambiguous failure");
        responder_two.join().expect("responder two");
        assert_eq!(error.code, ActionErrorCode::DeliveryUnknown);
    }

    #[tokio::test]
    async fn send_error_mapping_distinguishes_scope_quota_and_unauthorized() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let unauthorized = server.recv().expect("unauthorized");
            unauthorized
                .respond(TinyResponse::empty(401))
                .expect("401");
            let scope = server.recv().expect("scope");
            scope
                .respond(
                    TinyResponse::from_string(
                        r#"{"error":{"code":403,"message":"Insufficient Permission","errors":[{"message":"Insufficient Permission","domain":"global","reason":"insufficientPermissions"}]}}"#,
                    )
                    .with_status_code(403)
                    .with_header(json_header()),
                )
                .expect("403 scope");
            let quota = server.recv().expect("quota");
            quota
                .respond(
                    TinyResponse::from_string(
                        r#"{"error":{"code":429,"message":"Rate Limit Exceeded","errors":[{"message":"Rate Limit Exceeded","domain":"global","reason":"rateLimitExceeded"}]}}"#,
                    )
                    .with_status_code(403)
                    .with_header(json_header()),
                )
                .expect("403 quota");
            let limited = server.recv().expect("limited");
            limited
                .respond(
                    TinyResponse::empty(429).with_header(
                        Header::from_bytes("Retry-After", "9").expect("retry header"),
                    ),
                )
                .expect("429");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let inputs = BTreeMap::from([
            ("to".into(), Value::String("other@example.com".into())),
            ("subject".into(), Value::String("Release notes".into())),
            ("body".into(), Value::String("Hello world".into())),
        ]);
        for (expected, expected_retry) in [
            (ActionErrorCode::ProviderUnauthorized, None),
            (ActionErrorCode::ScopeMissing, None),
            (ActionErrorCode::RateLimited, None),
            (ActionErrorCode::RateLimited, Some(9)),
        ] {
            let error = service
                .execute(
                    &ValidatedActionRequest {
                        connection_id: "connection".into(),
                        provider_id: "gmail".into(),
                        action_id: "gmail.send_email".into(),
                        input: inputs.clone(),
                    },
                    &connection(),
                    token_capability().await,
                    ActionCancellation::never(),
                )
                .await
                .expect_err("provider error");
            assert_eq!(error.code, expected);
            assert_eq!(error.retry_after_seconds, expected_retry);
        }
        responder.join().expect("responder");
    }

    #[tokio::test]
    async fn refresh_rotates_tokens_and_terminates_on_revoked_grants() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            for step in 0..3 {
                let mut refresh = server.recv().expect("refresh request");
                assert_eq!(refresh.url(), "/token");
                let mut body = String::new();
                refresh.as_reader().read_to_string(&mut body).expect("body");
                assert!(body.contains("grant_type=refresh_token"));
                let expected_refresh_token = if step == 0 {
                    "refresh-fixture"
                } else {
                    "refresh-rotated-fixture"
                };
                assert!(body.contains(&format!("refresh_token={expected_refresh_token}")));
                assert!(!body.contains("client_secret"));
                match step {
                    0 | 1 => {
                        refresh
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"access_token":"ya29.rotated-fixture","token_type":"Bearer","expires_in":3599,"refresh_token":"refresh-rotated-fixture"}"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("rotated response");
                    }
                    _ => {
                        refresh
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
                                )
                                .with_status_code(400)
                                .with_header(json_header()),
                            )
                            .expect("revoked response");
                    }
                }
            }
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let handler = service.refresh_handler();
        let mut credential = CredentialEnvelope::new("ya29.stale-fixture".into());
        credential.refresh_token = Some("refresh-fixture".into());
        let refreshed = handler
            .refresh(&connection(), credential)
            .await
            .expect("refresh");
        assert_eq!(refreshed.access_token, "ya29.rotated-fixture");
        assert_eq!(
            refreshed.refresh_token.as_deref(),
            Some("refresh-rotated-fixture")
        );

        let rotated = handler
            .refresh(&connection(), refreshed)
            .await
            .expect("second refresh");
        assert_eq!(rotated.access_token, "ya29.rotated-fixture");

        let error = handler
            .refresh(&connection(), rotated)
            .await
            .expect_err("revoked grant");
        responder.join().expect("responder");
        assert_eq!(error.code(), "gmail_grant_revoked");
    }

    #[test]
    fn every_command_dto_and_descriptor_stays_secret_free() {
        let started = GmailAuthorizationStarted {
            session_id: "session".into(),
            authorization_url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=public-client-id.apps.googleusercontent.com".into(),
            expires_at: "now".into(),
        };
        let serialized = serde_json::to_string(&started).expect("DTO");
        assert!(!serialized.contains("code_verifier"));
        assert!(!serialized.contains("client_secret"));
        assert!(serialized.contains("accounts.google.com"));
        let descriptors = action_descriptors();
        for field in descriptors.iter().flat_map(|descriptor| descriptor.fields.iter()) {
            assert!(!field.secret);
        }
        assert!(descriptors.iter().all(|descriptor| {
            descriptor
                .fields
                .iter()
                .all(|field| field.default.is_none() || !serde_json::to_string(&field.default).unwrap().contains("token"))
        }));
    }
}
