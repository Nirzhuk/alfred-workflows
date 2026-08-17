//! Telegram personal-notification connector.
//!
//! A user-owned BotFather token is held only for a short pairing session and
//! then in the OS credential store. Pairing consumes a bounded set of updates;
//! runtime operation is outbound-only and never starts a polling loop.

use super::actions::{
    ActionArtifact, ActionCancellation, ActionDescriptor, ActionError, ActionErrorCode,
    ActionExecutor, ActionFieldDescriptor, ActionFieldKind, ActionFuture, ActionLimits,
    ActionRegistry, ActionResult, TokenAccessCapability, ValidatedActionRequest,
};
use super::models::{
    canonical_identity_key, AppConnection, AppConnectionDto, IntegrationCommandError,
    UpsertAppConnection,
};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use rand::RngCore;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const TELEGRAM_CHAT_ID_FIELD: &str = "chat_id";
const TELEGRAM_MESSAGE_MAX_CHARS: usize = 4_096;
const TELEGRAM_TOKEN_MAX_BYTES: usize = 256;
const TELEGRAM_RESPONSE_LIMIT: usize = 256 * 1024;
const TELEGRAM_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const TELEGRAM_PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
const TELEGRAM_PAIRING_REQUESTS: usize = 3;
const TELEGRAM_PAIRING_LONG_POLL_SECONDS: u8 = 2;
const TELEGRAM_MINUTE_LIMIT: usize = 5;
const TELEGRAM_HOUR_LIMIT: usize = 60;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPrepareInput {
    pub bot_token: String,
}

