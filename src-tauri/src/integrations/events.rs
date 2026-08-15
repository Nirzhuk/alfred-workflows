//! Descriptor-driven connected-app events.
//!
//! Adapters execute only in Rust and receive credentials through the same
//! backend-only capability as app actions. Before persistence, every event is
//! normalized, minimized, validated against a descriptor allow-list, bounded,
//! and checked for credential leakage.

use super::actions::{ActionErrorCode, ActionFieldDescriptor, TokenAccessCapability};
use super::models::{AppConnection, ConnectionStatus};
use super::token_store::TokenStore;
use crate::db::{
    AppTriggerCheckpointUpdate, Db, RecordAppEventOutcome, Trigger, DEFAULT_APP_EVENT_PENDING_CAP,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use thiserror::Error;

pub const NORMALIZED_APP_EVENT_SCHEMA_VERSION: u16 = 1;
pub const MAX_APP_EVENT_BYTES: usize = 16 * 1024;
pub const MAX_APP_EVENT_PREVIEW_CHARS: usize = 1_000;
const MAX_EVENT_ATTRIBUTES: usize = 16;
const MAX_ATTRIBUTE_STRING_CHARS: usize = 512;
const MAX_RESOURCE_QUERY_BYTES: usize = 200;
const MAX_RESOURCE_PAGE_TOKEN_BYTES: usize = 512;
const MAX_RESOURCE_ITEMS: usize = 100;
const ADAPTER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppEventDeliveryMode {
    Polling,
    Socket,
    Subscription,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEventDescriptor {
    pub provider_id: String,
    pub event_type: String,
    pub label: String,
    pub description: String,
    pub required_scopes: Vec<String>,
    pub delivery_modes: Vec<AppEventDeliveryMode>,
    pub filter_fields: Vec<ActionFieldDescriptor>,
    pub fetches_resource_content: bool,
    pub descriptor_version: u16,
    pub external_event_id_required: bool,
    pub allowed_attribute_keys: Vec<String>,
    pub poll_interval_seconds: u64,
    pub pending_cap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppTriggerConfig {
    pub provider_id: String,
    pub event_type: String,
    pub connection_id: String,
    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
    pub descriptor_version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedAppEvent {
    pub schema_version: u16,
    pub provider_id: String,
    pub event_type: String,
    pub connection_id: String,
    pub external_event_id: String,
    pub occurred_at: String,
    pub subject: Option<String>,
    pub actor: Option<String>,
    pub resource_url: Option<String>,
    pub preview: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct AppEventBatch {
    pub events: Vec<NormalizedAppEvent>,
    pub cursor: Option<String>,
    pub subscription_id: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AppEventRenewal {
    pub subscription_id: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEventResourceItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEventResourcePage {
    pub items: Vec<AppEventResourceItem>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppEventErrorCode {
    EventNotFound,
    ConnectionRequired,
    ScopeMissing,
    RateLimited,
    ProviderUnauthorized,
    ProviderUnavailable,
    InvalidInput,
    EventTooLarge,
    EventInvalid,
    TimedOut,
    Cancelled,
    QueueFull,
}

impl AppEventErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EventNotFound => "event_not_found",
            Self::ConnectionRequired => "connection_required",
            Self::ScopeMissing => "scope_missing",
            Self::RateLimited => "rate_limited",
            Self::ProviderUnauthorized => "provider_unauthorized",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::InvalidInput => "invalid_input",
            Self::EventTooLarge => "event_too_large",
            Self::EventInvalid => "event_invalid",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::QueueFull => "queue_full",
        }
    }
}

#[derive(Debug, Clone, Error, Serialize)]
#[error("{code:?}")]
#[serde(rename_all = "camelCase")]
pub struct AppEventError {
    pub code: AppEventErrorCode,
    pub retry_after_seconds: Option<u64>,
}

impl AppEventError {
    pub fn new(code: AppEventErrorCode) -> Self {
        Self {
            code,
            retry_after_seconds: None,
        }
    }

    pub fn retry_after(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds.min(86_400));
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub accepted: usize,
    pub duplicates: usize,
    pub rejected: usize,
    pub dropped_overrun: usize,
    pub backpressured: bool,
}

#[derive(Clone, Default)]
pub struct AppEventCancellation {
    cancelled: Arc<AtomicBool>,
}

impl AppEventCancellation {
    pub fn never() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) async fn wait(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
}

pub type AppEventFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AppEventError>> + Send + 'a>>;

pub trait AppEventAdapter: Send + Sync {
    fn poll<'a>(
        &'a self,
        config: &'a AppTriggerConfig,
        connection: &'a AppConnection,
        cursor: Option<&'a str>,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch>;

    /// Socket adapters may keep a provider connection internally and return
    /// the next bounded batch here. The runtime calls this only while at least
    /// one matching trigger is enabled.
    fn connect<'a>(
        &'a self,
        config: &'a AppTriggerConfig,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        self.poll(config, connection, None, tokens, cancellation)
    }

    fn renew<'a>(
        &'a self,
        _config: &'a AppTriggerConfig,
        _connection: &'a AppConnection,
        _subscription_id: Option<&'a str>,
        _tokens: TokenAccessCapability,
        _cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventRenewal> {
        Box::pin(async { Ok(AppEventRenewal::default()) })
    }

    fn disconnect<'a>(
        &'a self,
        _config: &'a AppTriggerConfig,
        _connection: &'a AppConnection,
        _tokens: TokenAccessCapability,
        _cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn list_filter_resources<'a>(
        &'a self,
        _field_key: &'a str,
        _query: &'a str,
        _page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        _tokens: TokenAccessCapability,
        _cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async { Err(AppEventError::new(AppEventErrorCode::InvalidInput)) })
    }

    /// Drop provider connections after trigger edits, connection revocation,
    /// or runtime reload. Implementations must not block here.
    fn reset(&self) {}
}

struct RegisteredAppEvent {
    descriptor: AppEventDescriptor,
    adapter: Arc<dyn AppEventAdapter>,
}

#[derive(Default)]
pub struct AppEventRegistry {
    events: RwLock<HashMap<String, Arc<RegisteredAppEvent>>>,
    socket_syncs: Mutex<HashSet<String>>,
}

impl AppEventRegistry {
    pub fn register(
        &self,
        descriptor: AppEventDescriptor,
        adapter: Arc<dyn AppEventAdapter>,
    ) -> Result<(), AppEventError> {
        validate_descriptor(&descriptor)?;
        let key = event_key(&descriptor.provider_id, &descriptor.event_type);
        let mut events = self
            .events
            .write()
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
        if events.contains_key(&key) {
            return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
        }
        events.insert(
            key,
            Arc::new(RegisteredAppEvent {
                descriptor,
                adapter,
            }),
        );
        Ok(())
    }

    pub fn descriptors(&self, provider_id: Option<&str>) -> Vec<AppEventDescriptor> {
        let Ok(events) = self.events.read() else {
            return Vec::new();
        };
        let mut descriptors = events
            .values()
            .filter(|item| {
                provider_id.is_none_or(|provider| item.descriptor.provider_id == provider)
            })
            .map(|item| item.descriptor.clone())
            .collect::<Vec<_>>();
        descriptors.sort_by(|left, right| {
            (&left.provider_id, &left.event_type).cmp(&(&right.provider_id, &right.event_type))
        });
        descriptors
    }

    pub fn descriptor(&self, provider_id: &str, event_type: &str) -> Option<AppEventDescriptor> {
        self.registration(provider_id, event_type)
            .ok()
            .map(|item| item.descriptor.clone())
    }

    pub fn reset(&self) {
        if let Ok(events) = self.events.read() {
            for registration in events.values() {
                registration.adapter.reset();
            }
        }
        if let Ok(mut active) = self.socket_syncs.lock() {
            active.clear();
        }
    }

    pub fn validate_trigger(
        &self,
        db: &Db,
        config: &AppTriggerConfig,
    ) -> Result<(), AppEventError> {
        let registration = self.registration(&config.provider_id, &config.event_type)?;
        validate_trigger_config(config, &registration.descriptor)?;
        let _ = load_compatible_connection(db, config, &registration.descriptor)?;
        Ok(())
    }

    pub async fn sync_trigger(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        trigger: &Trigger,
    ) -> Result<SyncReport, AppEventError> {
        self.sync_trigger_cancellable(db, store, trigger, AppEventCancellation::never())
            .await
    }

    pub async fn sync_trigger_cancellable(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        trigger: &Trigger,
        cancellation: AppEventCancellation,
    ) -> Result<SyncReport, AppEventError> {
        if cancellation.is_cancelled() {
            return Err(AppEventError::new(AppEventErrorCode::Cancelled));
        }
        let config: AppTriggerConfig = serde_json::from_value(trigger.config.clone())
            .map_err(|_| AppEventError::new(AppEventErrorCode::InvalidInput))?;
        let registration = self.registration(&config.provider_id, &config.event_type)?;
        validate_trigger_config(&config, &registration.descriptor)?;
        let connection = load_compatible_connection(db, &config, &registration.descriptor)?;
        let replayable = registration
            .descriptor
            .delivery_modes
            .contains(&AppEventDeliveryMode::Polling);
        let _socket_guard = if replayable {
            None
        } else {
            let key =
                event_key(&config.provider_id, &config.event_type) + "\0" + &config.connection_id;
            let Some(guard) = SocketSyncGuard::try_acquire(&self.socket_syncs, key) else {
                return Ok(SyncReport::default());
            };
            Some(guard)
        };
        let tokens = load_tokens(store, &connection).await?;
        let state = db
            .get_app_trigger_state(&trigger.id)
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;

        let mut subscription_id = state
            .as_ref()
            .and_then(|current| current.subscription_id.clone());
        let mut expires_at = state
            .as_ref()
            .and_then(|current| current.expires_at.clone());
        if renewal_due(expires_at.as_deref()) {
            let renewal_future = tokio::time::timeout(
                ADAPTER_TIMEOUT,
                registration.adapter.renew(
                    &config,
                    &connection,
                    subscription_id.as_deref(),
                    tokens.clone(),
                    cancellation.clone(),
                ),
            );
            let renewal = tokio::select! {
                result = renewal_future => result
                    .map_err(|_| AppEventError::new(AppEventErrorCode::TimedOut))?,
                _ = cancellation.wait() => {
                    return Err(AppEventError::new(AppEventErrorCode::Cancelled));
                }
            }?;
            if renewal.subscription_id.is_some() {
                subscription_id = renewal.subscription_id;
            }
            if renewal.expires_at.is_some() {
                expires_at = renewal.expires_at;
            }
        }

        let future = if replayable {
            registration.adapter.poll(
                &config,
                &connection,
                state.as_ref().and_then(|current| current.cursor.as_deref()),
                tokens.clone(),
                cancellation.clone(),
            )
        } else {
            registration
                .adapter
                .connect(&config, &connection, tokens.clone(), cancellation.clone())
        };
        let batch_result = tokio::select! {
            result = tokio::time::timeout(ADAPTER_TIMEOUT, future) => result
                .map_err(|_| AppEventError::new(AppEventErrorCode::TimedOut))?,
            _ = cancellation.wait() => {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
        };
        let batch = match batch_result {
            Ok(batch) => batch,
            Err(error) => {
                if error.code == AppEventErrorCode::ProviderUnauthorized {
                    let _ = db.set_app_connection_refresh_state(
                        &connection.id,
                        ConnectionStatus::Error,
                        connection.expires_at.as_deref(),
                        Some(error.code.as_str()),
                    );
                }
                return Err(error);
            }
        };
        if cancellation.is_cancelled() {
            return Err(AppEventError::new(AppEventErrorCode::Cancelled));
        }
        if replayable {
            return process_batch_for_trigger(
                db,
                &registration.descriptor,
                trigger,
                &config,
                &tokens,
                batch,
                true,
                state.and_then(|value| value.cursor),
                subscription_id,
                expires_at,
            );
        }

        // One provider socket feeds every matching enabled trigger. Persist to
        // each trigger before returning to the socket so a burst cannot be
        // lost in an in-memory broadcast queue.
        let targets = db
            .list_enabled_triggers(Some("app"))
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
        let mut requested_report = SyncReport::default();
        for target in targets {
            let Ok(target_config) =
                serde_json::from_value::<AppTriggerConfig>(target.config.clone())
            else {
                continue;
            };
            if target_config.provider_id != config.provider_id
                || target_config.event_type != config.event_type
                || target_config.connection_id != config.connection_id
            {
                continue;
            }
            let report = process_batch_for_trigger(
                db,
                &registration.descriptor,
                &target,
                &target_config,
                &tokens,
                batch.clone(),
                false,
                None,
                batch
                    .subscription_id
                    .clone()
                    .or_else(|| Some("socket_mode".into())),
                batch.expires_at.clone(),
            )?;
            if target.id == trigger.id {
                requested_report = report;
            }
        }
        Ok(requested_report)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_resources(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        connection_id: &str,
        provider_id: &str,
        event_type: &str,
        field_key: &str,
        query: &str,
        page_token: Option<&str>,
    ) -> Result<AppEventResourcePage, AppEventError> {
        if query.len() > MAX_RESOURCE_QUERY_BYTES
            || page_token.is_some_and(|token| token.len() > MAX_RESOURCE_PAGE_TOKEN_BYTES)
        {
            return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
        }
        let registration = self.registration(provider_id, event_type)?;
        let field = registration
            .descriptor
            .filter_fields
            .iter()
            .find(|field| field.key == field_key && field.option_source.is_some())
            .ok_or_else(|| AppEventError::new(AppEventErrorCode::InvalidInput))?;
        let config = AppTriggerConfig {
            provider_id: provider_id.into(),
            event_type: event_type.into(),
            connection_id: connection_id.into(),
            filters: BTreeMap::new(),
            descriptor_version: registration.descriptor.descriptor_version,
        };
        let connection = load_compatible_connection(db, &config, &registration.descriptor)?;
        let tokens = load_tokens(store, &connection).await?;
        let page = tokio::time::timeout(
            ADAPTER_TIMEOUT,
            registration.adapter.list_filter_resources(
                &field.key,
                query,
                page_token,
                &connection,
                tokens.clone(),
                AppEventCancellation::never(),
            ),
        )
        .await
        .map_err(|_| AppEventError::new(AppEventErrorCode::TimedOut))??;
        if page.items.len() > MAX_RESOURCE_ITEMS
            || page
                .items
                .iter()
                .any(|item| item.id.len() > 512 || item.label.chars().count() > 256)
        {
            return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
        }
        let serialized = serde_json::to_string(&page)
            .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
        if tokens.contains_secret(&serialized) {
            return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
        }
        Ok(page)
    }

    fn registration(
        &self,
        provider_id: &str,
        event_type: &str,
    ) -> Result<Arc<RegisteredAppEvent>, AppEventError> {
        self.events
            .read()
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?
            .get(&event_key(provider_id, event_type))
            .cloned()
            .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventNotFound))
    }
}

struct SocketSyncGuard<'a> {
    active: &'a Mutex<HashSet<String>>,
    key: String,
}

impl<'a> SocketSyncGuard<'a> {
    fn try_acquire(active: &'a Mutex<HashSet<String>>, key: String) -> Option<Self> {
        let mut values = active.lock().ok()?;
        if !values.insert(key.clone()) {
            return None;
        }
        drop(values);
        Some(Self { active, key })
    }
}

impl Drop for SocketSyncGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.key);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_batch_for_trigger(
    db: &Db,
    descriptor: &AppEventDescriptor,
    trigger: &Trigger,
    config: &AppTriggerConfig,
    tokens: &TokenAccessCapability,
    batch: AppEventBatch,
    replayable: bool,
    prior_cursor: Option<String>,
    prior_subscription_id: Option<String>,
    prior_expires_at: Option<String>,
) -> Result<SyncReport, AppEventError> {
    let mut report = SyncReport::default();
    for candidate in &batch.events {
        let fallback_id = invalid_event_id(candidate);
        let normalized = match normalize_event(candidate.clone(), descriptor, config) {
            Ok(event) => event,
            Err(_) => {
                let _ = db.record_rejected_app_event(
                    &trigger.id,
                    &fallback_id,
                    AppEventErrorCode::EventInvalid.as_str(),
                );
                report.rejected += 1;
                continue;
            }
        };
        if !event_matches_filters(&normalized, config) {
            continue;
        }
        let serialized = serde_json::to_string(&normalized)
            .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
        if tokens.contains_secret(&serialized) {
            let _ = db.record_rejected_app_event(
                &trigger.id,
                &fallback_id,
                AppEventErrorCode::EventInvalid.as_str(),
            );
            report.rejected += 1;
            continue;
        }
        match db
            .record_app_event(
                &trigger.id,
                &normalized.external_event_id,
                &serialized,
                replayable,
                descriptor
                    .pending_cap
                    .max(1)
                    .min(DEFAULT_APP_EVENT_PENDING_CAP * 10),
            )
            .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?
        {
            RecordAppEventOutcome::Queued => report.accepted += 1,
            RecordAppEventOutcome::Duplicate => report.duplicates += 1,
            RecordAppEventOutcome::Backpressure => report.backpressured = true,
            RecordAppEventOutcome::DroppedOverrun => report.dropped_overrun += 1,
        }
    }

    // A replayable cursor is committed only after every event in its batch was
    // durably accepted or recognized as an existing receipt. Socket health is
    // checkpointed for every trigger fed by the shared connection.
    if !report.backpressured {
        db.save_app_trigger_checkpoint(
            &trigger.id,
            &AppTriggerCheckpointUpdate {
                cursor: batch.cursor.or(prior_cursor),
                subscription_id: batch.subscription_id.or(prior_subscription_id),
                expires_at: batch.expires_at.or(prior_expires_at),
                polled_at: Utc::now().to_rfc3339(),
            },
        )
        .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?;
    }
    Ok(report)
}

