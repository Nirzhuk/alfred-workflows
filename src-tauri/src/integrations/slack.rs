//! Slack private-workspace action connector.
//!
//! This is deliberately the advanced, local-only mode. A user supplies their
//! own Slack bot token after creating a private Slack app. The secret goes
//! directly to this Rust command, is validated with `auth.test`, and then moves
//! into the OS credential store. It never enters connection metadata, workflow
//! JSON, or an action descriptor.

use super::actions::{
    ActionArtifact, ActionCancellation, ActionDescriptor, ActionError, ActionErrorCode,
    ActionExecutor, ActionFieldDescriptor, ActionFieldKind, ActionFuture, ActionLimits,
    ActionOption, ActionRegistry, ActionResourceItem, ActionResourcePage, ActionResourcesFuture,
    ActionResult, TokenAccessCapability, ValidatedActionRequest,
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
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

const SLACK_API_BASE: &str = "https://slack.com/api";
const SLACK_MESSAGE_RECOMMENDED_CHARS: usize = 4_000;
const SLACK_MESSAGE_HARD_CHARS: usize = 40_000;
const SLACK_THREAD_TS_MAX_BYTES: usize = 64;
const SLACK_HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const SLACK_RESPONSE_LIMIT: usize = 256 * 1024;
const SLACK_SOCKET_MESSAGE_LIMIT: usize = 256 * 1024;
const SLACK_SOCKET_IDLE_SLICE: Duration = Duration::from_secs(10);
const SLACK_SOCKET_BATCH_LIMIT: usize = 100;
const SLACK_SOCKET_DRAIN_WAIT: Duration = Duration::from_millis(2);
const SLACK_PERMALINK_LOOKUP_BUDGET: Duration = Duration::from_millis(750);
const SLACK_SOCKET_APP_TOKEN_FIELD: &str = "socket_app_token";

type SlackWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackPrivateConnectionMode {
    Bot,
    IncomingWebhook,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackPrivateConnectionInput {
    pub mode: SlackPrivateConnectionMode,
    pub bot_token: String,
    #[serde(default)]
    pub app_token: Option<String>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub enable_private_channels: bool,
    #[serde(default)]
    pub enable_mentions: bool,
}

struct SlackSecrets {
    bot_token: Option<String>,
    app_token: Option<String>,
    webhook_url: Option<String>,
    enable_private_channels: bool,
    enable_mentions: bool,
}

impl Drop for SlackSecrets {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.bot_token.zeroize();
        self.app_token.zeroize();
        self.webhook_url.zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackApiResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    enterprise_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    bot_id: Option<String>,
    #[serde(default)]
    channel: Option<Value>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    response_metadata: Option<SlackResponseMetadata>,
    #[serde(default)]
    permalink: Option<String>,
    #[serde(default)]
    channels: Vec<SlackConversation>,
    #[serde(default)]
    url: Option<String>,
    #[serde(skip)]
    granted_scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackResponseMetadata {
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackConversation {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    is_member: bool,
    #[serde(default)]
    is_archived: bool,
}

pub fn register(actions: &ActionRegistry, events: &AppEventRegistry) -> Result<(), ActionError> {
    let executor = Arc::new(SlackActionExecutor::default());
    actions.register(
        send_message_descriptor(),
        ActionLimits::default(),
        executor.clone(),
    )?;
    actions.register(reply_descriptor(), ActionLimits::default(), executor)?;
    events
        .register(
            app_mention_descriptor(),
            Arc::new(SlackSocketAdapter::default()),
        )
        .map_err(|error| ActionError::new(map_event_registration_error(error.code)))?;
    Ok(())
}

fn map_event_registration_error(code: AppEventErrorCode) -> ActionErrorCode {
    match code {
        AppEventErrorCode::ProviderUnavailable => ActionErrorCode::ProviderUnavailable,
        _ => ActionErrorCode::InvalidInput,
    }
}

fn conversation_field() -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: "conversation".into(),
        label: "Conversation".into(),
        description: "A public channel or accessible private channel.".into(),
        kind: ActionFieldKind::ResourceSelector,
        required: true,
        default: None,
        secret: false,
        option_source: Some("conversations".into()),
        options: Vec::<ActionOption>::new(),
        supports_interpolation: false,
    }
}

fn text_field() -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: "text".into(),
        label: "Message".into(),
        description: "Slack mrkdwn text. Keep messages concise.".into(),
        kind: ActionFieldKind::Textarea,
        required: true,
        default: None,
        secret: false,
        option_source: None,
        options: Vec::<ActionOption>::new(),
        supports_interpolation: true,
    }
}

fn send_message_descriptor() -> ActionDescriptor {
    ActionDescriptor {
        provider_id: "slack".into(),
        action_id: "slack.send_message".into(),
        label: "Send Slack message".into(),
        description: "Send a mrkdwn message with the selected Slack connection.".into(),
        fields: vec![conversation_field(), text_field()],
        required_scopes: vec!["chat:write".into(), "channels:read".into()],
        output_schema_version: 1,
    }
}

fn reply_descriptor() -> ActionDescriptor {
    ActionDescriptor {
        provider_id: "slack".into(),
        action_id: "slack.reply_in_thread".into(),
        label: "Reply in Slack thread".into(),
        description: "Reply to an existing message thread.".into(),
        fields: vec![
            conversation_field(),
            ActionFieldDescriptor {
                key: "thread_ts".into(),
                label: "Thread timestamp".into(),
                description: "The parent message timestamp, for example 1712345678.000100.".into(),
                kind: ActionFieldKind::Text,
                required: true,
                default: None,
                secret: false,
                option_source: None,
                options: Vec::<ActionOption>::new(),
                supports_interpolation: true,
            },
            text_field(),
        ],
        required_scopes: vec!["chat:write".into(), "channels:read".into()],
        output_schema_version: 1,
    }
}

fn app_mention_descriptor() -> AppEventDescriptor {
    AppEventDescriptor {
        provider_id: "slack".into(),
        event_type: "slack.app_mention".into(),
        label: "App mention".into(),
        description: "Run when someone mentions your private Slack app.".into(),
        required_scopes: vec!["app_mentions:read".into(), "connections:write".into()],
        delivery_modes: vec![AppEventDeliveryMode::Socket],
        filter_fields: vec![ActionFieldDescriptor {
            key: "channelId".into(),
            label: "Channel".into(),
            description: "Optionally limit mentions to one accessible channel.".into(),
            kind: ActionFieldKind::ResourceSelector,
            required: false,
            default: None,
            secret: false,
            option_source: Some("conversations".into()),
            options: Vec::new(),
            supports_interpolation: false,
        }],
        fetches_resource_content: false,
        descriptor_version: 1,
        external_event_id_required: true,
        allowed_attribute_keys: vec![
            "teamId".into(),
            "channelId".into(),
            "userId".into(),
            "messageTs".into(),
            "threadTs".into(),
        ],
        poll_interval_seconds: 1,
        pending_cap: 100,
    }
}