impl Drop for TelegramPrepareInput {
    fn drop(&mut self) {
        self.bot_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramCompleteInput {
    pub pairing_session_id: String,
    pub test_message: String,
}

impl Drop for TelegramCompleteInput {
    fn drop(&mut self) {
        self.test_message.zeroize();
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPairingPrepared {
    pub pairing_session_id: String,
    pub bot_username: String,
    pub pairing_url: String,
    pub expires_at: String,
}

#[derive(Clone)]
struct PairedChat {
    id: i64,
    mask: String,
}

struct PairingSession {
    token: String,
    bot_id: i64,
    bot_username: String,
    bot_display_name: String,
    nonce: String,
    expires_at: Instant,
    cancelled: AtomicBool,
    paired_chat: Mutex<Option<PairedChat>>,
}

impl Drop for PairingSession {
    fn drop(&mut self) {
        self.token.zeroize();
        self.nonce.zeroize();
    }
}

type PairingSessions = Arc<Mutex<HashMap<String, Arc<PairingSession>>>>;

pub struct TelegramService {
    api_base: String,
    sessions: PairingSessions,
    connect_lock: tokio::sync::Mutex<()>,
    send_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    send_history: Mutex<HashMap<String, SendWindow>>,
}

impl Default for TelegramService {
    fn default() -> Self {
        Self {
            api_base: TELEGRAM_API_BASE.into(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            connect_lock: tokio::sync::Mutex::new(()),
            send_locks: Mutex::new(HashMap::new()),
            send_history: Mutex::new(HashMap::new()),
        }
    }
}

pub fn register(
    actions: &ActionRegistry,
    service: Arc<TelegramService>,
) -> Result<(), ActionError> {
    actions.register(
        send_personal_message_descriptor(),
        ActionLimits::default(),
        service,
    )
}

fn send_personal_message_descriptor() -> ActionDescriptor {
    ActionDescriptor {
        provider_id: "telegram".into(),
        action_id: "telegram.send_personal_message".into(),
        label: "Send Telegram notification".into(),
        description: "Send a plain-text notification to the paired private chat.".into(),
        fields: vec![ActionFieldDescriptor {
            key: "message".into(),
            label: "Message".into(),
            description: "Plain text, up to 4,096 characters.".into(),
            kind: ActionFieldKind::Textarea,
            required: true,
            default: None,
            secret: false,
            option_source: None,
            options: Vec::new(),
            supports_interpolation: true,
        }],
        required_scopes: Vec::new(),
        output_schema_version: 1,
        output_is_untrusted: false,
    }
}

impl TelegramService {
    pub async fn prepare(
        &self,
        db: &Db,
        mut input: TelegramPrepareInput,
    ) -> Result<TelegramPairingPrepared, IntegrationCommandError> {
        self.remove_expired_sessions();
        ensure_no_telegram_connection(db)?;

        let provided = Zeroizing::new(std::mem::take(&mut input.bot_token));
        let token = Zeroizing::new(provided.trim().to_owned());
        validate_bot_token(token.as_str())?;

        let bot: TelegramUser = self
            .call(
                "getMe",
                token.as_str(),
                &serde_json::json!({}),
                CallPhase::Lookup,
            )
            .await
            .map_err(connect_api_error)?;
        if !bot.is_bot {
            return Err(command_error(
                "telegram_identity_invalid",
                "Telegram did not identify this token as a bot.",
            ));
        }
        let username = bot
            .username
            .filter(|value| valid_bot_username(value))
            .ok_or_else(|| {
                command_error(
                    "telegram_identity_invalid",
                    "Telegram did not return a valid bot username.",
                )
            })?;
        let display_name = bounded_display_name(&bot.first_name).ok_or_else(|| {
            command_error(
                "telegram_identity_invalid",
                "Telegram did not return a valid bot identity.",
            )
        })?;
        let webhook: TelegramWebhookInfo = self
            .call(
                "getWebhookInfo",
                token.as_str(),
                &serde_json::json!({}),
                CallPhase::Lookup,
            )
            .await
            .map_err(connect_api_error)?;
        if !webhook.url.is_empty() {
            return Err(command_error(
                "telegram_webhook_conflict",
                "This bot already has a webhook. Create a fresh BotFather bot dedicated to Alfred.",
            ));
        }

        let session_id = Uuid::new_v4().to_string();
        let mut nonce_bytes = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        nonce_bytes.zeroize();
        let expires_at =
            Utc::now() + ChronoDuration::seconds(TELEGRAM_PAIRING_TTL.as_secs() as i64);
        let session = Arc::new(PairingSession {
            token: token.to_string(),
            bot_id: bot.id,
            bot_username: username.clone(),
            bot_display_name: display_name,
            nonce: nonce.clone(),
            expires_at: Instant::now() + TELEGRAM_PAIRING_TTL,
            cancelled: AtomicBool::new(false),
            paired_chat: Mutex::new(None),
        });
        self.sessions
            .lock()
            .map_err(|_| pairing_state_error())?
            .insert(session_id.clone(), session.clone());
        expire_session_later(self.sessions.clone(), session_id.clone(), session);

        Ok(TelegramPairingPrepared {
            pairing_session_id: session_id,
            bot_username: username.clone(),
            pairing_url: format!("https://t.me/{username}?start={nonce}"),
            expires_at: expires_at.to_rfc3339(),
        })
    }

    pub async fn complete(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        mut input: TelegramCompleteInput,
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let _guard = self.connect_lock.lock().await;
        self.remove_expired_sessions();
        ensure_no_telegram_connection(db)?;
        let session = self
            .session(&input.pairing_session_id)
            .ok_or_else(pairing_expired_error)?;
        if session.cancelled.load(Ordering::SeqCst) || Instant::now() >= session.expires_at {
            self.cancel(&input.pairing_session_id);
            return Err(pairing_expired_error());
        }
        let message = Zeroizing::new(std::mem::take(&mut input.test_message));
        validate_test_message(message.as_str())?;

        let paired = session
            .paired_chat
            .lock()
            .map_err(|_| pairing_state_error())?
            .clone();
        let paired = match paired {
            Some(chat) => chat,
            None => {
                let chat = match self.resolve_pairing(&session).await {
                    Ok(chat) => chat,
                    Err(error) => {
                        if matches!(
                            error.code.as_str(),
                            "telegram_pairing_ambiguous" | "telegram_private_chat_required"
                        ) {
                            self.cancel(&input.pairing_session_id);
                        }
                        return Err(error);
                    }
                };
                *session
                    .paired_chat
                    .lock()
                    .map_err(|_| pairing_state_error())? = Some(chat.clone());
                chat
            }
        };
        if !session_is_active(&session) {
            self.cancel(&input.pairing_session_id);
            return Err(pairing_expired_error());
        }

        match self
            .send_message(&session.token, paired.id, message.as_str())
            .await
        {
            Ok(_) if session_is_active(&session) => {}
            Ok(_) => {
                self.cancel(&input.pairing_session_id);
                return Err(pairing_expired_error());
            }
            Err(error) if error.kind == TelegramApiErrorKind::Ambiguous => {
                self.cancel(&input.pairing_session_id);
                return Err(command_error(
                    "telegram_test_delivery_unknown",
                    "Telegram may have accepted the test, but Alfred could not confirm it. Start pairing again before sending another test.",
                ));
            }
            Err(error) => return Err(test_send_error(error)),
        }

        let credential_ref = Uuid::new_v4().to_string();
        let mut credential = CredentialEnvelope::new(session.token.clone());
        credential
            .provider_fields
            .insert(TELEGRAM_CHAT_ID_FIELD.into(), paired.id.to_string());
        let saved_ref = credential_ref.clone();
        let saved_store = store.clone();
        tauri::async_runtime::spawn_blocking(move || saved_store.put(&saved_ref, &credential))
            .await
            .map_err(|_| credential_write_error())?
            .map_err(map_token_store_connect_error)?;
        if !session_is_active(&session) {
            let cleanup_store = store;
            let _ =
                tauri::async_runtime::spawn_blocking(move || cleanup_store.delete(&credential_ref))
                    .await;
            self.cancel(&input.pairing_session_id);
            return Err(pairing_expired_error());
        }

        let identity = canonical_identity_key(
            "telegram",
            "private_bot",
            &[&session.bot_id.to_string(), &paired.id.to_string()],
        );
        let metadata = BTreeMap::from([
            ("bot_id".into(), session.bot_id.to_string()),
            ("chat_type".into(), "private".into()),
            ("pairing_mode".into(), "start_nonce".into()),
            ("recipient_mask".into(), paired.mask.clone()),
        ]);
        let saved = db.upsert_app_connection(UpsertAppConnection {
            provider_id: "telegram".into(),
            display_name: Some(format!(
                "{} (@{}) → {}",
                session.bot_display_name, session.bot_username, paired.mask
            )),
            external_account_id: Some(format!("@{}", session.bot_username)),
            external_tenant_id: None,
            connection_mode: "private_bot".into(),
            identity_key: identity,
            scopes: Vec::new(),
            provider_metadata: metadata,
            expires_at: None,
            credential_ref: credential_ref.clone(),
        });
        let saved = match saved {
            Ok(connection) => connection,
            Err(_) => {
                let cleanup_store = store;
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    cleanup_store.delete(&credential_ref)
                })
                .await;
                return Err(command_error(
                    "connection_store_failed",
                    "The tested Telegram connection could not be saved.",
                ));
            }
        };
        self.cancel(&input.pairing_session_id);
        Ok(AppConnectionDto::from(saved))
    }

    pub fn cancel(&self, pairing_session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.remove(pairing_session_id) {
                session.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    fn session(&self, pairing_session_id: &str) -> Option<Arc<PairingSession>> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(pairing_session_id).cloned())
    }

    fn remove_expired_sessions(&self) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        let now = Instant::now();
        sessions.retain(|_, session| {
            let keep = !session.cancelled.load(Ordering::SeqCst) && now < session.expires_at;
            if !keep {
                session.cancelled.store(true, Ordering::SeqCst);
            }
            keep
        });
    }

    async fn resolve_pairing(
        &self,
        session: &PairingSession,
    ) -> Result<PairedChat, IntegrationCommandError> {
        let expected = Zeroizing::new(format!("/start {}", session.nonce));
        let mut offset: Option<i64> = None;
        let mut matches = Vec::new();
        for _ in 0..TELEGRAM_PAIRING_REQUESTS {
            if session.cancelled.load(Ordering::SeqCst) || Instant::now() >= session.expires_at {
                return Err(pairing_expired_error());
            }
            let mut body = serde_json::json!({
                "limit": 100,
                "timeout": TELEGRAM_PAIRING_LONG_POLL_SECONDS,
                "allowed_updates": ["message"]
            });
            if let Some(value) = offset {
                body["offset"] = Value::Number(value.into());
            }
            let updates: Vec<TelegramUpdate> = self
                .call("getUpdates", &session.token, &body, CallPhase::Lookup)
                .await
                .map_err(pairing_api_error)?;
            if session.cancelled.load(Ordering::SeqCst) || Instant::now() >= session.expires_at {
                return Err(pairing_expired_error());
            }
            offset = updates
                .iter()
                .map(|update| update.update_id)
                .max()
                .and_then(|value| value.checked_add(1))
                .or(offset);
            for update in updates {
                let Some(message) = update.message else {
                    continue;
                };
                if message.text.as_deref() == Some(expected.as_str()) {
                    matches.push(message.chat);
                }
            }
        }
        if matches.is_empty() {
            return Err(command_error(
                "telegram_pairing_not_found",
                "Press Start in the opened Telegram chat, then try finishing again.",
            ));
        }
        if matches.len() != 1 {
            return Err(command_error(
                "telegram_pairing_ambiguous",
                "More than one pairing message matched. Start pairing again with a new link.",
            ));
        }
        let matched = matches.pop().expect("one pairing match");
        if matched.kind != "private" {
            return Err(private_chat_error());
        }
        let chat: TelegramChat = self
            .call(
                "getChat",
                &session.token,
                &serde_json::json!({ "chat_id": matched.id }),
                CallPhase::Lookup,
            )
            .await
            .map_err(pairing_api_error)?;
        if session.cancelled.load(Ordering::SeqCst) || Instant::now() >= session.expires_at {
            return Err(pairing_expired_error());
        }
        if chat.id != matched.id || chat.kind != "private" {
            return Err(private_chat_error());
        }
        Ok(PairedChat {
            id: chat.id,
            mask: mask_chat_id(chat.id),
        })
    }

    async fn send_message(
        &self,
        token: &str,
        chat_id: i64,
        message: &str,
    ) -> Result<TelegramSentMessage, TelegramApiError> {
        let sent: TelegramSentMessage = self
            .call(
                "sendMessage",
                token,
                &serde_json::json!({ "chat_id": chat_id, "text": message }),
                CallPhase::Dispatch,
            )
            .await?;
        if sent.chat.id != chat_id || sent.chat.kind != "private" {
            return Err(invalid_response_for(CallPhase::Dispatch));
        }
        Ok(sent)
    }

    fn send_lock(&self, connection_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, ActionError> {
        let mut locks = self
            .send_locks
            .lock()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        Ok(locks
            .entry(connection_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }

    fn record_send_attempt(&self, connection_id: &str) -> Result<(), ActionError> {
        let mut histories = self
            .send_history
            .lock()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        let window = histories.entry(connection_id.to_owned()).or_default();
        window
            .record(Instant::now())
            .map_err(ActionError::rate_limited)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        token: &str,
        body: &Value,
        phase: CallPhase,
    ) -> Result<T, TelegramApiError> {
        let request = telegram_client()
            .post(format!("{}/bot{token}/{method}", self.api_base))
            .json(body);
        parse_telegram_response(request, phase).await
    }
}

impl ActionExecutor for TelegramService {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            let message = request
                .input
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
            validate_action_message(message)?;
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let (token, chat_id) = tokens.with_credential(|credential| {
                (
                    credential.access_token.clone(),
                    credential
                        .provider_fields
                        .get(TELEGRAM_CHAT_ID_FIELD)
                        .cloned(),
                )
            })?;
            let token = Zeroizing::new(token);
            let chat_id = Zeroizing::new(
                chat_id.ok_or_else(|| ActionError::new(ActionErrorCode::ConnectionRequired))?,
            );
            let chat_id = chat_id
                .parse::<i64>()
                .map_err(|_| ActionError::new(ActionErrorCode::ConnectionRequired))?;
            let send_lock = self.send_lock(&connection.id)?;
            let _guard = send_lock.lock().await;
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            self.record_send_attempt(&connection.id)?;
            let sent = self
                .send_message(token.as_str(), chat_id, message)
                .await
                .map_err(map_action_api_error)?;
            let masked_destination = connection
                .provider_metadata
                .get("recipient_mask")
                .cloned()
                .unwrap_or_else(|| "Telegram private chat".into());
            Ok(ActionResult {
                summary: "Telegram accepted the notification".into(),
                output: serde_json::json!({
                    "schemaVersion": 1,
                    "messageId": sent.message_id,
                    "acceptedAt": Utc::now().to_rfc3339(),
                    "maskedDestination": masked_destination,
                }),
                artifacts: Vec::<ActionArtifact>::new(),
                provider_request_id: None,
            })
        })
    }
}

#[derive(Default)]
struct SendWindow {
    attempts: VecDeque<Instant>,
}

impl SendWindow {
    fn record(&mut self, now: Instant) -> Result<(), Option<u64>> {
        while self
            .attempts
            .front()
            .is_some_and(|attempt| now.duration_since(*attempt) >= Duration::from_secs(60 * 60))
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= TELEGRAM_HOUR_LIMIT {
            let retry = retry_after(now, self.attempts[0], Duration::from_secs(60 * 60));
            return Err(Some(retry));
        }
        let minute_start = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        let recent = self
            .attempts
            .iter()
            .copied()
            .filter(|attempt| *attempt > minute_start)
            .collect::<Vec<_>>();
        if recent.len() >= TELEGRAM_MINUTE_LIMIT {
            return Err(Some(retry_after(now, recent[0], Duration::from_secs(60))));
        }
        self.attempts.push_back(now);
        Ok(())
    }
}

fn retry_after(now: Instant, first: Instant, window: Duration) -> u64 {
    window
        .saturating_sub(now.duration_since(first))
        .as_secs()
        .max(1)
}

#[derive(Deserialize)]
struct TelegramEnvelope<T> {
    #[serde(default)]
    ok: bool,
    result: Option<T>,
    #[serde(default)]
    error_code: Option<u16>,
    #[serde(default)]
    parameters: Option<TelegramResponseParameters>,
}

#[derive(Deserialize)]
struct TelegramResponseParameters {
    #[serde(default)]
    retry_after: Option<u64>,
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    is_bot: bool,
    first_name: String,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Deserialize)]
struct TelegramWebhookInfo {
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct TelegramSentMessage {
    message_id: i64,
    chat: TelegramChat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallPhase {
    Lookup,
    Dispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramApiErrorKind {
    RateLimited,
    Unauthorized,
    Rejected,
    Unavailable,
    InvalidResponse,
    Ambiguous,
}

#[derive(Debug)]
struct TelegramApiError {
    kind: TelegramApiErrorKind,
    retry_after: Option<u64>,
}

impl TelegramApiError {
    fn new(kind: TelegramApiErrorKind) -> Self {
        Self {
            kind,
            retry_after: None,
        }
    }

    fn retry_after(mut self, seconds: Option<u64>) -> Self {
        self.retry_after = seconds.map(|value| value.min(86_400));
        self
    }
}

async fn parse_telegram_response<T: DeserializeOwned>(
    request: RequestBuilder,
    phase: CallPhase,
) -> Result<T, TelegramApiError> {
    let response = request.send().await.map_err(|error| {
        if phase == CallPhase::Dispatch && !error.is_connect() {
            TelegramApiError::new(TelegramApiErrorKind::Ambiguous)
        } else {
            TelegramApiError::new(TelegramApiErrorKind::Unavailable)
        }
    })?;
    let status = response.status();
    let header_retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if response
        .content_length()
        .is_some_and(|length| length as usize > TELEGRAM_RESPONSE_LIMIT)
    {
        return Err(invalid_response_for(phase));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| invalid_response_for(phase))?;
        if bytes.len().saturating_add(chunk.len()) > TELEGRAM_RESPONSE_LIMIT {
            return Err(invalid_response_for(phase));
        }
        bytes.extend_from_slice(&chunk);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        let body_retry_after = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| value.get("parameters")?.get("retry_after")?.as_u64());
        return Err(TelegramApiError::new(TelegramApiErrorKind::RateLimited)
            .retry_after(body_retry_after.or(header_retry_after)));
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(TelegramApiError::new(TelegramApiErrorKind::Unauthorized));
    }
    if status.is_server_error() {
        return Err(TelegramApiError::new(if phase == CallPhase::Dispatch {
            TelegramApiErrorKind::Ambiguous
        } else {
            TelegramApiErrorKind::Unavailable
        }));
    }
    let envelope = serde_json::from_slice::<TelegramEnvelope<T>>(&bytes)
        .map_err(|_| invalid_response_for(phase))?;
    let retry_after = envelope
        .parameters
        .and_then(|parameters| parameters.retry_after)
        .or(header_retry_after);
    let code = envelope.error_code.unwrap_or(status.as_u16());
    if code == 429 {
        return Err(
            TelegramApiError::new(TelegramApiErrorKind::RateLimited).retry_after(retry_after)
        );
    }
    if matches!(code, 401 | 403) {
        return Err(TelegramApiError::new(TelegramApiErrorKind::Unauthorized));
    }
    if code >= 500 {
        return Err(TelegramApiError::new(if phase == CallPhase::Dispatch {
            TelegramApiErrorKind::Ambiguous
        } else {
            TelegramApiErrorKind::Unavailable
        }));
    }
    if !status.is_success() || !envelope.ok {
        return Err(TelegramApiError::new(TelegramApiErrorKind::Rejected));
    }
    envelope.result.ok_or_else(|| invalid_response_for(phase))
}

fn invalid_response_for(phase: CallPhase) -> TelegramApiError {
    TelegramApiError::new(if phase == CallPhase::Dispatch {
        TelegramApiErrorKind::Ambiguous
    } else {
        TelegramApiErrorKind::InvalidResponse
    })
}

fn telegram_client() -> Client {
    Client::builder()
        .timeout(TELEGRAM_HTTP_TIMEOUT)
        .user_agent("Alfred/0.5 TelegramConnector")
        .build()
        .expect("static Telegram HTTP client configuration")
}

fn validate_bot_token(token: &str) -> Result<(), IntegrationCommandError> {
    let Some((prefix, secret)) = token.split_once(':') else {
        return Err(token_invalid_error());
    };
    if token.is_empty()
        || token.len() > TELEGRAM_TOKEN_MAX_BYTES
        || prefix.is_empty()
        || secret.is_empty()
        || !prefix.bytes().all(|byte| byte.is_ascii_digit())
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(token_invalid_error());
    }
    Ok(())
}

fn valid_bot_username(value: &str) -> bool {
    (5..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && value.to_ascii_lowercase().ends_with("bot")
}

fn bounded_display_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= 64 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn validate_test_message(message: &str) -> Result<(), IntegrationCommandError> {
    if message.trim().is_empty() || message.chars().count() > TELEGRAM_MESSAGE_MAX_CHARS {
        return Err(command_error(
            "telegram_test_message_invalid",
            "Enter a test message between 1 and 4,096 characters.",
        ));
    }
    Ok(())
}

fn validate_action_message(message: &str) -> Result<(), ActionError> {
    if message.trim().is_empty() || message.chars().count() > TELEGRAM_MESSAGE_MAX_CHARS {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(())
}

fn ensure_no_telegram_connection(db: &Db) -> Result<(), IntegrationCommandError> {
    let exists = db
        .list_app_connections()
        .map_err(|_| connection_store_error())?
        .iter()
        .any(|connection| connection.provider_id == "telegram");
    if exists {
        return Err(command_error(
            "telegram_connection_exists",
            "Disconnect the current Telegram bot before pairing another one.",
        ));
    }
    Ok(())
}

fn mask_chat_id(chat_id: i64) -> String {
    let digits = chat_id.to_string();
    let digits = digits.trim_start_matches('-');
    let suffix = if digits.len() > 4 {
        &digits[digits.len() - 4..]
    } else {
        digits
    };
    format!("private chat ••••{suffix}")
}

fn session_is_active(session: &PairingSession) -> bool {
    !session.cancelled.load(Ordering::SeqCst) && Instant::now() < session.expires_at
}

fn expire_session_later(
    sessions: PairingSessions,
    session_id: String,
    expected: Arc<PairingSession>,
) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TELEGRAM_PAIRING_TTL).await;
        let Ok(mut sessions) = sessions.lock() else {
            return;
        };
        if sessions
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &expected))
        {
            expected.cancelled.store(true, Ordering::SeqCst);
            sessions.remove(&session_id);
        }
    });
}