fn event_matches_filters(event: &NormalizedAppEvent, config: &AppTriggerConfig) -> bool {
    config.filters.iter().all(|(key, expected)| {
        if value_is_empty(expected) {
            return true;
        }
        event
            .attributes
            .get(key)
            .is_some_and(|actual| actual == expected)
    })
}

fn event_key(provider_id: &str, event_type: &str) -> String {
    format!("{provider_id}\u{0}{event_type}")
}

fn validate_descriptor(descriptor: &AppEventDescriptor) -> Result<(), AppEventError> {
    if descriptor.provider_id.trim().is_empty()
        || descriptor.event_type.trim().is_empty()
        || descriptor.label.trim().is_empty()
        || descriptor.descriptor_version == 0
        || descriptor.delivery_modes.is_empty()
        || descriptor.poll_interval_seconds > 86_400
        || descriptor.pending_cap == 0
    {
        return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
    }
    let mut fields = HashSet::new();
    if descriptor.filter_fields.iter().any(|field| {
        field.secret
            || field.key.trim().is_empty()
            || !fields.insert(field.key.clone())
            || secret_like_key(&field.key)
    }) {
        return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
    }
    let mut attributes = HashSet::new();
    if descriptor
        .allowed_attribute_keys
        .iter()
        .any(|key| key.trim().is_empty() || secret_like_key(key) || !attributes.insert(key.clone()))
    {
        return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
    }
    Ok(())
}