pub async fn connect_private(
    db: &Db,
    store: Arc<dyn TokenStore>,
    input: SlackPrivateConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let secrets = validate_private_input(input)?;
    match secrets.webhook_url.as_deref() {
        Some(webhook_url) => connect_incoming_webhook(db, store, webhook_url).await,
        None => connect_bot(db, store, &secrets).await,
    }
}

fn validate_private_input(
    mut input: SlackPrivateConnectionInput,
) -> Result<SlackSecrets, IntegrationCommandError> {
    use zeroize::Zeroize;
    let result = match input.mode {
        SlackPrivateConnectionMode::Bot => {
            let bot = zeroize::Zeroizing::new(input.bot_token.trim().to_owned());
            let app = input
                .app_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if app.is_some() && !input.enable_mentions {
                Err(safe_connect_error(
                    "slack_app_token_unused",
                    "Enable Socket Mode mentions before adding an app-level token.",
                ))
            } else if input.enable_mentions && app.is_none() {
                Err(safe_connect_error(
                    "slack_app_token_required",
                    "Add an app-level token to enable Socket Mode mentions.",
                ))
            } else if app
                .as_deref()
                .is_some_and(|value| !value.starts_with("xapp-"))
            {
                Err(safe_connect_error(
                    "slack_app_token_invalid",
                    "Use a Slack app-level token beginning with xapp-.",
                ))
            } else if !bot.starts_with("xoxb-") {
                Err(safe_connect_error(
                    "slack_token_invalid",
                    "Use a Slack bot token beginning with xoxb-.",
                ))
            } else {
                Ok(SlackSecrets {
                    bot_token: Some(bot.as_str().to_owned()),
                    app_token: app,
                    webhook_url: None,
                    enable_private_channels: input.enable_private_channels,
                    enable_mentions: input.enable_mentions,
                })
            }
        }
        SlackPrivateConnectionMode::IncomingWebhook => {
            let webhook = zeroize::Zeroizing::new(
                input
                    .webhook_url
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            );
            let valid = url::Url::parse(&webhook).ok().is_some_and(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some("hooks.slack.com")
                    && url.path().starts_with("/services/")
            });
            if !valid {
                Err(safe_connect_error(
                    "slack_webhook_invalid",
                    "Use an HTTPS Incoming Webhook URL from hooks.slack.com.",
                ))
            } else {
                Ok(SlackSecrets {
                    bot_token: None,
                    app_token: None,
                    webhook_url: Some(webhook.as_str().to_owned()),
                    enable_private_channels: false,
                    enable_mentions: false,
                })
            }
        }
    };
    input.bot_token.zeroize();
    input.app_token.zeroize();
    input.webhook_url.zeroize();
    result
}

async fn connect_bot(
    db: &Db,
    store: Arc<dyn TokenStore>,
    secrets: &SlackSecrets,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let bot_token = secrets
        .bot_token
        .as_deref()
        .ok_or_else(|| safe_connect_error("slack_token_invalid", "A bot token is required."))?;
    let response = slack_get(SLACK_API_BASE, "auth.test", bot_token, &[])
        .await
        .map_err(connect_api_error)?;
    if !response.ok {
        return Err(connect_slack_error(response.error.as_deref()));
    }
    let required_scopes =
        required_bot_scopes(secrets.enable_private_channels, secrets.enable_mentions);
    if required_scopes.iter().any(|required| {
        !response
            .granted_scopes
            .iter()
            .any(|scope| scope == required)
    }) {
        return Err(safe_connect_error(
            "scope_missing",
            "Add the required Slack bot scopes, reinstall the app, and try again.",
        ));
    }
    let team_id = response
        .team_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            safe_connect_error(
                "slack_identity_invalid",
                "Slack did not return a workspace identity.",
            )
        })?;
    let bot_user_id = response
        .user_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            safe_connect_error(
                "slack_identity_invalid",
                "Slack did not return a bot identity.",
            )
        })?;
    if secrets.enable_mentions {
        let app_token = secrets.app_token.as_deref().ok_or_else(|| {
            safe_connect_error(
                "slack_app_token_required",
                "An app-level token is required for Socket Mode.",
            )
        })?;
        validate_socket_app_token(SLACK_API_BASE, app_token)
            .await
            .map_err(connect_socket_error)?;
    }
    let identity_key = canonical_identity_key("slack", "private_bot", &[&team_id, &bot_user_id]);
    let existing = db
        .get_app_connection_by_identity("slack", "private_bot", &identity_key)
        .map_err(|_| {
            safe_connect_error(
                "connection_store_failed",
                "Slack was validated, but existing connection metadata could not be read.",
            )
        })?;
    let credential_ref = existing
        .as_ref()
        .map(|connection| connection.credential_ref.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let is_new_credential = existing.is_none();
    let prior_credential = if is_new_credential {
        None
    } else {
        let prior_store = store.clone();
        let prior_ref = credential_ref.clone();
        tauri::async_runtime::spawn_blocking(move || prior_store.get(&prior_ref))
            .await
            .ok()
            .and_then(Result::ok)
    };
    let mut envelope = CredentialEnvelope::new(bot_token.to_owned());
    if let Some(app_token) = secrets.app_token.as_deref() {
        envelope
            .provider_fields
            .insert(SLACK_SOCKET_APP_TOKEN_FIELD.into(), app_token.to_owned());
    }
    let credential_store = store.clone();
    let credential_ref_for_store = credential_ref.clone();
    tauri::async_runtime::spawn_blocking(move || {
        credential_store.put(&credential_ref_for_store, &envelope)
    })
    .await
    .map_err(|_| credential_write_error())?
    .map_err(map_token_store_connect_error)?;

    let mut scopes = response.granted_scopes.clone();
    if secrets.enable_mentions {
        scopes.push("connections:write".into());
    }
    scopes.sort();
    scopes.dedup();
    let mut provider_metadata = BTreeMap::from([("team_id".into(), team_id.clone())]);
    if let Some(bot_id) = response.bot_id.clone() {
        provider_metadata.insert("bot_id".into(), bot_id);
    }
    if let Some(enterprise_id) = response.enterprise_id.clone() {
        provider_metadata.insert("enterprise_id".into(), enterprise_id);
    }
    let connection = match db.upsert_app_connection(UpsertAppConnection {
        provider_id: "slack".into(),
        display_name: response.team,
        external_account_id: Some(bot_user_id),
        external_tenant_id: Some(team_id.clone()),
        connection_mode: "private_bot".into(),
        identity_key,
        scopes,
        provider_metadata,
        expires_at: None,
        credential_ref: credential_ref.clone(),
    }) {
        Ok(connection) => connection,
        Err(_) => {
            if is_new_credential {
                let cleanup_store = store.clone();
                let cleanup_ref = credential_ref.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    cleanup_store.delete(&cleanup_ref)
                })
                .await;
            } else if let Some(prior) = prior_credential {
                let rollback_store = store.clone();
                let rollback_ref = credential_ref.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    rollback_store.put(&rollback_ref, &prior)
                })
                .await;
            }
            return Err(safe_connect_error(
                "connection_store_failed",
                "Slack was validated, but the connection could not be saved.",
            ));
        }
    };
    Ok(AppConnectionDto::from(connection))
}