fn map_action_api_error(error: TelegramApiError) -> ActionError {
    match error.kind {
        TelegramApiErrorKind::RateLimited => ActionError::rate_limited(error.retry_after),
        TelegramApiErrorKind::Unauthorized | TelegramApiErrorKind::Rejected => {
            ActionError::new(ActionErrorCode::ConnectionRequired)
        }
        TelegramApiErrorKind::Unavailable | TelegramApiErrorKind::InvalidResponse => {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
        TelegramApiErrorKind::Ambiguous => ActionError::new(ActionErrorCode::DeliveryUnknown),
    }
}

fn connect_api_error(error: TelegramApiError) -> IntegrationCommandError {
    match error.kind {
        TelegramApiErrorKind::Unauthorized | TelegramApiErrorKind::Rejected => {
            token_invalid_error()
        }
        TelegramApiErrorKind::RateLimited => command_error(
            "rate_limited",
            "Telegram is rate limiting connection checks. Try again later.",
        ),
        _ => command_error(
            "provider_unavailable",
            "Telegram could not be reached. Try again later.",
        ),
    }
}

fn pairing_api_error(error: TelegramApiError) -> IntegrationCommandError {
    match error.kind {
        TelegramApiErrorKind::Unauthorized => token_invalid_error(),
        TelegramApiErrorKind::RateLimited => command_error(
            "rate_limited",
            "Telegram is rate limiting pairing checks. Try again later.",
        ),
        _ => command_error(
            "provider_unavailable",
            "Telegram pairing could not be checked. Try again.",
        ),
    }
}

fn test_send_error(error: TelegramApiError) -> IntegrationCommandError {
    match error.kind {
        TelegramApiErrorKind::RateLimited => command_error(
            "rate_limited",
            "Telegram is rate limiting the test notification. Try again later.",
        ),
        TelegramApiErrorKind::Unauthorized => token_invalid_error(),
        TelegramApiErrorKind::Rejected => command_error(
            "telegram_test_failed",
            "Telegram rejected the test notification. Make sure the bot chat is still available.",
        ),
        _ => command_error(
            "provider_unavailable",
            "Telegram could not accept the test notification. Try again.",
        ),
    }
}

fn token_invalid_error() -> IntegrationCommandError {
    command_error("telegram_token_invalid", "Use a valid BotFather bot token.")
}

fn private_chat_error() -> IntegrationCommandError {
    command_error(
        "telegram_private_chat_required",
        "Pair Alfred from a private one-to-one Telegram chat, not a group or channel.",
    )
}

fn pairing_expired_error() -> IntegrationCommandError {
    command_error(
        "telegram_pairing_expired",
        "This pairing session expired. Validate the bot token again.",
    )
}

fn pairing_state_error() -> IntegrationCommandError {
    command_error(
        "provider_unavailable",
        "Telegram pairing state is temporarily unavailable.",
    )
}

fn credential_write_error() -> IntegrationCommandError {
    command_error(
        "credential_store_locked",
        "Unlock the system credential store and try again.",
    )
}

fn map_token_store_connect_error(error: TokenStoreError) -> IntegrationCommandError {
    match error {
        TokenStoreError::Locked => credential_write_error(),
        _ => command_error(
            "telegram_connection_failed",
            "The Telegram credential could not be saved.",
        ),
    }
}

fn connection_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "Connected-app details could not be read or updated.",
    )
}