fn validate_trigger_config(
    config: &AppTriggerConfig,
    descriptor: &AppEventDescriptor,
) -> Result<(), AppEventError> {
    if config.provider_id != descriptor.provider_id
        || config.event_type != descriptor.event_type
        || config.connection_id.trim().is_empty()
        || config.descriptor_version > descriptor.descriptor_version
    {
        return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
    }
    let known = descriptor
        .filter_fields
        .iter()
        .map(|field| (field.key.as_str(), field))
        .collect::<HashMap<_, _>>();
    if config
        .filters
        .keys()
        .any(|key| !known.contains_key(key.as_str()))
    {
        return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
    }
    for field in &descriptor.filter_fields {
        let value = config.filters.get(&field.key).or(field.default.as_ref());
        if field.required && value.is_none_or(value_is_empty) {
            return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
        }
        if let Some(value) = value {
            let valid = match field.kind {
                super::actions::ActionFieldKind::Boolean => value.is_boolean(),
                _ => value.as_str().is_some_and(|text| text.len() <= 2_048),
            };
            if !valid {
                return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
            }
        }
    }
    Ok(())
}

fn value_is_empty(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(|text| text.trim().is_empty())
}

fn load_compatible_connection(
    db: &Db,
    config: &AppTriggerConfig,
    descriptor: &AppEventDescriptor,
) -> Result<AppConnection, AppEventError> {
    let connection = db
        .get_app_connection(&config.connection_id)
        .map_err(|_| AppEventError::new(AppEventErrorCode::ProviderUnavailable))?
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::ConnectionRequired))?;
    if connection.provider_id != descriptor.provider_id
        || connection.status != ConnectionStatus::Connected
    {
        return Err(AppEventError::new(AppEventErrorCode::ConnectionRequired));
    }
    if descriptor
        .required_scopes
        .iter()
        .any(|scope| !connection.scopes.contains(scope))
    {
        return Err(AppEventError::new(AppEventErrorCode::ScopeMissing));
    }
    Ok(connection)
}