fn required_bot_scopes(enable_private_channels: bool, enable_mentions: bool) -> Vec<&'static str> {
    let mut scopes = vec!["chat:write", "channels:read"];
    if enable_private_channels {
        scopes.push("groups:read");
    }
    if enable_mentions {
        scopes.push("app_mentions:read");
    }
    scopes
}

async fn connect_incoming_webhook(
    _db: &Db,
    _store: Arc<dyn TokenStore>,
    _webhook_url: &str,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    Err(safe_connect_error(
        "slack_webhook_validation_required",
        "Incoming Webhook setup is not enabled until Alfred can verify its workspace identity.",
    ))
}

#[derive(Default)]
struct SlackActionExecutor {
    api_base: Option<String>,
}

impl SlackActionExecutor {
    fn api_base(&self) -> &str {
        self.api_base.as_deref().unwrap_or(SLACK_API_BASE)
    }
}

impl ActionExecutor for SlackActionExecutor {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            let conversation = required_string(&request.input, "conversation")?;
            let text = required_string(&request.input, "text")?;
            validate_message(&text)?;
            let thread_ts = if request.action_id == "slack.reply_in_thread" {
                let value = required_string(&request.input, "thread_ts")?;
                validate_thread_ts(&value)?;
                Some(value)
            } else {
                None
            };
            let token = zeroize::Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let mut body = serde_json::json!({
                "channel": conversation,
                "text": text,
                "unfurl_links": false,
                "unfurl_media": false,
            });
            if let Some(thread_ts) = thread_ts.as_deref() {
                body["thread_ts"] = Value::String(thread_ts.into());
            }
            let response =
                slack_post_json(self.api_base(), "chat.postMessage", token.as_str(), &body).await?;
            if !response.ok {
                return Err(map_slack_action_error(response.error.as_deref()));
            }
            let channel = channel_id(&response.channel).unwrap_or(conversation);
            let ts = response
                .ts
                .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
            let permalink = slack_get(
                self.api_base(),
                "chat.getPermalink",
                token.as_str(),
                &[("channel", channel.as_str()), ("message_ts", ts.as_str())],
            )
            .await
            .ok()
            .and_then(|value| value.ok.then_some(value.permalink).flatten());
            let mut output = serde_json::json!({ "channel": channel, "timestamp": ts });
            if let Some(permalink) = permalink {
                output["permalink"] = Value::String(permalink);
            }
            Ok(ActionResult {
                summary: if request.action_id == "slack.reply_in_thread" {
                    "Replied in Slack thread".into()
                } else {
                    "Sent Slack message".into()
                },
                output,
                artifacts: Vec::<ActionArtifact>::new(),
                provider_request_id: None,
            })
        })
    }

    fn list_resources<'a>(
        &'a self,
        source: &'a str,
        _field_key: &'a str,
        query: &'a str,
        page_token: Option<&'a str>,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async move {
            if source != "conversations" {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
            let token = zeroize::Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            let conversation_types = if connection.scopes.iter().any(|scope| scope == "groups:read")
            {
                "public_channel,private_channel"
            } else {
                "public_channel"
            };
            let mut params = vec![
                ("limit", "100"),
                ("exclude_archived", "true"),
                ("types", conversation_types),
            ];
            if let Some(cursor) = page_token {
                params.push(("cursor", cursor));
            }
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let response = slack_get(
                self.api_base(),
                "conversations.list",
                token.as_str(),
                &params,
            )
            .await
            .map_err(map_action_transport_error)?;
            if !response.ok {
                return Err(map_slack_action_error(response.error.as_deref()));
            }
            let query = query.trim().to_ascii_lowercase();
            let items = response
                .channels
                .into_iter()
                .filter(|channel| !channel.is_archived)
                .filter_map(|channel| {
                    let label = channel.name.unwrap_or_else(|| channel.id.clone());
                    if query.is_empty() || label.to_ascii_lowercase().contains(&query) {
                        Some(ActionResourceItem {
                            id: channel.id,
                            label: format!("#{label}"),
                        })
                    } else {
                        None
                    }
                })
                .take(100)
                .collect();
            Ok(ActionResourcePage {
                items,
                next_page_token: response
                    .response_metadata
                    .and_then(|metadata| metadata.next_cursor)
                    .filter(|value| !value.is_empty()),
            })
        })
    }
}

#[derive(Default)]
struct SlackSocketAdapter {
    api_base: Option<String>,
    allow_insecure_test_socket: bool,
    sockets: Mutex<HashMap<String, Arc<tokio::sync::Mutex<SlackSocketState>>>>,
}

#[derive(Default)]
struct SlackSocketState {
    stream: Option<SlackWebSocket>,
}

enum SocketMessageOutcome {
    Continue,
    Reconnect,
    Event(NormalizedAppEvent),
}

impl SlackSocketAdapter {
    fn api_base(&self) -> &str {
        self.api_base.as_deref().unwrap_or(SLACK_API_BASE)
    }

    fn socket_state(
        &self,
        connection_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<SlackSocketState>>, AppEventError> {
        let mut sockets = self
            .sockets
            .lock()
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
        Ok(sockets
            .entry(connection_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(SlackSocketState::default())))
            .clone())
    }

    async fn ensure_socket(
        &self,
        state: &mut SlackSocketState,
        app_token: &str,
    ) -> Result<(), AppEventError> {
        if state.stream.is_some() {
            return Ok(());
        }
        let socket_url = zeroize::Zeroizing::new(
            open_socket_url(self.api_base(), app_token, self.allow_insecure_test_socket)
                .await
                .map_err(map_slack_transport_to_event)?,
        );
        let socket_config = WebSocketConfig::default()
            .read_buffer_size(32 * 1024)
            .write_buffer_size(4 * 1024)
            .max_write_buffer_size(64 * 1024)
            .max_message_size(Some(SLACK_SOCKET_MESSAGE_LIMIT))
            .max_frame_size(Some(SLACK_SOCKET_MESSAGE_LIMIT));
        let (stream, _) =
            connect_async_with_config(socket_url.as_str(), Some(socket_config), false)
                .await
                .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
        state.stream = Some(stream);
        Ok(())
    }