fn command_error(code: &str, message: &str) -> IntegrationCommandError {
    IntegrationCommandError::new(code, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use std::thread::JoinHandle;
    use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

    const TOKEN: &str = "123456:secret_token_fixture";
    const CHAT_ID: i64 = 987_654_321;

    #[derive(Clone, Copy)]
    enum PairingFixture {
        Success,
        NoMatch,
        Ambiguous,
        Group,
        TestRejected,
        TestAmbiguous,
    }

    struct FixtureServer {
        service: TelegramService,
        nonce: Arc<Mutex<Option<String>>>,
        requests: Arc<Mutex<Vec<(String, String)>>>,
        thread: JoinHandle<()>,
    }

    struct LockedTokenStore;

    impl TokenStore for LockedTokenStore {
        fn put(
            &self,
            _credential_ref: &str,
            _credential: &CredentialEnvelope,
        ) -> Result<(), TokenStoreError> {
            Err(TokenStoreError::Locked)
        }

        fn get(&self, _credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError> {
            Err(TokenStoreError::Missing)
        }

        fn delete(&self, _credential_ref: &str) -> Result<(), TokenStoreError> {
            Err(TokenStoreError::Missing)
        }
    }

    fn pairing_server(fixture: PairingFixture) -> FixtureServer {
        let server = Server::http(("127.0.0.1", 0)).expect("fixture server");
        let port = server
            .server_addr()
            .to_ip()
            .expect("fixture address")
            .port();
        let nonce = Arc::new(Mutex::new(None::<String>));
        let thread_nonce = nonce.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_requests = requests.clone();
        let request_count = match fixture {
            PairingFixture::Success
            | PairingFixture::TestRejected
            | PairingFixture::TestAmbiguous => 7,
            PairingFixture::NoMatch | PairingFixture::Ambiguous | PairingFixture::Group => 5,
        };
        let thread = std::thread::spawn(move || {
            let mut update_call = 0;
            for _ in 0..request_count {
                let mut request = server.recv().expect("fixture request");
                let url = request.url().to_owned();
                let mut body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut body)
                    .expect("request body");
                thread_requests
                    .lock()
                    .expect("request capture")
                    .push((url.clone(), body));
                let nonce = thread_nonce
                    .lock()
                    .expect("nonce")
                    .clone()
                    .unwrap_or_default();
                let (status, response) = if url.ends_with("/getMe") {
                    (
                        200,
                        serde_json::json!({
                            "ok": true,
                            "result": {
                                "id": 42,
                                "is_bot": true,
                                "first_name": "Alfred Notices",
                                "username": "alfred_fixture_bot"
                            }
                        }),
                    )
                } else if url.ends_with("/getWebhookInfo") {
                    (200, serde_json::json!({"ok": true, "result": {"url": ""}}))
                } else if url.ends_with("/getUpdates") {
                    update_call += 1;
                    let result = match (fixture, update_call) {
                        (
                            PairingFixture::Success
                            | PairingFixture::TestRejected
                            | PairingFixture::TestAmbiguous,
                            1,
                        ) => {
                            serde_json::json!([{
                                "update_id": 10,
                                "message": {
                                    "chat": {"id": CHAT_ID, "type": "private"},
                                    "text": format!("/start {nonce}"),
                                    "untrusted_extra": "raw-update-content-fixture"
                                }
                            }])
                        }
                        (PairingFixture::Ambiguous, 1) => serde_json::json!([
                            {
                                "update_id": 10,
                                "message": {
                                    "chat": {"id": CHAT_ID, "type": "private"},
                                    "text": format!("/start {nonce}")
                                }
                            },
                            {
                                "update_id": 11,
                                "message": {
                                    "chat": {"id": CHAT_ID, "type": "private"},
                                    "text": format!("/start {nonce}")
                                }
                            }
                        ]),
                        (PairingFixture::Group, 1) => serde_json::json!([{
                            "update_id": 10,
                            "message": {
                                "chat": {"id": -100123, "type": "supergroup"},
                                "text": format!("/start {nonce}")
                            }
                        }]),
                        _ => serde_json::json!([]),
                    };
                    (200, serde_json::json!({"ok": true, "result": result}))
                } else if url.ends_with("/getChat") {
                    (
                        200,
                        serde_json::json!({
                            "ok": true,
                            "result": {"id": CHAT_ID, "type": "private"}
                        }),
                    )
                } else if matches!(fixture, PairingFixture::TestRejected) {
                    (400, serde_json::json!({"ok": false, "error_code": 400}))
                } else if matches!(fixture, PairingFixture::TestAmbiguous) {
                    (500, serde_json::json!({"ok": false, "error_code": 500}))
                } else {
                    (
                        200,
                        serde_json::json!({
                            "ok": true,
                            "result": {
                                "message_id": 77,
                                "chat": {"id": CHAT_ID, "type": "private"}
                            }
                        }),
                    )
                };
                let content_type =
                    Header::from_bytes("Content-Type", "application/json").expect("header");
                request
                    .respond(
                        Response::from_string(response.to_string())
                            .with_status_code(TinyStatusCode(status))
                            .with_header(content_type),
                    )
                    .expect("fixture response");
            }
        });
        FixtureServer {
            service: TelegramService {
                api_base: format!("http://127.0.0.1:{port}"),
                ..Default::default()
            },
            nonce,
            requests,
            thread,
        }
    }

    async fn prepared_pairing(fixture: &FixtureServer, db: &Db) -> TelegramPairingPrepared {
        let prepared = fixture
            .service
            .prepare(
                db,
                TelegramPrepareInput {
                    bot_token: TOKEN.into(),
                },
            )
            .await
            .expect("prepare pairing");
        let nonce = prepared
            .pairing_url
            .split("?start=")
            .nth(1)
            .expect("pairing nonce")
            .to_owned();
        *fixture.nonce.lock().expect("nonce") = Some(nonce);
        prepared
    }

    fn response_server(responses: Vec<(u16, Value)>) -> (String, JoinHandle<()>) {
        let server = Server::http(("127.0.0.1", 0)).expect("response server");
        let port = server
            .server_addr()
            .to_ip()
            .expect("response address")
            .port();
        let thread = std::thread::spawn(move || {
            for (status, body) in responses {
                let request = server.recv().expect("response request");
                request
                    .respond(
                        Response::from_string(body.to_string())
                            .with_status_code(TinyStatusCode(status)),
                    )
                    .expect("response");
            }
        });
        (format!("http://127.0.0.1:{port}"), thread)
    }

    fn action_connection() -> AppConnection {
        AppConnection {
            id: "telegram-connection".into(),
            provider_id: "telegram".into(),
            display_name: Some("Alfred Notices → private chat ••••4321".into()),
            external_account_id: Some("@alfred_fixture_bot".into()),
            external_tenant_id: None,
            connection_mode: "private_bot".into(),
            identity_key: "digest".into(),
            scopes: Vec::new(),
            provider_metadata: BTreeMap::from([(
                "recipient_mask".into(),
                "private chat ••••4321".into(),
            )]),
            status: ConnectionStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "telegram-credential".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    async fn action_tokens() -> TokenAccessCapability {
        let store = Arc::new(InMemoryTokenStore::default());
        let mut credential = CredentialEnvelope::new(TOKEN.into());
        credential
            .provider_fields
            .insert(TELEGRAM_CHAT_ID_FIELD.into(), CHAT_ID.to_string());
        store
            .put("telegram-credential", &credential)
            .expect("credential");
        TokenAccessCapability::load(store, "telegram-credential".into())
            .await
            .expect("token capability")
    }

    fn action_request(message: &str) -> ValidatedActionRequest {
        ValidatedActionRequest {
            connection_id: "telegram-connection".into(),
            provider_id: "telegram".into(),
            action_id: "telegram.send_personal_message".into(),
            input: BTreeMap::from([("message".into(), Value::String(message.into()))]),
        }
    }

    #[test]
    fn descriptor_has_one_interpolatable_non_secret_message() {
        let descriptor = send_personal_message_descriptor();
        assert_eq!(descriptor.provider_id, "telegram");
        assert_eq!(descriptor.action_id, "telegram.send_personal_message");
        assert!(descriptor.required_scopes.is_empty());
        assert_eq!(descriptor.fields.len(), 1);
        assert_eq!(descriptor.fields[0].key, "message");
        assert!(descriptor.fields[0].supports_interpolation);
        assert!(!descriptor.fields[0].secret);
        assert!(descriptor
            .fields
            .iter()
            .all(|field| field.key != "chat" && field.key != "recipient"));
    }

    #[test]
    fn message_validation_counts_unicode_characters() {
        assert!(validate_action_message("hello").is_ok());
        assert!(validate_action_message("  ").is_err());
        assert!(validate_action_message(&"🟢".repeat(TELEGRAM_MESSAGE_MAX_CHARS)).is_ok());
        assert!(validate_action_message(&"🟢".repeat(TELEGRAM_MESSAGE_MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn local_rate_window_enforces_minute_and_hour_caps() {
        let base = Instant::now();
        let mut minute = SendWindow::default();
        for index in 0..TELEGRAM_MINUTE_LIMIT {
            minute
                .record(base + Duration::from_secs(index as u64))
                .expect("inside minute allowance");
        }
        assert!(minute.record(base + Duration::from_secs(5)).is_err());
        assert!(minute.record(base + Duration::from_secs(61)).is_ok());

        let mut hour = SendWindow::default();
        for index in 0..TELEGRAM_HOUR_LIMIT {
            hour.record(base + Duration::from_secs(index as u64 * 12))
                .expect("inside hourly allowance");
        }
        assert!(hour
            .record(base + Duration::from_secs(TELEGRAM_HOUR_LIMIT as u64 * 12))
            .is_err());
    }

    #[test]
    fn masks_destination_and_validates_token_without_echoing_it() {
        assert_eq!(mask_chat_id(123456789), "private chat ••••6789");
        assert_eq!(mask_chat_id(-987654), "private chat ••••7654");
        let secret = "123456:secret/token-fixture";
        let error = validate_bot_token(secret).expect_err("invalid token fixture");
        assert!(!serde_json::to_string(&error)
            .expect("safe error")
            .contains(secret));
        assert!(validate_bot_token("123456:secret_token-fixture").is_ok());
    }

    #[test]
    fn provider_is_action_only_and_not_an_event_source() {
        let state =
            crate::integrations::IntegrationsState::new(Arc::new(InMemoryTokenStore::default()));
        let provider = state
            .catalog
            .list()
            .into_iter()
            .find(|provider| provider.id == "telegram")
            .expect("Telegram provider");
        assert_eq!(provider.connection_modes, vec!["private_bot".to_string()]);
        assert!(provider.connect_available);
        assert_eq!(state.actions.descriptors(Some("telegram")).len(), 1);
        assert!(state.events.descriptors(Some("telegram")).is_empty());
    }

    #[tokio::test]
    async fn prepare_rejects_revoked_tokens_non_bots_and_existing_webhooks() {
        let fixtures = vec![
            (
                vec![(401, serde_json::json!({"ok": false, "error_code": 401}))],
                "telegram_token_invalid",
            ),
            (
                vec![(
                    200,
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "id": 42,
                            "is_bot": false,
                            "first_name": "Not a bot",
                            "username": "alfred_fixture_bot"
                        }
                    }),
                )],
                "telegram_identity_invalid",
            ),
            (
                vec![
                    (
                        200,
                        serde_json::json!({
                            "ok": true,
                            "result": {
                                "id": 42,
                                "is_bot": true,
                                "first_name": "Alfred Notices",
                                "username": "alfred_fixture_bot"
                            }
                        }),
                    ),
                    (
                        200,
                        serde_json::json!({
                            "ok": true,
                            "result": {"url": "https://example.invalid/webhook"}
                        }),
                    ),
                ],
                "telegram_webhook_conflict",
            ),
        ];
        for (responses, expected) in fixtures {
            let (api_base, thread) = response_server(responses);
            let service = TelegramService {
                api_base,
                ..Default::default()
            };
            let error = match service
                .prepare(
                    &Db::open_in_memory().expect("database"),
                    TelegramPrepareInput {
                        bot_token: TOKEN.into(),
                    },
                )
                .await
            {
                Ok(_) => panic!("prepare rejection expected"),
                Err(error) => error,
            };
            thread.join().expect("response thread");
            assert_eq!(error.code, expected);
            assert!(!serde_json::to_string(&error)
                .expect("error")
                .contains(TOKEN));
        }
    }

    #[tokio::test]
    async fn pairing_saves_only_redacted_metadata_after_the_test_succeeds() {
        let fixture = pairing_server(PairingFixture::Success);
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let prepared = prepared_pairing(&fixture, &db).await;
        let nonce = fixture.nonce.lock().expect("nonce").clone().unwrap();
        let dto = fixture
            .service
            .complete(
                &db,
                store.clone(),
                TelegramCompleteInput {
                    pairing_session_id: prepared.pairing_session_id.clone(),
                    test_message: "Alfred test notification".into(),
                },
            )
            .await
            .expect("complete pairing");
        fixture.thread.join().expect("fixture thread");

        let serialized = serde_json::to_string(&dto).expect("DTO");
        assert!(serialized.contains("••••4321"));
        for forbidden in [
            TOKEN,
            &CHAT_ID.to_string(),
            &nonce,
            "raw-update-content-fixture",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        let saved = db
            .list_app_connections()
            .expect("connections")
            .pop()
            .expect("connection");
        let metadata = serde_json::to_string(&saved.provider_metadata).expect("metadata");
        assert!(!metadata.contains(&CHAT_ID.to_string()));
        assert!(!metadata.contains(TOKEN));
        assert!(!metadata.contains(&nonce));
        let credential = store.get(&saved.credential_ref).expect("saved credential");
        assert_eq!(credential.access_token, TOKEN);
        assert_eq!(
            credential.provider_fields.get(TELEGRAM_CHAT_ID_FIELD),
            Some(&CHAT_ID.to_string())
        );
        assert_eq!(
            fixture
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|(url, _)| url.ends_with("/getUpdates"))
                .count(),
            TELEGRAM_PAIRING_REQUESTS
        );
        assert_eq!(
            fixture
                .service
                .complete(
                    &db,
                    store,
                    TelegramCompleteInput {
                        pairing_session_id: prepared.pairing_session_id,
                        test_message: "replay".into(),
                    },
                )
                .await
                .expect_err("one connection and nonce cannot be replayed")
                .code,
            "telegram_connection_exists"
        );
    }

    #[tokio::test]
    async fn pairing_fails_closed_for_no_match_multiple_matches_and_groups() {
        for (kind, expected) in [
            (PairingFixture::NoMatch, "telegram_pairing_not_found"),
            (PairingFixture::Ambiguous, "telegram_pairing_ambiguous"),
            (PairingFixture::Group, "telegram_private_chat_required"),
        ] {
            let fixture = pairing_server(kind);
            let db = Db::open_in_memory().expect("database");
            let prepared = prepared_pairing(&fixture, &db).await;
            let error = fixture
                .service
                .complete(
                    &db,
                    Arc::new(InMemoryTokenStore::default()),
                    TelegramCompleteInput {
                        pairing_session_id: prepared.pairing_session_id,
                        test_message: "test".into(),
                    },
                )
                .await
                .expect_err("pairing must fail closed");
            fixture.thread.join().expect("fixture thread");
            assert_eq!(error.code, expected);
            assert!(db.list_app_connections().expect("connections").is_empty());
        }
    }

    #[tokio::test]
    async fn rejected_test_never_creates_a_ready_connection() {
        let fixture = pairing_server(PairingFixture::TestRejected);
        let db = Db::open_in_memory().expect("database");
        let prepared = prepared_pairing(&fixture, &db).await;
        let error = fixture
            .service
            .complete(
                &db,
                Arc::new(InMemoryTokenStore::default()),
                TelegramCompleteInput {
                    pairing_session_id: prepared.pairing_session_id,
                    test_message: "test".into(),
                },
            )
            .await
            .expect_err("test rejection");
        fixture.thread.join().expect("fixture thread");
        assert_eq!(error.code, "telegram_test_failed");
        assert!(db.list_app_connections().expect("connections").is_empty());
    }

    #[tokio::test]
    async fn credential_store_failure_leaves_no_connection_metadata() {
        let fixture = pairing_server(PairingFixture::Success);
        let db = Db::open_in_memory().expect("database");
        let prepared = prepared_pairing(&fixture, &db).await;
        let error = fixture
            .service
            .complete(
                &db,
                Arc::new(LockedTokenStore),
                TelegramCompleteInput {
                    pairing_session_id: prepared.pairing_session_id,
                    test_message: "test".into(),
                },
            )
            .await
            .expect_err("credential store rejection");
        fixture.thread.join().expect("fixture thread");
        assert_eq!(error.code, "credential_store_locked");
        assert!(db.list_app_connections().expect("connections").is_empty());
    }

    #[tokio::test]
    async fn ambiguous_test_is_not_saved_or_retried_with_the_same_session() {
        let fixture = pairing_server(PairingFixture::TestAmbiguous);
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let prepared = prepared_pairing(&fixture, &db).await;
        let session_id = prepared.pairing_session_id;
        let error = fixture
            .service
            .complete(
                &db,
                store.clone(),
                TelegramCompleteInput {
                    pairing_session_id: session_id.clone(),
                    test_message: "test".into(),
                },
            )
            .await
            .expect_err("ambiguous test");
        fixture.thread.join().expect("fixture thread");
        assert_eq!(error.code, "telegram_test_delivery_unknown");
        assert!(db.list_app_connections().expect("connections").is_empty());
        assert_eq!(
            fixture
                .service
                .complete(
                    &db,
                    store,
                    TelegramCompleteInput {
                        pairing_session_id: session_id,
                        test_message: "do not retry".into(),
                    },
                )
                .await
                .expect_err("session consumed after ambiguity")
                .code,
            "telegram_pairing_expired"
        );
    }

    #[tokio::test]
    async fn expired_pairing_session_is_removed_before_network_use() {
        let service = TelegramService::default();
        service.sessions.lock().expect("sessions").insert(
            "expired".into(),
            Arc::new(PairingSession {
                token: TOKEN.into(),
                bot_id: 42,
                bot_username: "alfred_fixture_bot".into(),
                bot_display_name: "Alfred".into(),
                nonce: "nonce".into(),
                expires_at: Instant::now() - Duration::from_secs(1),
                cancelled: AtomicBool::new(false),
                paired_chat: Mutex::new(None),
            }),
        );
        let error = service
            .complete(
                &Db::open_in_memory().expect("database"),
                Arc::new(InMemoryTokenStore::default()),
                TelegramCompleteInput {
                    pairing_session_id: "expired".into(),
                    test_message: "test".into(),
                },
            )
            .await
            .expect_err("expired session");
        assert_eq!(error.code, "telegram_pairing_expired");
        assert!(service.sessions.lock().expect("sessions").is_empty());
    }

    #[tokio::test]
    async fn action_returns_minimal_accepted_output_and_plain_text_request() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let request_body = Arc::new(Mutex::new(String::new()));
        let captured = request_body.clone();
        let thread = std::thread::spawn(move || {
            let mut request = server.recv().expect("send request");
            request
                .as_reader()
                .read_to_string(&mut captured.lock().expect("capture"))
                .expect("body");
            request
                .respond(Response::from_string(
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "message_id": 91,
                            "chat": {"id": CHAT_ID, "type": "private"}
                        }
                    })
                    .to_string(),
                ))
                .expect("response");
        });
        let service = TelegramService {
            api_base: format!("http://127.0.0.1:{port}"),
            ..Default::default()
        };
        let message = "private resolved message fixture";
        let result = service
            .execute(
                &action_request(message),
                &action_connection(),
                action_tokens().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action success");
        thread.join().expect("server thread");
        let output = serde_json::to_string(&result).expect("result");
        assert!(output.contains("Telegram accepted"));
        assert!(output.contains("••••4321"));
        assert!(!output.contains(message));
        assert!(!output.contains(TOKEN));
        assert!(!output.contains(&CHAT_ID.to_string()));
        let body: Value = serde_json::from_str(&request_body.lock().expect("body")).expect("JSON");
        assert_eq!(body.get("text").and_then(Value::as_str), Some(message));
        assert!(body.get("parse_mode").is_none());
        assert_eq!(body.as_object().expect("object").len(), 2);
    }

    #[tokio::test]
    async fn ambiguous_dispatch_and_telegram_429_map_to_stable_action_errors() {
        for (status, body, expected, retry) in [
            (
                500,
                serde_json::json!({"ok": false, "error_code": 500}),
                ActionErrorCode::DeliveryUnknown,
                None,
            ),
            (
                429,
                serde_json::json!({
                    "ok": false,
                    "error_code": 429,
                    "parameters": {"retry_after": 17}
                }),
                ActionErrorCode::RateLimited,
                Some(17),
            ),
        ] {
            let server = Server::http(("127.0.0.1", 0)).expect("server");
            let port = server.server_addr().to_ip().expect("address").port();
            let thread = std::thread::spawn(move || {
                let request = server.recv().expect("request");
                request
                    .respond(
                        Response::from_string(body.to_string())
                            .with_status_code(TinyStatusCode(status)),
                    )
                    .expect("response");
            });
            let service = TelegramService {
                api_base: format!("http://127.0.0.1:{port}"),
                ..Default::default()
            };
            let error = service
                .execute(
                    &action_request("test"),
                    &action_connection(),
                    action_tokens().await,
                    ActionCancellation::never(),
                )
                .await
                .expect_err("action failure");
            thread.join().expect("server thread");
            assert_eq!(error.code, expected);
            assert_eq!(error.retry_after_seconds, retry);
            assert!(!error.message.contains(TOKEN));
        }
    }

    #[tokio::test]
    async fn malformed_and_oversized_send_responses_are_delivery_unknown() {
        for body in [
            b"not-json".to_vec(),
            vec![b'x'; TELEGRAM_RESPONSE_LIMIT + 1],
        ] {
            let server = Server::http(("127.0.0.1", 0)).expect("server");
            let port = server.server_addr().to_ip().expect("address").port();
            let thread = std::thread::spawn(move || {
                let request = server.recv().expect("request");
                request
                    .respond(Response::from_data(body))
                    .expect("response");
            });
            let service = TelegramService {
                api_base: format!("http://127.0.0.1:{port}"),
                ..Default::default()
            };
            let error = service
                .execute(
                    &action_request("test"),
                    &action_connection(),
                    action_tokens().await,
                    ActionCancellation::never(),
                )
                .await
                .expect_err("invalid response");
            thread.join().expect("server thread");
            assert_eq!(error.code, ActionErrorCode::DeliveryUnknown);
        }
    }

    #[tokio::test]
    async fn sends_for_one_connection_are_serialized() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let thread = std::thread::spawn(move || {
            let first = server.recv().expect("first request");
            assert!(server
                .recv_timeout(Duration::from_millis(150))
                .expect("serialization check")
                .is_none());
            first
                .respond(Response::from_string(
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "message_id": 1,
                            "chat": {"id": CHAT_ID, "type": "private"}
                        }
                    })
                    .to_string(),
                ))
                .expect("first response");
            let second = server.recv().expect("second request");
            second
                .respond(Response::from_string(
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "message_id": 2,
                            "chat": {"id": CHAT_ID, "type": "private"}
                        }
                    })
                    .to_string(),
                ))
                .expect("second response");
        });
        let service = Arc::new(TelegramService {
            api_base: format!("http://127.0.0.1:{port}"),
            ..Default::default()
        });
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let mut tasks = Vec::new();
        for message in ["first", "second"] {
            let service = service.clone();
            let barrier = barrier.clone();
            let tokens = action_tokens().await;
            tasks.push(tokio::spawn(async move {
                let request = action_request(message);
                let connection = action_connection();
                barrier.wait().await;
                service
                    .execute(&request, &connection, tokens, ActionCancellation::never())
                    .await
            }));
        }
        barrier.wait().await;
        for task in tasks {
            task.await.expect("send task").expect("serialized send");
        }
        thread.join().expect("server thread");
    }

    #[tokio::test]
    async fn cancelled_action_does_not_dispatch() {
        let service = TelegramService::default();
        let cancellation = ActionCancellation::new(Arc::new(AtomicBool::new(true)));
        let error = service
            .execute(
                &action_request("test"),
                &action_connection(),
                action_tokens().await,
                cancellation,
            )
            .await
            .expect_err("cancelled");
        assert_eq!(error.code, ActionErrorCode::Cancelled);
    }
}