async fn load_tokens(
    store: Arc<dyn TokenStore>,
    connection: &AppConnection,
) -> Result<TokenAccessCapability, AppEventError> {
    TokenAccessCapability::load(store, connection.credential_ref.clone())
        .await
        .map_err(|error| match error.code {
            ActionErrorCode::ConnectionRequired => {
                AppEventError::new(AppEventErrorCode::ConnectionRequired)
            }
            _ => AppEventError::new(AppEventErrorCode::ProviderUnavailable),
        })
}

fn normalize_event(
    mut event: NormalizedAppEvent,
    descriptor: &AppEventDescriptor,
    config: &AppTriggerConfig,
) -> Result<NormalizedAppEvent, AppEventError> {
    if event.schema_version != NORMALIZED_APP_EVENT_SCHEMA_VERSION
        || event.provider_id != descriptor.provider_id
        || event.event_type != descriptor.event_type
        || event.connection_id != config.connection_id
        || DateTime::parse_from_rfc3339(&event.occurred_at).is_err()
        || event
            .subject
            .as_ref()
            .is_some_and(|text| text.chars().count() > 512)
        || event
            .actor
            .as_ref()
            .is_some_and(|text| text.chars().count() > 256)
    {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    if event.external_event_id.trim().is_empty() {
        if descriptor.external_event_id_required {
            return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
        }
        event.external_event_id = derive_event_id(&event);
    }
    if event.external_event_id.len() > 512 {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    if let Some(url) = event.resource_url.as_deref() {
        let parsed = url::Url::parse(url)
            .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
        if parsed.scheme() != "https" || url.len() > 2_048 {
            return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
        }
    }
    if let Some(preview) = event.preview.as_mut() {
        if preview.chars().count() > MAX_APP_EVENT_PREVIEW_CHARS {
            *preview = format!(
                "{}\n… (truncated)",
                preview
                    .chars()
                    .take(MAX_APP_EVENT_PREVIEW_CHARS)
                    .collect::<String>()
            );
        }
    }
    let allowlist = descriptor
        .allowed_attribute_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if event.attributes.len() > MAX_EVENT_ATTRIBUTES
        || event.attributes.iter().any(|(key, value)| {
            !allowlist.contains(key.as_str())
                || secret_like_key(key)
                || !safe_attribute_value(value)
        })
    {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    let serialized = serde_json::to_vec(&event)
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    if serialized.len() > MAX_APP_EVENT_BYTES {
        return Err(AppEventError::new(AppEventErrorCode::EventTooLarge));
    }
    Ok(event)
}

fn safe_attribute_value(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(text) => text.chars().count() <= MAX_ATTRIBUTE_STRING_CHARS,
        _ => false,
    }
}

fn secret_like_key(key: &str) -> bool {
    let compact = key.to_ascii_lowercase().replace(['-', '_'], "");
    [
        "token",
        "secret",
        "authorization",
        "signature",
        "password",
        "cookie",
        "rawbody",
        "headers",
        "attachment",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

fn derive_event_id(event: &NormalizedAppEvent) -> String {
    let stable = serde_json::json!({
        "providerId": event.provider_id,
        "eventType": event.event_type,
        "occurredAt": event.occurred_at,
        "subject": event.subject,
        "actor": event.actor,
        "resourceUrl": event.resource_url,
        "attributes": event.attributes,
    });
    let digest = Sha256::digest(serde_json::to_vec(&stable).unwrap_or_default());
    format!("derived-{digest:x}")
}

fn invalid_event_id(event: &NormalizedAppEvent) -> String {
    if !event.external_event_id.trim().is_empty() && event.external_event_id.len() <= 512 {
        return event.external_event_id.clone();
    }
    let digest = Sha256::digest(serde_json::to_vec(event).unwrap_or_default());
    format!("invalid-{digest:x}")
}

fn renewal_due(expires_at: Option<&str>) -> bool {
    expires_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|expiry| expiry.with_timezone(&Utc) <= Utc::now() + ChronoDuration::minutes(5))
        .unwrap_or(false)
}

pub fn next_retry_at(error: &AppEventError, retry_count: u32) -> String {
    use rand::Rng;

    let seconds = error.retry_after_seconds.unwrap_or_else(|| {
        let exponent = retry_count.min(8);
        let base = 5u64.saturating_mul(1u64 << exponent).min(900);
        let jitter = rand::thread_rng().gen_range(0..=base.saturating_div(4));
        base.saturating_add(jitter).min(900)
    });
    (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, UpsertTriggerInput};
    use crate::integrations::actions::{ActionFieldKind, ActionOption};
    use crate::integrations::models::{canonical_identity_key, UpsertAppConnection};
    use crate::integrations::token_store::{CredentialEnvelope, InMemoryTokenStore};
    use serde_json::json;
    use std::sync::Mutex;

    struct FakeAdapter {
        batches: Mutex<Vec<AppEventBatch>>,
    }

    impl AppEventAdapter for FakeAdapter {
        fn poll<'a>(
            &'a self,
            _config: &'a AppTriggerConfig,
            _connection: &'a AppConnection,
            _cursor: Option<&'a str>,
            _tokens: TokenAccessCapability,
            _cancellation: AppEventCancellation,
        ) -> AppEventFuture<'a, AppEventBatch> {
            Box::pin(async move { Ok(self.batches.lock().expect("batches").remove(0)) })
        }
    }

    fn descriptor() -> AppEventDescriptor {
        AppEventDescriptor {
            provider_id: "slack".into(),
            event_type: "slack.app_mention".into(),
            label: "App mention".into(),
            description: "A safe mention preview".into(),
            required_scopes: vec!["app_mentions:read".into()],
            delivery_modes: vec![AppEventDeliveryMode::Polling],
            filter_fields: vec![ActionFieldDescriptor {
                key: "channelId".into(),
                label: "Channel".into(),
                description: String::new(),
                kind: ActionFieldKind::ResourceSelector,
                required: false,
                default: None,
                secret: false,
                option_source: Some("conversations".into()),
                options: Vec::<ActionOption>::new(),
                supports_interpolation: false,
            }],
            fetches_resource_content: false,
            descriptor_version: 1,
            external_event_id_required: true,
            allowed_attribute_keys: vec!["channelId".into(), "userId".into()],
            poll_interval_seconds: 15,
            pending_cap: 10,
        }
    }

    fn event(id: &str, preview: &str) -> NormalizedAppEvent {
        NormalizedAppEvent {
            schema_version: 1,
            provider_id: "slack".into(),
            event_type: "slack.app_mention".into(),
            connection_id: "connection".into(),
            external_event_id: id.into(),
            occurred_at: Utc::now().to_rfc3339(),
            subject: Some("Mention".into()),
            actor: None,
            resource_url: Some("https://example.slack.com/archives/C1/p1".into()),
            preview: Some(preview.into()),
            attributes: BTreeMap::from([("channelId".into(), json!("C1"))]),
        }
    }

    fn fixture(batch: AppEventBatch) -> (Db, Arc<InMemoryTokenStore>, Trigger, AppEventRegistry) {
        fixture_with_descriptor(batch, descriptor())
    }

    fn fixture_with_descriptor(
        batch: AppEventBatch,
        event_descriptor: AppEventDescriptor,
    ) -> (Db, Arc<InMemoryTokenStore>, Trigger, AppEventRegistry) {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Events".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("workflow");
        let connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: Some("Workspace".into()),
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "private_bot".into(),
                identity_key: canonical_identity_key("slack", "private_bot", &["team"]),
                scopes: vec!["app_mentions:read".into()],
                provider_metadata: BTreeMap::new(),
                expires_at: None,
                credential_ref: "credential".into(),
            })
            .expect("connection");
        let config = AppTriggerConfig {
            provider_id: "slack".into(),
            event_type: "slack.app_mention".into(),
            connection_id: connection.id.clone(),
            filters: BTreeMap::new(),
            descriptor_version: 1,
        };
        let mut batch = batch;
        for event in &mut batch.events {
            event.connection_id = connection.id.clone();
        }
        let trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: workflow.id,
                source: "app".into(),
                label: "Mention".into(),
                config: serde_json::to_value(config).expect("config"),
                enabled: true,
            })
            .expect("trigger");
        let store = Arc::new(InMemoryTokenStore::default());
        store
            .put(
                "credential",
                &CredentialEnvelope::new("fixture-access-token".into()),
            )
            .expect("credential");
        let registry = AppEventRegistry::default();
        registry
            .register(
                event_descriptor,
                Arc::new(FakeAdapter {
                    batches: Mutex::new(vec![batch]),
                }),
            )
            .expect("register");
        (db, store, trigger, registry)
    }

    #[test]
    fn rejects_duplicate_descriptors_and_secret_filter_fields() {
        let registry = AppEventRegistry::default();
        let adapter = Arc::new(FakeAdapter {
            batches: Mutex::new(vec![]),
        });
        registry
            .register(descriptor(), adapter.clone())
            .expect("first");
        assert_eq!(
            registry.register(descriptor(), adapter).unwrap_err().code,
            AppEventErrorCode::InvalidInput
        );
        let mut invalid = descriptor();
        invalid.filter_fields[0].secret = true;
        assert_eq!(
            validate_descriptor(&invalid).unwrap_err().code,
            AppEventErrorCode::InvalidInput
        );
    }

    #[test]
    fn normalization_bounds_preview_and_rejects_raw_secret_fields() {
        let config = AppTriggerConfig {
            provider_id: "slack".into(),
            event_type: "slack.app_mention".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::new(),
            descriptor_version: 1,
        };
        let normalized = normalize_event(
            event("evt", &"x".repeat(MAX_APP_EVENT_PREVIEW_CHARS + 20)),
            &descriptor(),
            &config,
        )
        .expect("normalized");
        assert!(normalized.preview.expect("preview").contains("truncated"));
        let mut unsafe_event = event("unsafe", "preview");
        unsafe_event
            .attributes
            .insert("authorizationHeader".into(), json!("Bearer fixture"));
        assert_eq!(
            normalize_event(unsafe_event, &descriptor(), &config)
                .unwrap_err()
                .code,
            AppEventErrorCode::EventInvalid
        );
    }

    #[tokio::test]
    async fn event_receipt_is_deduplicated_and_cursor_advances_after_durable_recording() {
        let batch = AppEventBatch {
            events: vec![event("evt-1", "safe preview"), event("evt-1", "duplicate")],
            cursor: Some("cursor-2".into()),
            ..Default::default()
        };
        let (db, store, trigger, registry) = fixture(batch);
        let report = registry
            .sync_trigger(&db, store, &trigger)
            .await
            .expect("sync");
        assert_eq!(report.accepted, 1);
        assert_eq!(report.duplicates, 1);
        assert_eq!(
            db.get_app_trigger_state(&trigger.id)
                .expect("state")
                .expect("state row")
                .cursor
                .as_deref(),
            Some("cursor-2")
        );
    }

    #[tokio::test]
    async fn one_socket_batch_fans_out_with_filters_and_durable_overrun() {
        let mut second_channel = event("evt-c2", "channel two");
        second_channel
            .attributes
            .insert("channelId".into(), json!("C2"));
        let mut descriptor = descriptor();
        descriptor.delivery_modes = vec![AppEventDeliveryMode::Socket];
        descriptor.pending_cap = 1;
        let batch = AppEventBatch {
            events: vec![
                event("evt-c1-a", "first"),
                event("evt-c1-b", "overrun"),
                second_channel,
            ],
            subscription_id: Some("socket_mode".into()),
            ..Default::default()
        };
        let (db, store, mut first_trigger, registry) = fixture_with_descriptor(batch, descriptor);
        let mut first_config: AppTriggerConfig =
            serde_json::from_value(first_trigger.config.clone()).expect("first config");
        first_config.filters.insert("channelId".into(), json!("C1"));
        first_trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: Some(first_trigger.id.clone()),
                workflow_id: first_trigger.workflow_id.clone(),
                source: "app".into(),
                label: "Channel one".into(),
                config: serde_json::to_value(&first_config).expect("first config JSON"),
                enabled: true,
            })
            .expect("update first trigger");
        let second_workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Second channel".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("second workflow");
        let mut second_config = first_config.clone();
        second_config
            .filters
            .insert("channelId".into(), json!("C2"));
        let second_trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: second_workflow.id,
                source: "app".into(),
                label: "Channel two".into(),
                config: serde_json::to_value(second_config).expect("second config JSON"),
                enabled: true,
            })
            .expect("second trigger");

        let report = registry
            .sync_trigger(&db, store, &first_trigger)
            .await
            .expect("socket sync");
        assert_eq!(report.accepted, 1);
        assert_eq!(report.dropped_overrun, 1);
        let (first_queued, first_dropped, second_queued): (i64, i64, i64) = db
            .with_conn(|conn| {
                Ok((
                    conn.query_row(
                        "SELECT COUNT(*) FROM app_event_queue WHERE trigger_id = ?1",
                        rusqlite::params![first_trigger.id],
                        |row| row.get(0),
                    )?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM app_event_receipts WHERE trigger_id = ?1 AND disposition = 'dropped_overrun'",
                        rusqlite::params![first_trigger.id],
                        |row| row.get(0),
                    )?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM app_event_queue WHERE trigger_id = ?1",
                        rusqlite::params![second_trigger.id],
                        |row| row.get(0),
                    )?,
                ))
            })
            .expect("delivery state");
        assert_eq!((first_queued, first_dropped, second_queued), (1, 1, 1));
        assert!(db
            .get_app_trigger_state(&second_trigger.id)
            .expect("second state")
            .expect("second state row")
            .subscription_id
            .is_some());
    }

    #[tokio::test]
    async fn credential_fixture_is_rejected_before_persistence() {
        let batch = AppEventBatch {
            events: vec![event("evt-secret", "fixture-access-token")],
            cursor: Some("cursor".into()),
            ..Default::default()
        };
        let (db, store, trigger, registry) = fixture(batch);
        let report = registry
            .sync_trigger(&db, store, &trigger)
            .await
            .expect("sync");
        assert_eq!(report.rejected, 1);
        let payloads: String = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COALESCE(GROUP_CONCAT(normalized_event_json), '') FROM app_event_queue",
                    [],
                    |row| row.get(0),
                )
                .map_err(crate::db::DbError::from)
            })
            .expect("payloads");
        assert!(!payloads.contains("fixture-access-token"));
    }
}