    async fn next_batch(
        &self,
        connection: &AppConnection,
        bot_token: &str,
        app_token: &str,
        cancellation: AppEventCancellation,
    ) -> Result<AppEventBatch, AppEventError> {
        let state = self.socket_state(&connection.id)?;
        let mut state = state.lock().await;
        let mut events = Vec::new();
        loop {
            if cancellation.is_cancelled() {
                state.stream = None;
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            self.ensure_socket(&mut state, app_token).await?;
            let wait = if events.is_empty() {
                SLACK_SOCKET_IDLE_SLICE
            } else {
                SLACK_SOCKET_DRAIN_WAIT
            };
            let next = tokio::select! {
                _ = cancellation.wait() => {
                    state.stream = None;
                    return Err(AppEventError::new(AppEventErrorCode::Cancelled));
                }
                result = tokio::time::timeout(
                    wait,
                    state.stream.as_mut().expect("socket established").next(),
                ) => result,
            };
            let message = match next {
                Err(_) if events.is_empty() => {
                    return Ok(AppEventBatch {
                        subscription_id: Some("socket_mode".into()),
                        ..Default::default()
                    });
                }
                Err(_) => break,
                Ok(Some(Ok(message))) => message,
                Ok(Some(Err(_))) | Ok(None) if events.is_empty() => {
                    state.stream = None;
                    return Err(AppEventError::new(AppEventErrorCode::ProviderUnavailable));
                }
                Ok(Some(Err(_))) | Ok(None) => {
                    state.stream = None;
                    break;
                }
            };
            let outcome = process_socket_message(
                state.stream.as_mut().expect("socket established"),
                message,
                connection,
            )
            .await?;
            match outcome {
                SocketMessageOutcome::Continue => {}
                SocketMessageOutcome::Reconnect => {
                    state.stream = None;
                    if !events.is_empty() {
                        break;
                    }
                    let jitter = rand::random::<u64>() % 401 + 100;
                    tokio::time::sleep(Duration::from_millis(jitter)).await;
                }
                SocketMessageOutcome::Event(event) => events.push(event),
            }
            if events.len() >= SLACK_SOCKET_BATCH_LIMIT {
                break;
            }
        }
        enrich_permalinks(&mut events, bot_token, self.api_base()).await;
        Ok(AppEventBatch {
            events,
            subscription_id: Some("socket_mode".into()),
            ..Default::default()
        })
    }
}

impl AppEventAdapter for SlackSocketAdapter {
    fn poll<'a>(
        &'a self,
        _config: &'a AppTriggerConfig,
        _connection: &'a AppConnection,
        _cursor: Option<&'a str>,
        _tokens: TokenAccessCapability,
        _cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        Box::pin(async { Err(AppEventError::new(AppEventErrorCode::InvalidInput)) })
    }

    fn connect<'a>(
        &'a self,
        _config: &'a AppTriggerConfig,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        Box::pin(async move {
            let (bot_token, app_token) = tokens
                .with_credential(|credential| {
                    (
                        credential.access_token.clone(),
                        credential
                            .provider_fields
                            .get(SLACK_SOCKET_APP_TOKEN_FIELD)
                            .cloned(),
                    )
                })
                .map_err(map_action_error_to_event)?;
            let bot_token = zeroize::Zeroizing::new(bot_token);
            let app_token = zeroize::Zeroizing::new(
                app_token
                    .ok_or_else(|| AppEventError::new(AppEventErrorCode::ConnectionRequired))?,
            );
            if !app_token.starts_with("xapp-") {
                return Err(AppEventError::new(AppEventErrorCode::ConnectionRequired));
            }
            self.next_batch(
                connection,
                bot_token.as_str(),
                app_token.as_str(),
                cancellation,
            )
            .await
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
            if field_key != "channelId" || cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
            }
            let token = zeroize::Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let conversation_types = if connection.scopes.iter().any(|scope| scope == "groups:read")
            {
                "public_channel,private_channel"
            } else {
                "public_channel"
            };
            let mut params = vec![
                ("limit", "100"),
                ("exclude_archived", "true"),
                ("types", conversation_types),
            ];
            if let Some(cursor) = page_token {
                params.push(("cursor", cursor));
            }
            let response = slack_get(
                self.api_base(),
                "conversations.list",
                token.as_str(),
                &params,
            )
            .await
            .map_err(map_slack_transport_to_event)?;
            if !response.ok {
                return Err(map_slack_error_to_event(response.error.as_deref()));
            }
            let query = query.trim().to_ascii_lowercase();
            let items = response
                .channels
                .into_iter()
                .filter(|channel| !channel.is_archived)
                .filter_map(|channel| {
                    let label = channel.name.unwrap_or_else(|| channel.id.clone());
                    (query.is_empty() || label.to_ascii_lowercase().contains(&query)).then(|| {
                        AppEventResourceItem {
                            id: channel.id,
                            label: format!("#{label}"),
                        }
                    })
                })
                .take(100)
                .collect();
            Ok(AppEventResourcePage {
                items,
                next_page_token: response
                    .response_metadata
                    .and_then(|metadata| metadata.next_cursor)
                    .filter(|value| !value.is_empty()),
            })
        })
    }

    fn reset(&self) {
        if let Ok(mut sockets) = self.sockets.lock() {
            sockets.clear();
        }
    }
}

async fn process_socket_message(
    socket: &mut SlackWebSocket,
    message: Message,
    connection: &AppConnection,
) -> Result<SocketMessageOutcome, AppEventError> {
    let text = match message {
        Message::Text(text) => {
            if text.len() > SLACK_SOCKET_MESSAGE_LIMIT {
                return Err(AppEventError::new(AppEventErrorCode::EventTooLarge));
            }
            text.to_string()
        }
        Message::Binary(bytes) => {
            if bytes.len() > SLACK_SOCKET_MESSAGE_LIMIT {
                return Err(AppEventError::new(AppEventErrorCode::EventTooLarge));
            }
            String::from_utf8(bytes.to_vec())
                .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?
        }
        Message::Ping(payload) => {
            socket
                .send(Message::Pong(payload))
                .await
                .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
            return Ok(SocketMessageOutcome::Continue);
        }
        Message::Pong(_) => return Ok(SocketMessageOutcome::Continue),
        Message::Close(_) => return Ok(SocketMessageOutcome::Reconnect),
        Message::Frame(_) => return Ok(SocketMessageOutcome::Continue),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    if value.get("type").and_then(Value::as_str) == Some("disconnect") {
        return Ok(SocketMessageOutcome::Reconnect);
    }
    let Some(envelope_id) = value
        .get("envelope_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256)
    else {
        return Ok(SocketMessageOutcome::Continue);
    };

    // Ack before payload inspection or any optional Web API lookup. Slack may
    // retry an unacknowledged envelope; Plan 010 then deduplicates by event ID.
    let ack = serde_json::json!({ "envelope_id": envelope_id }).to_string();
    socket
        .send(Message::Text(ack.into()))
        .await
        .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;

    let Some(event) = normalize_slack_mention(&value, connection)? else {
        return Ok(SocketMessageOutcome::Continue);
    };
    Ok(SocketMessageOutcome::Event(event))
}

async fn enrich_permalinks(events: &mut [NormalizedAppEvent], bot_token: &str, api_base: &str) {
    let targets = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            Some((
                index,
                event.attributes.get("channelId")?.as_str()?.to_owned(),
                event.attributes.get("messageTs")?.as_str()?.to_owned(),
            ))
        })
        .collect::<Vec<_>>();
    let lookups = futures_util::stream::iter(targets)
        .map(|(index, channel, message_ts)| async move {
            let url = slack_get(
                api_base,
                "chat.getPermalink",
                bot_token,
                &[
                    ("channel", channel.as_str()),
                    ("message_ts", message_ts.as_str()),
                ],
            )
            .await
            .ok()
            .and_then(|response| response.ok.then_some(response.permalink).flatten())
            .filter(|url| url.starts_with("https://"));
            (index, url)
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>();
    let Ok(results) = tokio::time::timeout(SLACK_PERMALINK_LOOKUP_BUDGET, lookups).await else {
        return;
    };
    for (index, url) in results {
        events[index].resource_url = url;
    }
}

fn normalize_slack_mention(
    envelope: &Value,
    connection: &AppConnection,
) -> Result<Option<NormalizedAppEvent>, AppEventError> {
    if envelope.get("type").and_then(Value::as_str) != Some("events_api") {
        return Ok(None);
    }
    let payload = envelope
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    if payload.get("type").and_then(Value::as_str) != Some("event_callback") {
        return Ok(None);
    }
    let event_id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let team_id = payload
        .get("team_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let expected_team = connection
        .provider_metadata
        .get("team_id")
        .or(connection.external_tenant_id.as_ref())
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::ConnectionRequired))?;
    if team_id != expected_team {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    let event = payload
        .get("event")
        .and_then(Value::as_object)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    if event.get("type").and_then(Value::as_str) != Some("app_mention")
        || event.get("subtype").is_some_and(|value| !value.is_null())
        || event.get("edited").is_some_and(|value| !value.is_null())
        || event.get("bot_id").is_some_and(|value| !value.is_null())
    {
        return Ok(None);
    }
    let user_id = bounded_slack_id(event.get("user"))?;
    if connection.external_account_id.as_deref() == Some(user_id) {
        return Ok(None);
    }
    let channel_id = bounded_slack_id(event.get("channel"))?;
    let message_ts = event
        .get("ts")
        .and_then(Value::as_str)
        .filter(|value| validate_thread_ts(value).is_ok())
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let occurred_at = payload
        .get("event_time")
        .and_then(Value::as_i64)
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
        .or_else(|| slack_timestamp(message_ts))
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?
        .to_rfc3339();
    let preview = event
        .get("text")
        .and_then(Value::as_str)
        .map(|text| bounded_preview(text, 1_000));
    let mut attributes = BTreeMap::from([
        ("teamId".into(), Value::String(team_id.into())),
        ("channelId".into(), Value::String(channel_id.into())),
        ("userId".into(), Value::String(user_id.into())),
        ("messageTs".into(), Value::String(message_ts.into())),
    ]);
    if let Some(thread_ts) = event
        .get("thread_ts")
        .and_then(Value::as_str)
        .filter(|value| validate_thread_ts(value).is_ok())
    {
        attributes.insert("threadTs".into(), Value::String(thread_ts.into()));
    }
    Ok(Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "slack".into(),
        event_type: "slack.app_mention".into(),
        connection_id: connection.id.clone(),
        external_event_id: event_id.into(),
        occurred_at,
        subject: Some("Slack app mention".into()),
        actor: Some(user_id.into()),
        resource_url: None,
        preview,
        attributes,
    }))
}

fn bounded_slack_id(value: Option<&Value>) -> Result<&str, AppEventError> {
    value
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn bounded_preview(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    format!(
        "{}\n… (truncated)",
        text.chars().take(limit).collect::<String>()
    )
}

fn slack_timestamp(value: &str) -> Option<chrono::DateTime<Utc>> {
    let (seconds, micros) = value.split_once('.')?;
    let seconds = seconds.parse::<i64>().ok()?;
    let micros = micros.parse::<u32>().ok()?;
    Utc.timestamp_opt(seconds, micros.saturating_mul(1_000))
        .single()
}

fn map_action_error_to_event(error: ActionError) -> AppEventError {
    match error.code {
        ActionErrorCode::ConnectionRequired => {
            AppEventError::new(AppEventErrorCode::ConnectionRequired)
        }
        _ => AppEventError::new(AppEventErrorCode::ProviderUnavailable),
    }
}

fn map_slack_transport_to_event(error: SlackTransportError) -> AppEventError {
    match error.kind {
        SlackTransportErrorKind::RateLimited => AppEventError::new(AppEventErrorCode::RateLimited)
            .retry_after(error.retry_after.unwrap_or(30)),
        SlackTransportErrorKind::Unauthorized => {
            AppEventError::new(AppEventErrorCode::ProviderUnauthorized)
        }
        SlackTransportErrorKind::Unavailable | SlackTransportErrorKind::InvalidResponse => {
            AppEventError::new(AppEventErrorCode::ProviderUnavailable)
        }
    }
}

fn map_slack_error_to_event(error: Option<&str>) -> AppEventError {
    match error.unwrap_or_default() {
        "invalid_auth" | "not_authed" | "account_inactive" | "token_revoked" | "token_expired" => {
            AppEventError::new(AppEventErrorCode::ProviderUnauthorized)
        }
        "missing_scope" | "no_permission" | "not_allowed_token_type" => {
            AppEventError::new(AppEventErrorCode::ScopeMissing)
        }
        "ratelimited" => AppEventError::new(AppEventErrorCode::RateLimited),
        _ => AppEventError::new(AppEventErrorCode::ProviderUnavailable),
    }
}

fn required_string(input: &BTreeMap<String, Value>, key: &str) -> Result<String, ActionError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))
}

fn validate_message(text: &str) -> Result<(), ActionError> {
    if text.is_empty() || text.chars().count() > SLACK_MESSAGE_HARD_CHARS {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    let _recommended = text.chars().count() <= SLACK_MESSAGE_RECOMMENDED_CHARS;
    Ok(())
}

fn validate_thread_ts(value: &str) -> Result<(), ActionError> {
    let mut parts = value.split('.');
    let seconds = parts.next().unwrap_or_default();
    let micros = parts.next().unwrap_or_default();
    if value.len() > SLACK_THREAD_TS_MAX_BYTES
        || parts.next().is_some()
        || seconds.len() < 10
        || micros.len() != 6
        || !seconds.bytes().all(|byte| byte.is_ascii_digit())
        || !micros.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(())
}

async fn slack_get(
    api_base: &str,
    method: &str,
    token: &str,
    params: &[(&str, &str)],
) -> Result<SlackApiResponse, SlackTransportError> {
    let response = slack_client()
        .get(format!("{api_base}/{method}"))
        .bearer_auth(token)
        .query(params)
        .send()
        .await
        .map_err(SlackTransportError::from_reqwest)?;
    parse_slack_response(response).await
}

async fn validate_socket_app_token(
    api_base: &str,
    app_token: &str,
) -> Result<(), SlackTransportError> {
    open_socket_url(api_base, app_token, false)
        .await
        .map(|_| ())
}

async fn open_socket_url(
    api_base: &str,
    app_token: &str,
    allow_insecure_test_socket: bool,
) -> Result<String, SlackTransportError> {
    let response = slack_client()
        .post(format!("{api_base}/apps.connections.open"))
        .bearer_auth(app_token)
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(SlackTransportError::from_reqwest)?;
    let response = parse_slack_response(response).await?;
    if !response.ok {
        return Err(slack_api_transport_error(response.error.as_deref()));
    }
    let url = response.url.ok_or_else(invalid_slack_response)?;
    let parsed = url::Url::parse(&url).map_err(|_| invalid_slack_response())?;
    let trusted = parsed.scheme() == "wss"
        && parsed
            .host_str()
            .is_some_and(|host| host == "slack.com" || host.ends_with(".slack.com"));
    let local_test = allow_insecure_test_socket
        && parsed.scheme() == "ws"
        && parsed
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "localhost");
    if !trusted && !local_test {
        return Err(invalid_slack_response());
    }
    Ok(url)
}

fn invalid_slack_response() -> SlackTransportError {
    SlackTransportError {
        kind: SlackTransportErrorKind::InvalidResponse,
        retry_after: None,
    }
}

fn slack_api_transport_error(error: Option<&str>) -> SlackTransportError {
    let kind = match error.unwrap_or_default() {
        "invalid_auth"
        | "not_authed"
        | "account_inactive"
        | "token_revoked"
        | "token_expired"
        | "not_allowed_token_type"
        | "missing_scope" => SlackTransportErrorKind::Unauthorized,
        "ratelimited" => SlackTransportErrorKind::RateLimited,
        _ => SlackTransportErrorKind::Unavailable,
    };
    SlackTransportError {
        kind,
        retry_after: None,
    }
}

async fn slack_post_json(
    api_base: &str,
    method: &str,
    token: &str,
    body: &Value,
) -> Result<SlackApiResponse, ActionError> {
    let response = slack_client()
        .post(format!("{api_base}/{method}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() || error.is_connect() || error.is_body() {
                ActionError::new(ActionErrorCode::DeliveryUnknown)
            } else {
                ActionError::new(ActionErrorCode::ProviderUnavailable)
            }
        })?;
    parse_slack_response(response)
        .await
        .map_err(|error| match error.kind {
            SlackTransportErrorKind::RateLimited => ActionError::rate_limited(error.retry_after),
            SlackTransportErrorKind::Unauthorized => {
                ActionError::new(ActionErrorCode::ProviderUnauthorized)
            }
            SlackTransportErrorKind::Unavailable | SlackTransportErrorKind::InvalidResponse => {
                ActionError::new(ActionErrorCode::DeliveryUnknown)
            }
        })
}

fn slack_client() -> Client {
    Client::builder()
        .timeout(SLACK_HTTP_TIMEOUT)
        .user_agent("Alfred/0.5 SlackConnector")
        .build()
        .expect("static Slack HTTP client configuration")
}

#[derive(Debug)]
struct SlackTransportError {
    kind: SlackTransportErrorKind,
    retry_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlackTransportErrorKind {
    RateLimited,
    Unauthorized,
    Unavailable,
    InvalidResponse,
}

impl SlackTransportError {
    fn from_reqwest(error: reqwest::Error) -> Self {
        Self {
            kind: if error.is_timeout() || error.is_connect() {
                SlackTransportErrorKind::Unavailable
            } else {
                SlackTransportErrorKind::InvalidResponse
            },
            retry_after: None,
        }
    }
}

async fn parse_slack_response(
    response: reqwest::Response,
) -> Result<SlackApiResponse, SlackTransportError> {
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let granted_scopes = response
        .headers()
        .get("x-oauth-scopes")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(SlackTransportError {
            kind: SlackTransportErrorKind::RateLimited,
            retry_after,
        });
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(SlackTransportError {
            kind: SlackTransportErrorKind::Unauthorized,
            retry_after: None,
        });
    }
    if !status.is_success() {
        return Err(SlackTransportError {
            kind: SlackTransportErrorKind::Unavailable,
            retry_after: None,
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > SLACK_RESPONSE_LIMIT)
    {
        return Err(SlackTransportError {
            kind: SlackTransportErrorKind::InvalidResponse,
            retry_after: None,
        });
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(SlackTransportError::from_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > SLACK_RESPONSE_LIMIT {
            return Err(SlackTransportError {
                kind: SlackTransportErrorKind::InvalidResponse,
                retry_after: None,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut parsed: SlackApiResponse =
        serde_json::from_slice(&bytes).map_err(|_| SlackTransportError {
            kind: SlackTransportErrorKind::InvalidResponse,
            retry_after: None,
        })?;
    parsed.granted_scopes = granted_scopes;
    Ok(parsed)
}

fn map_action_transport_error(error: SlackTransportError) -> ActionError {
    match error.kind {
        SlackTransportErrorKind::RateLimited => ActionError::rate_limited(error.retry_after),
        SlackTransportErrorKind::Unauthorized => {
            ActionError::new(ActionErrorCode::ProviderUnauthorized)
        }
        SlackTransportErrorKind::Unavailable | SlackTransportErrorKind::InvalidResponse => {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
    }
}

fn connect_api_error(error: SlackTransportError) -> IntegrationCommandError {
    match error.kind {
        SlackTransportErrorKind::RateLimited => safe_connect_error(
            "rate_limited",
            "Slack is rate limiting connection checks. Try again later.",
        ),
        SlackTransportErrorKind::Unauthorized => connect_slack_error(Some("invalid_auth")),
        _ => safe_connect_error(
            "provider_unavailable",
            "Slack could not be reached. Try again later.",
        ),
    }
}

fn connect_socket_error(error: SlackTransportError) -> IntegrationCommandError {
    match error.kind {
        SlackTransportErrorKind::RateLimited => safe_connect_error(
            "rate_limited",
            "Slack is rate limiting Socket Mode validation. Try again later.",
        ),
        SlackTransportErrorKind::Unauthorized => safe_connect_error(
            "slack_app_token_invalid",
            "Slack rejected the app-level token or its connections:write scope.",
        ),
        _ => safe_connect_error(
            "provider_unavailable",
            "Slack Socket Mode could not be validated. Try again later.",
        ),
    }
}

fn map_slack_action_error(error: Option<&str>) -> ActionError {
    match error.unwrap_or_default() {
        "invalid_auth" | "not_authed" | "account_inactive" | "token_revoked" | "token_expired" => {
            ActionError::new(ActionErrorCode::ProviderUnauthorized)
        }
        "missing_scope" | "no_permission" | "not_allowed_token_type" => {
            ActionError::new(ActionErrorCode::ScopeMissing)
        }
        "ratelimited" => ActionError::rate_limited(None),
        "channel_not_found" | "thread_not_found" | "invalid_arguments" | "invalid_arg_name" => {
            ActionError::new(ActionErrorCode::InvalidInput)
        }
        _ => ActionError::new(ActionErrorCode::ProviderUnavailable),
    }
}

fn connect_slack_error(error: Option<&str>) -> IntegrationCommandError {
    let code = match error.unwrap_or_default() {
        "invalid_auth" | "not_authed" | "token_revoked" | "token_expired" => "slack_token_invalid",
        "account_inactive" => "slack_account_inactive",
        _ => "slack_connection_failed",
    };
    safe_connect_error(code, "Slack rejected this private app connection.")
}

fn channel_id(value: &Option<Value>) -> Option<String> {
    value.as_ref().and_then(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_owned))
    })
}

fn safe_connect_error(code: &str, message: &str) -> IntegrationCommandError {
    IntegrationCommandError::new(code, message, true)
}

fn credential_write_error() -> IntegrationCommandError {
    safe_connect_error(
        "credential_store_locked",
        "Unlock the system credential store and try again.",
    )
}

fn map_token_store_connect_error(error: TokenStoreError) -> IntegrationCommandError {
    match error {
        TokenStoreError::Locked => credential_write_error(),
        _ => safe_connect_error(
            "slack_connection_failed",
            "The Slack credential could not be saved.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use tiny_http::{Header, Response, Server};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    fn connection(scopes: Vec<&str>) -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "slack".into(),
            display_name: Some("Workspace".into()),
            external_account_id: Some("UAPP".into()),
            external_tenant_id: Some("T123".into()),
            connection_mode: "private_bot".into(),
            identity_key: "identity".into(),
            scopes: scopes.into_iter().map(str::to_owned).collect(),
            provider_metadata: BTreeMap::from([("team_id".into(), "T123".into())]),
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
                &CredentialEnvelope::new("xoxb-secret-fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    async fn socket_token_capability() -> TokenAccessCapability {
        let store = Arc::new(InMemoryTokenStore::default());
        let mut credential = CredentialEnvelope::new("xoxb-secret-fixture".into());
        credential.provider_fields.insert(
            SLACK_SOCKET_APP_TOKEN_FIELD.into(),
            "xapp-secret-fixture".into(),
        );
        store.put("credential", &credential).expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    #[test]
    fn descriptors_are_least_privilege_and_secret_free() {
        for descriptor in [send_message_descriptor(), reply_descriptor()] {
            assert_eq!(
                descriptor.required_scopes,
                vec!["chat:write".to_string(), "channels:read".to_string()]
            );
            assert!(descriptor.fields.iter().all(|field| !field.secret));
            assert!(!serde_json::to_string(&descriptor)
                .expect("descriptor")
                .contains("xoxb-"));
        }
        let mention = app_mention_descriptor();
        assert_eq!(
            mention.required_scopes,
            vec![
                "app_mentions:read".to_string(),
                "connections:write".to_string()
            ]
        );
        assert!(mention.filter_fields.iter().all(|field| !field.secret));
    }

    #[test]
    fn private_channel_scope_is_opt_in() {
        assert_eq!(
            required_bot_scopes(false, false),
            vec!["chat:write", "channels:read"]
        );
        assert_eq!(
            required_bot_scopes(true, true),
            vec![
                "chat:write",
                "channels:read",
                "groups:read",
                "app_mentions:read"
            ]
        );
    }

    #[tokio::test]
    async fn socket_mode_reconnects_acknowledges_and_minimizes_app_mentions() {
        let first_listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("socket 1");
        let first_address = first_listener.local_addr().expect("address 1");
        let second_listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("socket 2");
        let second_address = second_listener.local_addr().expect("address 2");

        let first_socket = tokio::spawn(async move {
            let (stream, _) = first_listener.accept().await.expect("accept 1");
            let mut socket = accept_async(stream).await.expect("websocket 1");
            socket
                .send(Message::Text(
                    r#"{"type":"disconnect","reason":"refresh_requested"}"#.into(),
                ))
                .await
                .expect("disconnect");
        });
        let second_socket = tokio::spawn(async move {
            let (stream, _) = second_listener.accept().await.expect("accept 2");
            let mut socket = accept_async(stream).await.expect("websocket 2");
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "events_api",
                        "envelope_id": "env-1",
                        "accepts_response_payload": false,
                        "payload": {
                            "type": "event_callback",
                            "team_id": "T123",
                            "event_id": "Ev123",
                            "event_time": 1712345678,
                            "event": {
                                "type": "app_mention",
                                "user": "UUSER",
                                "text": "<@UAPP> please run the workflow",
                                "ts": "1712345678.000100",
                                "thread_ts": "1712345600.000001",
                                "channel": "C123",
                                "event_ts": "1712345678.000100",
                                "blocks": [{"type": "section", "text": {"text": "raw-block"}}]
                            }
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("event");
            let ack = socket
                .next()
                .await
                .expect("ack frame")
                .expect("ack message")
                .into_text()
                .expect("ack text");
            assert_eq!(ack, r#"{"envelope_id":"env-1"}"#);
        });

        let api_server = Server::http(("127.0.0.1", 0)).expect("api server");
        let api_port = api_server
            .server_addr()
            .to_ip()
            .expect("api address")
            .port();
        let api_thread = std::thread::spawn(move || {
            for response_body in [
                serde_json::json!({"ok": true, "url": format!("ws://{first_address}")}),
                serde_json::json!({"ok": true, "url": format!("ws://{second_address}")}),
                serde_json::json!({
                    "ok": true,
                    "permalink": "https://example.slack.com/archives/C123/p1712345678000100"
                }),
            ] {
                let request = api_server.recv().expect("API request");
                let content_type =
                    Header::from_bytes("Content-Type", "application/json").expect("header");
                request
                    .respond(
                        Response::from_string(response_body.to_string()).with_header(content_type),
                    )
                    .expect("API response");
            }
        });

        let adapter = SlackSocketAdapter {
            api_base: Some(format!("http://127.0.0.1:{api_port}")),
            allow_insecure_test_socket: true,
            ..Default::default()
        };
        let batch = adapter
            .connect(
                &AppTriggerConfig {
                    provider_id: "slack".into(),
                    event_type: "slack.app_mention".into(),
                    connection_id: "connection".into(),
                    filters: BTreeMap::new(),
                    descriptor_version: 1,
                },
                &connection(vec![
                    "chat:write",
                    "channels:read",
                    "app_mentions:read",
                    "connections:write",
                ]),
                socket_token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("socket batch");
        first_socket.await.expect("first socket");
        second_socket.await.expect("second socket");
        api_thread.join().expect("API thread");

        assert_eq!(batch.events.len(), 1);
        let event = &batch.events[0];
        assert_eq!(event.external_event_id, "Ev123");
        assert_eq!(event.actor.as_deref(), Some("UUSER"));
        assert_eq!(
            event.attributes.get("channelId"),
            Some(&Value::String("C123".into()))
        );
        assert_eq!(
            event.attributes.get("threadTs"),
            Some(&Value::String("1712345600.000001".into()))
        );
        assert!(event
            .resource_url
            .as_deref()
            .is_some_and(|url| url.starts_with("https://")));
        let serialized = serde_json::to_string(event).expect("event JSON");
        assert!(!serialized.contains("raw-block"));
        assert!(!serialized.contains("xoxb-secret-fixture"));
        assert!(!serialized.contains("xapp-secret-fixture"));
    }

    #[test]
    fn socket_fixture_rejects_bot_loops_edits_and_unsupported_subtypes() {
        let base = serde_json::json!({
            "type": "events_api",
            "envelope_id": "env",
            "payload": {
                "type": "event_callback",
                "team_id": "T123",
                "event_id": "Ev123",
                "event_time": 1712345678,
                "event": {
                    "type": "app_mention",
                    "user": "UUSER",
                    "text": "mention",
                    "ts": "1712345678.000100",
                    "channel": "C123"
                }
            }
        });
        let mut bot = base.clone();
        bot["payload"]["event"]["bot_id"] = Value::String("B123".into());
        assert!(normalize_slack_mention(&bot, &connection(vec![]))
            .expect("bot fixture")
            .is_none());
        let mut edited = base.clone();
        edited["payload"]["event"]["edited"] = serde_json::json!({"user": "U1"});
        assert!(normalize_slack_mention(&edited, &connection(vec![]))
            .expect("edited fixture")
            .is_none());
        let mut subtype = base;
        subtype["payload"]["event"]["subtype"] = Value::String("document_mention".into());
        assert!(normalize_slack_mention(&subtype, &connection(vec![]))
            .expect("subtype fixture")
            .is_none());
    }

    #[tokio::test]
    async fn conversation_listing_normalizes_page_and_cursor() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            assert!(request.url().starts_with("/conversations.list?"));
            assert!(request.url().contains("cursor=next-page"));
            assert!(request
                .url()
                .contains("types=public_channel%2Cprivate_channel"));
            let content_type =
                Header::from_bytes("Content-Type", "application/json").expect("header");
            request
                .respond(Response::from_string(
                    r#"{"ok":true,"channels":[{"id":"C123","name":"general","is_archived":false}],"response_metadata":{"next_cursor":"after"}}"#,
                ).with_header(content_type))
                .expect("respond");
        });
        let executor = SlackActionExecutor {
            api_base: Some(format!("http://127.0.0.1:{port}")),
        };
        let page = executor
            .list_resources(
                "conversations",
                "conversation",
                "gen",
                Some("next-page"),
                &connection(vec!["chat:write", "channels:read", "groups:read"]),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("page");
        responder.join().expect("responder");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "C123");
        assert_eq!(page.items[0].label, "#general");
        assert_eq!(page.next_page_token.as_deref(), Some("after"));
    }

    #[tokio::test]
    async fn send_normalizes_output_and_never_returns_raw_response_or_token() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            for expected in ["/chat.postMessage", "/chat.getPermalink"] {
                let mut request = server.recv().expect("request");
                assert!(request.url().starts_with(expected));
                let content_type =
                    Header::from_bytes("Content-Type", "application/json").expect("header");
                if expected == "/chat.postMessage" {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).expect("body");
                    assert!(body.contains("Hello"));
                    assert!(!body.contains("xoxb-secret-fixture"));
                    request
                        .respond(
                            Response::from_string(
                                r#"{"ok":true,"channel":"C123","ts":"1712345678.000100","extra":"raw-provider-data"}"#,
                            )
                            .with_header(content_type),
                        )
                        .expect("respond");
                } else {
                    request
                        .respond(
                            Response::from_string(
                                r#"{"ok":true,"permalink":"https://example.slack.com/archives/C123/p1712345678000100"}"#,
                            )
                            .with_header(content_type),
                        )
                        .expect("respond");
                }
            }
        });
        let executor = SlackActionExecutor {
            api_base: Some(format!("http://127.0.0.1:{port}")),
        };
        let result = executor
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "slack".into(),
                    action_id: "slack.send_message".into(),
                    input: BTreeMap::from([
                        ("conversation".into(), Value::String("C123".into())),
                        ("text".into(), Value::String("Hello".into())),
                    ]),
                },
                &connection(vec!["chat:write", "channels:read"]),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action result");
        responder.join().expect("responder");
        let serialized = serde_json::to_string(&result).expect("serialize");
        assert!(serialized.contains("1712345678.000100"));
        assert!(serialized.contains("permalink"));
        assert!(!serialized.contains("raw-provider-data"));
        assert!(!serialized.contains("xoxb-secret-fixture"));
    }

    #[tokio::test]
    async fn conversation_rate_limit_honors_retry_after() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            let retry_after = Header::from_bytes("Retry-After", "7").expect("header");
            request
                .respond(Response::empty(429).with_header(retry_after))
                .expect("respond");
        });
        let executor = SlackActionExecutor {
            api_base: Some(format!("http://127.0.0.1:{port}")),
        };
        let error = executor
            .list_resources(
                "conversations",
                "conversation",
                "",
                None,
                &connection(vec!["chat:write", "channels:read"]),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect_err("rate limited");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::RateLimited);
        assert_eq!(error.retry_after_seconds, Some(7));
    }

    #[test]
    fn connection_input_rejects_wrong_token_types_and_untrusted_webhook_hosts() {
        assert_eq!(
            validate_private_input(SlackPrivateConnectionInput {
                mode: SlackPrivateConnectionMode::Bot,
                bot_token: "xoxp-user".into(),
                app_token: None,
                webhook_url: None,
                enable_private_channels: false,
                enable_mentions: false,
            })
            .err()
            .expect("wrong token type")
            .code,
            "slack_token_invalid"
        );
        assert_eq!(
            validate_private_input(SlackPrivateConnectionInput {
                mode: SlackPrivateConnectionMode::IncomingWebhook,
                bot_token: String::new(),
                app_token: None,
                webhook_url: Some("https://example.com/services/secret".into()),
                enable_private_channels: false,
                enable_mentions: false,
            })
            .err()
            .expect("untrusted webhook host")
            .code,
            "slack_webhook_invalid"
        );
        assert!(validate_private_input(SlackPrivateConnectionInput {
            mode: SlackPrivateConnectionMode::Bot,
            bot_token: "xoxb-valid-shape".into(),
            app_token: Some("xapp-valid-shape".into()),
            webhook_url: None,
            enable_private_channels: false,
            enable_mentions: true,
        })
        .is_ok());
        assert_eq!(
            validate_private_input(SlackPrivateConnectionInput {
                mode: SlackPrivateConnectionMode::Bot,
                bot_token: "xoxb-valid-shape".into(),
                app_token: None,
                webhook_url: None,
                enable_private_channels: false,
                enable_mentions: true,
            })
            .err()
            .expect("missing app token")
            .code,
            "slack_app_token_required"
        );
    }

    #[test]
    fn slack_errors_map_to_stable_framework_codes() {
        assert_eq!(
            map_slack_action_error(Some("missing_scope")).code,
            ActionErrorCode::ScopeMissing
        );
        assert_eq!(
            map_slack_action_error(Some("token_revoked")).code,
            ActionErrorCode::ProviderUnauthorized
        );
        assert_eq!(
            map_slack_action_error(Some("channel_not_found")).code,
            ActionErrorCode::InvalidInput
        );
    }

    #[test]
    fn validates_slack_message_and_thread_limits() {
        assert!(validate_message(&"x".repeat(SLACK_MESSAGE_HARD_CHARS)).is_ok());
        assert!(validate_message(&"x".repeat(SLACK_MESSAGE_HARD_CHARS + 1)).is_err());
        assert!(validate_thread_ts("1712345678.000100").is_ok());
        assert!(validate_thread_ts("not-a-timestamp").is_err());
    }

    #[test]
    fn ambiguous_post_failures_never_enter_the_automatic_retry_path() {
        let error = ActionError::new(ActionErrorCode::DeliveryUnknown);
        assert!(!error.is_unauthorized());
        assert_eq!(error.code.as_str(), "delivery_unknown");
    }
}
