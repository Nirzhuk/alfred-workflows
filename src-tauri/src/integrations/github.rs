//! GitHub connected-app provider.
//!
//! Authentication uses a GitHub App user access token obtained through the
//! OAuth device flow. The configured client ID is public; no GitHub App client
//! secret or private key is compiled into Alfred. Access remains the
//! intersection of the user's permissions, the GitHub App permissions, and the
//! repositories selected when the app was installed.

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
use super::refresh::{ProviderRefreshError, RefreshFuture, RefreshHandler};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_AUTH_BASE: &str = "https://github.com/login";
const GITHUB_API_VERSION: &str = "2026-03-10";
const GITHUB_USER_AGENT: &str = "Alfred-Desktop";
const GITHUB_RESPONSE_LIMIT: usize = 512 * 1024;
const GITHUB_ERROR_HINT_LIMIT: usize = 16 * 1024;
const GITHUB_ACTION_MARKER: &str = "<!-- alfred-connected-app -->";
const GITHUB_REFRESH_EXPIRY_FIELD: &str = "refresh_expires_at";
const MAX_DEVICE_SESSIONS: usize = 8;
const MAX_DEVICE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_TEXT_CHARS: usize = 32 * 1024;
const MAX_CONTEXT_CHARS: usize = 4_000;

pub fn is_configured() -> bool {
    option_env!("ALFRED_GITHUB_APP_CLIENT_ID").is_some_and(|value| !value.trim().is_empty())
}

pub fn register(
    actions: &ActionRegistry,
    events: &AppEventRegistry,
    service: Arc<GitHubService>,
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

pub struct GitHubService {
    client_id: Option<String>,
    installation_url: Option<String>,
    auth_base: String,
    api_base: String,
    sessions: Mutex<HashMap<String, DeviceSession>>,
}

impl Default for GitHubService {
    fn default() -> Self {
        Self {
            client_id: option_env!("ALFRED_GITHUB_APP_CLIENT_ID")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            installation_url: option_env!("ALFRED_GITHUB_APP_INSTALL_URL")
                .map(str::trim)
                .filter(|value| valid_github_install_url(value))
                .map(str::to_owned),
            auth_base: GITHUB_AUTH_BASE.into(),
            api_base: GITHUB_API_BASE.into(),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl GitHubService {
    pub fn refresh_handler(&self) -> Arc<dyn RefreshHandler> {
        Arc::new(GitHubRefreshHandler {
            client_id: self.client_id.clone(),
            auth_base: self.auth_base.clone(),
        })
    }

    pub async fn prepare_device_authorization(
        &self,
    ) -> Result<GitHubDeviceAuthorization, IntegrationCommandError> {
        self.remove_expired_sessions();
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            command_error(
                "github_not_configured",
                "This build does not include the public GitHub App client ID.",
                false,
            )
        })?;
        if self
            .sessions
            .lock()
            .map_err(|_| github_state_error())?
            .len()
            >= MAX_DEVICE_SESSIONS
        {
            return Err(command_error(
                "github_pairing_busy",
                "Too many GitHub authorization attempts are active. Close another attempt and try again.",
                true,
            ));
        }
        let response: DeviceCodeResponse = auth_post_form(
            &format!("{}/device/code", self.auth_base),
            &[("client_id", client_id)],
        )
        .await
        .map_err(map_auth_transport_error)?;
        if response.device_code.trim().is_empty()
            || response.user_code.trim().is_empty()
            || !valid_device_verification_uri(&response.verification_uri)
            || response.expires_in == 0
        {
            return Err(github_invalid_response());
        }
        let ttl = Duration::from_secs(response.expires_in).min(MAX_DEVICE_TTL);
        let interval = response.interval.unwrap_or(5).clamp(1, 60);
        let session_id = Uuid::new_v4().to_string();
        let session = DeviceSession {
            device_code: response.device_code,
            expires_at: Instant::now() + ttl,
            next_poll_at: Instant::now(),
            interval_seconds: interval,
        };
        self.sessions
            .lock()
            .map_err(|_| github_state_error())?
            .insert(session_id.clone(), session);
        Ok(GitHubDeviceAuthorization {
            pairing_session_id: session_id,
            user_code: response.user_code,
            verification_uri: response.verification_uri,
            installation_url: self.installation_url.clone(),
            expires_at: (Utc::now() + ChronoDuration::seconds(ttl.as_secs() as i64)).to_rfc3339(),
            interval_seconds: interval,
        })
    }

    pub async fn poll_device_authorization(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        pairing_session_id: &str,
    ) -> Result<GitHubDevicePollResult, IntegrationCommandError> {
        let mut session = self
            .sessions
            .lock()
            .map_err(|_| github_state_error())?
            .remove(pairing_session_id)
            .ok_or_else(github_pairing_expired)?;
        if Instant::now() >= session.expires_at {
            return Err(github_pairing_expired());
        }
        if Instant::now() < session.next_poll_at {
            let retry_after = session
                .next_poll_at
                .saturating_duration_since(Instant::now())
                .as_secs()
                .max(1);
            self.reinsert_session(pairing_session_id, session)?;
            return Ok(GitHubDevicePollResult::Pending {
                retry_after_seconds: retry_after,
            });
        }
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            command_error(
                "github_not_configured",
                "This build does not include the public GitHub App client ID.",
                false,
            )
        })?;
        let response = auth_post_form(
            &format!("{}/oauth/access_token", self.auth_base),
            &[
                ("client_id", client_id),
                ("device_code", session.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )
        .await;
        let mut response: DeviceTokenResponse = match response {
            Ok(response) => response,
            Err(error) => {
                session.next_poll_at =
                    Instant::now() + Duration::from_secs(session.interval_seconds);
                self.reinsert_session(pairing_session_id, session)?;
                return Err(map_auth_transport_error(error));
            }
        };
        if let Some(access_token) = response.access_token.take() {
            if !response
                .token_type
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
                || access_token.trim().is_empty()
            {
                return Err(github_invalid_response());
            }
            let connection = self
                .save_authorized_connection(db, store, access_token, response)
                .await?;
            return Ok(GitHubDevicePollResult::Connected { connection });
        }
        match response.error.as_deref() {
            Some("authorization_pending") => {
                session.next_poll_at =
                    Instant::now() + Duration::from_secs(session.interval_seconds);
                let retry_after = session.interval_seconds;
                self.reinsert_session(pairing_session_id, session)?;
                Ok(GitHubDevicePollResult::Pending {
                    retry_after_seconds: retry_after,
                })
            }
            Some("slow_down") => {
                session.interval_seconds = (session.interval_seconds + 5).min(60);
                session.next_poll_at =
                    Instant::now() + Duration::from_secs(session.interval_seconds);
                let retry_after = session.interval_seconds;
                self.reinsert_session(pairing_session_id, session)?;
                Ok(GitHubDevicePollResult::Pending {
                    retry_after_seconds: retry_after,
                })
            }
            Some("expired_token") => Err(github_pairing_expired()),
            Some("access_denied") => Err(command_error(
                "github_authorization_denied",
                "GitHub authorization was denied. Start a new connection attempt when ready.",
                false,
            )),
            _ => Err(github_invalid_response()),
        }
    }

    pub fn cancel_device_authorization(&self, pairing_session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(pairing_session_id);
        }
    }

    fn reinsert_session(
        &self,
        pairing_session_id: &str,
        session: DeviceSession,
    ) -> Result<(), IntegrationCommandError> {
        self.sessions
            .lock()
            .map_err(|_| github_state_error())?
            .insert(pairing_session_id.to_owned(), session);
        Ok(())
    }

    fn remove_expired_sessions(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            let now = Instant::now();
            sessions.retain(|_, session| session.expires_at > now);
        }
    }

    async fn save_authorized_connection(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        access_token: String,
        token_response: DeviceTokenResponse,
    ) -> Result<AppConnectionDto, IntegrationCommandError> {
        let access_token = Zeroizing::new(access_token);
        let user: GitHubUser = github_get_json(&self.api_base, "/user", access_token.as_str(), &[])
            .await
            .map_err(map_connect_action_error)?;
        if user.id == 0 || !valid_login(&user.login) {
            return Err(command_error(
                "github_identity_invalid",
                "GitHub did not return a valid user identity.",
                false,
            ));
        }
        let installations = list_all_installations(&self.api_base, access_token.as_str())
            .await
            .map_err(map_connect_action_error)?;
        let active = installations
            .into_iter()
            .filter(|installation| installation.suspended_at.is_none())
            .collect::<Vec<_>>();
        if active.is_empty() {
            return Err(command_error(
                "github_installation_required",
                "Install the Alfred GitHub App on at least one repository, then authorize again.",
                false,
            ));
        }
        let scopes = installation_scopes(&active);
        if !scopes.iter().any(|scope| scope == "metadata:read") {
            return Err(command_error(
                "github_permissions_missing",
                "The GitHub App installation does not grant repository metadata access.",
                false,
            ));
        }

        let identity_key =
            canonical_identity_key("github", "github_app_device", &[&user.id.to_string()]);
        let existing = db
            .get_app_connection_by_identity("github", "github_app_device", &identity_key)
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
        credential.refresh_token = token_response.refresh_token;
        credential.expires_at = token_response
            .expires_in
            .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
        if let Some(seconds) = token_response.refresh_token_expires_in {
            credential.provider_fields.insert(
                GITHUB_REFRESH_EXPIRY_FIELD.into(),
                (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339(),
            );
        }
        let saved_store = store.clone();
        let saved_ref = credential_ref.clone();
        tauri::async_runtime::spawn_blocking(move || saved_store.put(&saved_ref, &credential))
            .await
            .map_err(|_| credential_write_error())?
            .map_err(map_token_store_connect_error)?;

        let provider_metadata = BTreeMap::from([
            ("login".into(), user.login.clone()),
            ("installation_count".into(), active.len().to_string()),
        ]);
        let saved = db.upsert_app_connection(UpsertAppConnection {
            provider_id: "github".into(),
            display_name: Some(format!("@{}", user.login)),
            external_account_id: Some(user.id.to_string()),
            external_tenant_id: None,
            connection_mode: "github_app_device".into(),
            identity_key,
            scopes,
            provider_metadata,
            expires_at: token_response
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
        github_client()
    }
}

struct DeviceSession {
    device_code: String,
    expires_at: Instant,
    next_poll_at: Instant,
    interval_seconds: u64,
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.device_code.zeroize();
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDeviceAuthorization {
    pub pairing_session_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub installation_url: Option<String>,
    pub expires_at: String,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GitHubDevicePollResult {
    Pending { retry_after_seconds: u64 },
    Connected { connection: AppConnectionDto },
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Default, Deserialize)]
struct DeviceTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    refresh_token_expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct GitHubUser {
    id: u64,
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubInstallation {
    id: u64,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
    #[serde(default)]
    suspended_at: Option<Value>,
}

#[derive(Deserialize)]
struct GitHubInstallationsPage {
    #[serde(default)]
    installations: Vec<GitHubInstallation>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRepository {
    id: u64,
    full_name: String,
    html_url: String,
    #[serde(default)]
    archived: bool,
}

#[derive(Deserialize)]
struct GitHubRepositoriesPage {
    #[serde(default)]
    repositories: Vec<GitHubRepository>,
}

fn installation_scopes(installations: &[GitHubInstallation]) -> Vec<String> {
    let mut scopes = Vec::new();
    for installation in installations {
        for (permission, level) in &installation.permissions {
            if !valid_permission_name(permission) {
                continue;
            }
            match level.as_str() {
                "read" => scopes.push(format!("{permission}:read")),
                "write" | "admin" => {
                    scopes.push(format!("{permission}:read"));
                    scopes.push(format!("{permission}:write"));
                }
                _ => {}
            }
        }
    }
    scopes.sort();
    scopes.dedup();
    scopes
}

fn valid_permission_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_login(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_device_verification_uri(value: &str) -> bool {
    url::Url::parse(value)
        .ok()
        .is_some_and(|url| exact_github_https_origin(&url) && url.path() == "/login/device")
}

fn valid_github_install_url(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|url| {
        exact_github_https_origin(&url)
            && url.path().starts_with("/apps/")
            && url.path().len() > "/apps/".len()
    })
}

fn exact_github_https_origin(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}

struct GitHubRefreshHandler {
    client_id: Option<String>,
    auth_base: String,
}

impl RefreshHandler for GitHubRefreshHandler {
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
                .ok_or_else(|| ProviderRefreshError::terminal("github_not_configured"))?;
            let refresh_token = Zeroizing::new(
                credential
                    .refresh_token
                    .take()
                    .ok_or_else(|| ProviderRefreshError::terminal("github_grant_expired"))?,
            );
            let response: DeviceTokenResponse = auth_post_form(
                &format!("{}/oauth/access_token", self.auth_base),
                &[
                    ("client_id", client_id),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.as_str()),
                ],
            )
            .await
            .map_err(|_| ProviderRefreshError::retryable("github_unavailable"))?;
            if response.error.as_deref().is_some() {
                return Err(ProviderRefreshError::terminal("github_grant_expired"));
            }
            let access_token = response
                .access_token
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| ProviderRefreshError::retryable("github_invalid_response"))?;
            credential.access_token = access_token;
            credential.refresh_token = response.refresh_token;
            credential.expires_at = response
                .expires_in
                .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
            if let Some(seconds) = response.refresh_token_expires_in {
                credential.provider_fields.insert(
                    GITHUB_REFRESH_EXPIRY_FIELD.into(),
                    (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339(),
                );
            }
            Ok(credential)
        })
    }
}

fn action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            provider_id: "github".into(),
            action_id: "github.create_issue".into(),
            label: "Create GitHub issue".into(),
            description: "Create an issue in a repository selected for the GitHub App.".into(),
            fields: vec![
                repository_field(),
                text_field("title", "Title", "Issue title.", true, true),
                textarea_field("body", "Body", "Issue body in GitHub Markdown.", false),
                text_field(
                    "labels",
                    "Labels",
                    "Optional comma-separated label names.",
                    false,
                    false,
                ),
                text_field(
                    "assignees",
                    "Assignees",
                    "Optional comma-separated GitHub logins.",
                    false,
                    false,
                ),
            ],
            required_scopes: vec!["metadata:read".into(), "issues:write".into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "github".into(),
            action_id: "github.comment_on_issue".into(),
            label: "Comment on GitHub issue or PR".into(),
            description: "Add a timeline comment to an issue or pull request.".into(),
            fields: vec![
                repository_field(),
                text_field(
                    "number",
                    "Issue or PR number",
                    "Positive GitHub number.",
                    true,
                    true,
                ),
                textarea_field("body", "Comment", "Comment in GitHub Markdown.", true),
            ],
            required_scopes: vec!["metadata:read".into(), "issues:write".into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "github".into(),
            action_id: "github.create_pull_request".into(),
            label: "Create GitHub pull request".into(),
            description:
                "Open a pull request for existing branches. This action never pushes code.".into(),
            fields: vec![
                repository_field(),
                text_field(
                    "head",
                    "Head branch",
                    "Existing source branch or owner:branch.",
                    true,
                    true,
                ),
                text_field("base", "Base branch", "Existing target branch.", true, true),
                text_field("title", "Title", "Pull request title.", true, true),
                textarea_field(
                    "body",
                    "Body",
                    "Pull request body in GitHub Markdown.",
                    false,
                ),
                ActionFieldDescriptor {
                    key: "draft".into(),
                    label: "Draft".into(),
                    description: "Create the pull request as a draft.".into(),
                    kind: ActionFieldKind::Boolean,
                    required: false,
                    default: Some(Value::Bool(false)),
                    secret: false,
                    option_source: None,
                    options: vec![],
                    supports_interpolation: false,
                },
            ],
            required_scopes: vec!["metadata:read".into(), "pull_requests:write".into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "github".into(),
            action_id: "github.get_issue_or_pull_request".into(),
            label: "Get GitHub issue or PR".into(),
            description: "Fetch bounded issue or pull-request context for a later workflow step."
                .into(),
            fields: vec![
                repository_field(),
                text_field(
                    "number",
                    "Issue or PR number",
                    "Positive GitHub number.",
                    true,
                    true,
                ),
            ],
            required_scopes: vec!["metadata:read".into(), "issues:read".into()],
            output_schema_version: 1,
            output_is_untrusted: true,
        },
    ]
}

fn repository_field() -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: "repository".into(),
        label: "Repository".into(),
        description: "A repository explicitly selected for the GitHub App installation.".into(),
        kind: ActionFieldKind::ResourceSelector,
        required: true,
        default: None,
        secret: false,
        option_source: Some("repositories".into()),
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

impl ActionExecutor for GitHubService {
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
            let repository_id = required_positive_u64(&request.input, "repository")?;
            let repository = self
                .get_repository(token.as_str(), repository_id, false)
                .await?;
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            match request.action_id.as_str() {
                "github.create_issue" => {
                    ensure_writable_repository(&repository)?;
                    let title = required_bounded_text(&request.input, "title", 256)?;
                    let body = optional_bounded_text(&request.input, "body", MAX_TEXT_CHARS)?;
                    let labels = comma_separated(&request.input, "labels", 20, 100, false)?;
                    let assignees = comma_separated(&request.input, "assignees", 10, 39, true)?;
                    let (response, provider_request_id): (GitHubIssue, _) = self
                        .post_json(
                            token.as_str(),
                            &format!("/repos/{}/issues", repository.full_name),
                            &serde_json::json!({
                                "title": title,
                                "body": marked_body(&body),
                                "labels": labels,
                                "assignees": assignees,
                            }),
                        )
                        .await?;
                    validate_numbered_response(
                        response.number,
                        &response.state,
                        &response.html_url,
                    )?;
                    Ok(ActionResult {
                        summary: format!("Created GitHub issue #{}", response.number),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "repositoryId": repository.id,
                            "number": response.number,
                            "state": response.state,
                            "url": response.html_url,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: format!("Issue #{}", response.number),
                            uri: response.html_url,
                        }],
                        provider_request_id,
                    })
                }
                "github.comment_on_issue" => {
                    ensure_writable_repository(&repository)?;
                    let number = required_positive_u64(&request.input, "number")?;
                    let body = required_bounded_text(&request.input, "body", MAX_TEXT_CHARS)?;
                    let (response, provider_request_id): (GitHubComment, _) = self
                        .post_json(
                            token.as_str(),
                            &format!("/repos/{}/issues/{number}/comments", repository.full_name),
                            &serde_json::json!({ "body": marked_body(&body) }),
                        )
                        .await?;
                    if response.id == 0
                        || !valid_github_resource_url(&response.html_url)
                        || DateTime::parse_from_rfc3339(&response.created_at).is_err()
                    {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    Ok(ActionResult {
                        summary: format!("Commented on GitHub #{}", number),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "repositoryId": repository.id,
                            "number": number,
                            "commentId": response.id,
                            "url": response.html_url,
                            "createdAt": response.created_at,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: format!("Comment on #{}", number),
                            uri: response.html_url,
                        }],
                        provider_request_id,
                    })
                }
                "github.create_pull_request" => {
                    ensure_writable_repository(&repository)?;
                    let head = required_branch(&request.input, "head", true)?;
                    let base = required_branch(&request.input, "base", false)?;
                    if head == base {
                        return Err(ActionError::new(ActionErrorCode::InvalidInput));
                    }
                    let title = required_bounded_text(&request.input, "title", 256)?;
                    let body = optional_bounded_text(&request.input, "body", MAX_TEXT_CHARS)?;
                    let draft = request
                        .input
                        .get("draft")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let (response, provider_request_id): (GitHubPullRequest, _) = self
                        .post_json(
                            token.as_str(),
                            &format!("/repos/{}/pulls", repository.full_name),
                            &serde_json::json!({
                                "head": head,
                                "base": base,
                                "title": title,
                                "body": marked_body(&body),
                                "draft": draft,
                            }),
                        )
                        .await?;
                    validate_numbered_response(
                        response.number,
                        &response.state,
                        &response.html_url,
                    )?;
                    Ok(ActionResult {
                        summary: format!("Created GitHub pull request #{}", response.number),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "repositoryId": repository.id,
                            "number": response.number,
                            "state": response.state,
                            "draft": response.draft,
                            "url": response.html_url,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: format!("Pull request #{}", response.number),
                            uri: response.html_url,
                        }],
                        provider_request_id,
                    })
                }
                "github.get_issue_or_pull_request" => {
                    let number = required_positive_u64(&request.input, "number")?;
                    let response: GitHubIssueContext = self
                        .get_json(
                            token.as_str(),
                            &format!("/repos/{}/issues/{number}", repository.full_name),
                            &[],
                        )
                        .await?;
                    validate_numbered_response(
                        response.number,
                        &response.state,
                        &response.html_url,
                    )?;
                    if response.number != number {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    Ok(ActionResult {
                        summary: format!("Fetched GitHub #{}", response.number),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "repositoryId": repository.id,
                            "number": response.number,
                            "kind": if response.pull_request.is_some() { "pull_request" } else { "issue" },
                            "title": bounded(&response.title, 512),
                            "state": response.state,
                            "url": response.html_url,
                            "author": response.user.map(|user| user.login),
                            "labels": response.labels.into_iter().take(20).map(|label| label.name).collect::<Vec<_>>(),
                            "assignees": response.assignees.into_iter().take(20).map(|user| user.login).collect::<Vec<_>>(),
                            "bodyPreview": response.body.map(|body| bounded(&body, MAX_CONTEXT_CHARS)),
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: format!("GitHub #{}", response.number),
                            uri: response.html_url,
                        }],
                        provider_request_id: None,
                    })
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
        page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async move {
            if source != "repositories" || cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
            let token = Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            let page = self
                .list_repository_resources(token.as_str(), query, page_token)
                .await?;
            Ok(ActionResourcePage {
                items: page
                    .items
                    .into_iter()
                    .map(|item| ActionResourceItem {
                        id: item.id,
                        label: item.label,
                    })
                    .collect(),
                next_page_token: page.next_page_token,
            })
        })
    }
}

#[derive(Deserialize)]
struct GitHubIssue {
    number: u64,
    state: String,
    html_url: String,
}

#[derive(Deserialize)]
struct GitHubComment {
    id: u64,
    html_url: String,
    created_at: String,
}

#[derive(Deserialize)]
struct GitHubPullRequest {
    number: u64,
    state: String,
    #[serde(default)]
    draft: bool,
    html_url: String,
}

#[derive(Deserialize)]
struct GitHubIssueContext {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    user: Option<GitHubLogin>,
    #[serde(default)]
    labels: Vec<GitHubLabel>,
    #[serde(default)]
    assignees: Vec<GitHubLogin>,
    #[serde(default)]
    pull_request: Option<Value>,
}

#[derive(Deserialize)]
struct GitHubLogin {
    login: String,
}

#[derive(Deserialize)]
struct GitHubLabel {
    name: String,
}

impl GitHubService {
    async fn get_repository(
        &self,
        token: &str,
        repository_id: u64,
        allow_archived: bool,
    ) -> Result<GitHubRepository, ActionError> {
        let repository: GitHubRepository = self
            .get_json(token, &format!("/repositories/{repository_id}"), &[])
            .await?;
        if repository.id != repository_id
            || !valid_full_name(&repository.full_name)
            || !valid_github_resource_url(&repository.html_url)
            || (!allow_archived && repository.archived)
        {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        Ok(repository)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, ActionError> {
        github_get_json(&self.api_base, path, token, query).await
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        body: &Value,
    ) -> Result<(T, Option<String>), ActionError> {
        let request = github_request(
            self.client(),
            token,
            Method::POST,
            &format!("{}{}", self.api_base, path),
        )
        .json(body);
        send_github_json(request, true)
            .await
            .map(|response| (response.value, response.request_id))
    }

    async fn list_repository_resources(
        &self,
        token: &str,
        query: &str,
        page_token: Option<&str>,
    ) -> Result<RepositoryResourcePage, ActionError> {
        let mut cursor = page_token
            .map(decode_repository_cursor)
            .transpose()?
            .unwrap_or_default();
        if cursor.installation_page == 0 || cursor.repository_page == 0 {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let installation_page = self
            .list_installations(token, cursor.installation_page)
            .await?;
        let has_more_installations = installation_page.installations.len() == 100;
        let installations = installation_page
            .installations
            .into_iter()
            .filter(|installation| installation.suspended_at.is_none())
            .collect::<Vec<_>>();
        if cursor.installation_index >= installations.len() {
            if has_more_installations {
                cursor.installation_page += 1;
                cursor.installation_index = 0;
                cursor.repository_page = 1;
                return Ok(RepositoryResourcePage {
                    items: vec![],
                    next_page_token: Some(encode_repository_cursor(&cursor)?),
                });
            }
            return Ok(RepositoryResourcePage {
                items: vec![],
                next_page_token: None,
            });
        }
        let installation = &installations[cursor.installation_index];
        let repositories = self
            .list_installation_repositories(token, installation.id, cursor.repository_page)
            .await?
            .repositories;
        let query = query.trim().to_ascii_lowercase();
        let items = repositories
            .iter()
            .filter(|repository| !repository.archived)
            .filter(|repository| {
                query.is_empty() || repository.full_name.to_ascii_lowercase().contains(&query)
            })
            .filter(|repository| {
                valid_full_name(&repository.full_name)
                    && valid_github_resource_url(&repository.html_url)
            })
            .map(|repository| AppEventResourceItem {
                id: repository.id.to_string(),
                label: repository.full_name.clone(),
            })
            .take(100)
            .collect();
        let next_page_token = if repositories.len() == 100 {
            cursor.repository_page += 1;
            Some(encode_repository_cursor(&cursor)?)
        } else if cursor.installation_index + 1 < installations.len() {
            cursor.installation_index += 1;
            cursor.repository_page = 1;
            Some(encode_repository_cursor(&cursor)?)
        } else if has_more_installations {
            cursor.installation_page += 1;
            cursor.installation_index = 0;
            cursor.repository_page = 1;
            Some(encode_repository_cursor(&cursor)?)
        } else {
            None
        };
        Ok(RepositoryResourcePage {
            items,
            next_page_token,
        })
    }

    async fn list_installations(
        &self,
        token: &str,
        page: u32,
    ) -> Result<GitHubInstallationsPage, ActionError> {
        let page = page.to_string();
        self.get_json(
            token,
            "/user/installations",
            &[("per_page", "100"), ("page", page.as_str())],
        )
        .await
    }

    async fn list_installation_repositories(
        &self,
        token: &str,
        installation_id: u64,
        page: u32,
    ) -> Result<GitHubRepositoriesPage, ActionError> {
        let page = page.to_string();
        self.get_json(
            token,
            &format!("/user/installations/{installation_id}/repositories"),
            &[("per_page", "100"), ("page", page.as_str())],
        )
        .await
    }
}

#[derive(Serialize, Deserialize)]
struct RepositoryCursor {
    #[serde(default = "one")]
    installation_page: u32,
    #[serde(default)]
    installation_index: usize,
    #[serde(default = "one")]
    repository_page: u32,
}

impl Default for RepositoryCursor {
    fn default() -> Self {
        Self {
            installation_page: 1,
            installation_index: 0,
            repository_page: 1,
        }
    }
}

fn one() -> u32 {
    1
}

struct RepositoryResourcePage {
    items: Vec<AppEventResourceItem>,
    next_page_token: Option<String>,
}

fn encode_repository_cursor(cursor: &RepositoryCursor) -> Result<String, ActionError> {
    serde_json::to_vec(cursor)
        .map(|value| URL_SAFE_NO_PAD.encode(value))
        .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))
}

fn decode_repository_cursor(value: &str) -> Result<RepositoryCursor, ActionError> {
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))
}

fn ensure_writable_repository(repository: &GitHubRepository) -> Result<(), ActionError> {
    if repository.archived {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    } else {
        Ok(())
    }
}

fn validate_numbered_response(number: u64, state: &str, html_url: &str) -> Result<(), ActionError> {
    if number > 0 && matches!(state, "open" | "closed") && valid_github_resource_url(html_url) {
        Ok(())
    } else {
        Err(ActionError::new(ActionErrorCode::OutputInvalid))
    }
}

fn required_positive_u64(input: &BTreeMap<String, Value>, key: &str) -> Result<u64, ActionError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))
}

fn required_bounded_text(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ActionError> {
    let value = optional_bounded_text(input, key, max_chars)?;
    if value.trim().is_empty() {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    } else {
        Ok(value)
    }
}

fn optional_bounded_text(
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

fn comma_separated(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_items: usize,
    max_chars: usize,
    github_login: bool,
) -> Result<Vec<String>, ActionError> {
    let raw = optional_bounded_text(input, key, 2_000)?;
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.len() > max_items
        || values.iter().any(|value| {
            value.chars().count() > max_chars
                || value.chars().any(char::is_control)
                || (github_login && !valid_login(value))
        })
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(values)
}

fn required_branch(
    input: &BTreeMap<String, Value>,
    key: &str,
    allow_owner: bool,
) -> Result<String, ActionError> {
    let value = required_bounded_text(input, key, 256)?;
    let valid = value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
    }) && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && (allow_owner || !value.contains(':'));
    if valid {
        Ok(value)
    } else {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    }
}

fn marked_body(body: &str) -> String {
    if body.is_empty() {
        GITHUB_ACTION_MARKER.into()
    } else {
        format!("{body}\n\n{GITHUB_ACTION_MARKER}")
    }
}

fn bounded(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_owned()
    } else {
        format!(
            "{}\n… (truncated)",
            value.chars().take(max_chars).collect::<String>()
        )
    }
}

fn valid_full_name(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !repository.is_empty()
        && owner.len() <= 100
        && repository.len() <= 100
        && owner
            .bytes()
            .chain(repository.bytes())
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_github_resource_url(value: &str) -> bool {
    url::Url::parse(value)
        .ok()
        .is_some_and(|url| exact_github_https_origin(&url))
}

fn event_descriptors() -> Vec<AppEventDescriptor> {
    [
        (
            "github.issues",
            "GitHub issue activity",
            "Run when an issue is opened, edited, closed, reopened, assigned, or labeled.",
            vec![
                "opened", "edited", "closed", "reopened", "assigned", "labeled",
            ],
        ),
        (
            "github.issue_comment",
            "GitHub issue or PR comment",
            "Run when a non-Alfred timeline comment is created or edited.",
            vec!["created", "edited"],
        ),
        (
            "github.pull_request",
            "GitHub pull request activity",
            "Run when a pull request is opened, edited, closed, reopened, or synchronized.",
            vec!["opened", "edited", "closed", "reopened", "synchronize"],
        ),
        (
            "github.pull_request_review",
            "GitHub pull request review",
            "Run when a pull-request review is submitted, edited, or dismissed.",
            vec!["submitted", "edited", "dismissed"],
        ),
    ]
    .into_iter()
    .map(
        |(event_type, label, description, actions)| AppEventDescriptor {
            provider_id: "github".into(),
            event_type: event_type.into(),
            label: label.into(),
            description: description.into(),
            required_scopes: vec!["metadata:read".into()],
            delivery_modes: vec![AppEventDeliveryMode::Polling],
            filter_fields: vec![
                ActionFieldDescriptor {
                    key: "repositoryId".into(),
                    label: "Repository".into(),
                    description: "A repository selected for the GitHub App installation.".into(),
                    kind: ActionFieldKind::ResourceSelector,
                    required: true,
                    default: None,
                    secret: false,
                    option_source: Some("repositories".into()),
                    options: vec![],
                    supports_interpolation: false,
                },
                ActionFieldDescriptor {
                    key: "action".into(),
                    label: "Action".into(),
                    description: "Optionally limit which GitHub action starts the workflow.".into(),
                    kind: ActionFieldKind::Enum,
                    required: false,
                    default: Some(Value::String("any".into())),
                    secret: false,
                    option_source: None,
                    options: std::iter::once(ActionOption {
                        id: "any".into(),
                        label: "Any supported action".into(),
                    })
                    .chain(actions.into_iter().map(|action| ActionOption {
                        id: action.into(),
                        label: action.replace('_', " "),
                    }))
                    .collect(),
                    supports_interpolation: false,
                },
            ],
            fetches_resource_content: false,
            descriptor_version: 1,
            external_event_id_required: true,
            allowed_attribute_keys: vec![
                "repositoryId".into(),
                "eventAction".into(),
                "number".into(),
                "kind".into(),
                "status".into(),
            ],
            poll_interval_seconds: 60,
            pending_cap: 100,
        },
    )
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubEventCursor {
    recent_ids: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct GitHubRepositoryEvent {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    actor: GitHubEventActor,
    repo: GitHubEventRepository,
    #[serde(default)]
    payload: Value,
    created_at: String,
}

#[derive(Clone, Deserialize)]
struct GitHubEventActor {
    login: String,
}

#[derive(Clone, Deserialize)]
struct GitHubEventRepository {
    id: u64,
    name: String,
}

impl AppEventAdapter for GitHubService {
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
            let repository_id = config
                .filters
                .get("repositoryId")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| AppEventError::new(AppEventErrorCode::InvalidInput))?;
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let repository = self
                .get_repository(token.as_str(), repository_id, true)
                .await
                .map_err(map_action_error_to_event)?;
            let events: Vec<GitHubRepositoryEvent> = self
                .get_json(
                    token.as_str(),
                    &format!("/repos/{}/events", repository.full_name),
                    &[("per_page", "100")],
                )
                .await
                .map_err(map_action_error_to_event)?;
            let recent_ids = events
                .iter()
                .map(|event| event.id.clone())
                .collect::<Vec<_>>();
            if !valid_recent_event_ids(&recent_ids) {
                return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
            }
            // Keep the whole visible window, rather than only a timestamp, so
            // delayed/out-of-order events are still accepted exactly once.
            let next_cursor = Some(encode_event_cursor(&GitHubEventCursor { recent_ids })?);
            let Some(cursor) = cursor else {
                // Connecting a trigger establishes "now" and does not replay
                // the repository's recent activity history. An empty window
                // still gets a cursor so the first later event is not lost.
                return Ok(AppEventBatch {
                    cursor: next_cursor,
                    ..Default::default()
                });
            };
            let prior = decode_event_cursor(cursor)?;
            let seen = prior.recent_ids.into_iter().collect::<HashSet<_>>();
            let mut normalized = Vec::new();
            for event in events.into_iter().rev() {
                if cancellation.is_cancelled() {
                    return Err(AppEventError::new(AppEventErrorCode::Cancelled));
                }
                if seen.contains(&event.id) {
                    continue;
                }
                if let Some(event) = normalize_github_event(config, repository_id, event)? {
                    normalized.push(event);
                }
            }
            Ok(AppEventBatch {
                events: normalized,
                cursor: next_cursor.or_else(|| Some(cursor.to_owned())),
                ..Default::default()
            })
        })
    }

    fn list_filter_resources<'a>(
        &'a self,
        field_key: &'a str,
        query: &'a str,
        page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async move {
            if field_key != "repositoryId" || cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
            }
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let page = self
                .list_repository_resources(token.as_str(), query, page_token)
                .await
                .map_err(map_action_error_to_event)?;
            Ok(AppEventResourcePage {
                items: page.items,
                next_page_token: page.next_page_token,
            })
        })
    }
}

fn encode_event_cursor(cursor: &GitHubEventCursor) -> Result<String, AppEventError> {
    serde_json::to_vec(cursor)
        .map(|value| URL_SAFE_NO_PAD.encode(value))
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn decode_event_cursor(value: &str) -> Result<GitHubEventCursor, AppEventError> {
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .filter(|cursor: &GitHubEventCursor| valid_recent_event_ids(&cursor.recent_ids))
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn valid_recent_event_ids(ids: &[String]) -> bool {
    ids.len() <= 100
        && ids.iter().all(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn normalize_github_event(
    config: &AppTriggerConfig,
    repository_id: u64,
    event: GitHubRepositoryEvent,
) -> Result<Option<NormalizedAppEvent>, AppEventError> {
    if event.repo.id != repository_id || !valid_full_name(&event.repo.name) {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    let expected_kind = match config.event_type.as_str() {
        "github.issues" => "IssuesEvent",
        "github.issue_comment" => "IssueCommentEvent",
        "github.pull_request" => "PullRequestEvent",
        "github.pull_request_review" => "PullRequestReviewEvent",
        _ => return Err(AppEventError::new(AppEventErrorCode::InvalidInput)),
    };
    if event.kind != expected_kind {
        return Ok(None);
    }
    let action = event
        .payload
        .get("action")
        .and_then(Value::as_str)
        .filter(|value| valid_event_action(value))
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let filter = config
        .filters
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("any");
    if filter != "any" && filter != action {
        return Ok(None);
    }
    let primary_key = match config.event_type.as_str() {
        "github.pull_request" | "github.pull_request_review" => "pull_request",
        _ => "issue",
    };
    let primary = event
        .payload
        .get(primary_key)
        .and_then(Value::as_object)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let number = primary
        .get("number")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    let title = primary
        .get("title")
        .and_then(Value::as_str)
        .map(|value| bounded(value, 256));
    let status = primary
        .get("state")
        .and_then(Value::as_str)
        .filter(|value| {
            matches!(
                *value,
                "open" | "closed" | "pending" | "approved" | "changes_requested"
            )
        })
        .map(str::to_owned);
    let resource_url = primary
        .get("html_url")
        .and_then(Value::as_str)
        .filter(|value| valid_github_resource_url(value))
        .map(str::to_owned);
    let preview = match config.event_type.as_str() {
        "github.issue_comment" => event
            .payload
            .get("comment")
            .and_then(|value| value.get("body"))
            .and_then(Value::as_str),
        "github.pull_request_review" => event
            .payload
            .get("review")
            .and_then(|value| value.get("body"))
            .and_then(Value::as_str),
        _ => primary.get("body").and_then(Value::as_str),
    };
    let alfred_originated = preview.is_some_and(|body| body.contains(GITHUB_ACTION_MARKER))
        && match config.event_type.as_str() {
            // The marker belongs to this exact comment, so edits remain
            // Alfred-authored too. For issues and PRs only suppress creation;
            // later human closes/edits of an Alfred-created item must fire.
            "github.issue_comment" => true,
            "github.issues" | "github.pull_request" => action == "opened",
            _ => false,
        };
    if alfred_originated {
        return Ok(None);
    }
    let kind = if primary.get("pull_request").is_some()
        || matches!(
            config.event_type.as_str(),
            "github.pull_request" | "github.pull_request_review"
        ) {
        "pull_request"
    } else {
        "issue"
    };
    let mut attributes = BTreeMap::from([
        (
            "repositoryId".into(),
            Value::String(repository_id.to_string()),
        ),
        ("eventAction".into(), Value::String(action.into())),
        ("number".into(), Value::Number(number.into())),
        ("kind".into(), Value::String(kind.into())),
    ]);
    if let Some(status) = status {
        attributes.insert("status".into(), Value::String(status));
    }
    Ok(Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "github".into(),
        event_type: config.event_type.clone(),
        connection_id: config.connection_id.clone(),
        external_event_id: event.id,
        occurred_at: event.created_at,
        subject: title,
        actor: valid_login(&event.actor.login).then_some(event.actor.login),
        resource_url,
        preview: preview.map(|value| bounded(value, 1_000)),
        attributes,
    }))
}

fn valid_event_action(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
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

async fn list_all_installations(
    api_base: &str,
    token: &str,
) -> Result<Vec<GitHubInstallation>, ActionError> {
    let mut all = Vec::new();
    for page in 1..=10_u32 {
        let page_value = page.to_string();
        let response: GitHubInstallationsPage = github_get_json(
            api_base,
            "/user/installations",
            &token,
            &[("per_page", "100"), ("page", page_value.as_str())],
        )
        .await?;
        let count = response.installations.len();
        all.extend(response.installations);
        if count < 100 {
            break;
        }
    }
    Ok(all)
}

async fn auth_post_form<T: DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
) -> Result<T, ActionError> {
    let request = github_client()
        .post(url)
        .header("Accept", "application/json")
        .header("User-Agent", GITHUB_USER_AGENT)
        .form(form);
    send_github_json(request, false)
        .await
        .map(|response| response.value)
}

async fn github_get_json<T: DeserializeOwned>(
    api_base: &str,
    path: &str,
    token: &str,
    query: &[(&str, &str)],
) -> Result<T, ActionError> {
    let url = format!("{api_base}{path}");
    let request = github_request(github_client(), token, Method::GET, &url).query(query);
    send_github_json(request, false)
        .await
        .map(|response| response.value)
}

fn github_request(client: Client, token: &str, method: Method, url: &str) -> RequestBuilder {
    client
        .request(method, url)
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {token}"))
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .header("User-Agent", GITHUB_USER_AGENT)
}

fn github_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("GitHub HTTP client")
}

async fn send_github_json<T: DeserializeOwned>(
    request: RequestBuilder,
    mutation: bool,
) -> Result<GitHubJsonResponse<T>, ActionError> {
    let response = request.send().await.map_err(|error| {
        if mutation && (error.is_timeout() || !error.is_connect()) {
            ActionError::new(ActionErrorCode::DeliveryUnknown)
        } else {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
    })?;
    parse_github_response(response, mutation).await
}

async fn parse_github_response<T: DeserializeOwned>(
    response: Response,
    mutation: bool,
) -> Result<GitHubJsonResponse<T>, ActionError> {
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-github-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let retry_after = github_retry_after(&response);
    if !status.is_success() {
        let secondary_rate_limit = status == StatusCode::FORBIDDEN
            && retry_after.is_none()
            && github_error_indicates_secondary_rate_limit(response).await;
        let code = match status {
            StatusCode::UNAUTHORIZED => ActionErrorCode::ProviderUnauthorized,
            StatusCode::TOO_MANY_REQUESTS => ActionErrorCode::RateLimited,
            StatusCode::FORBIDDEN if retry_after.is_some() || secondary_rate_limit => {
                ActionErrorCode::RateLimited
            }
            StatusCode::FORBIDDEN => ActionErrorCode::ScopeMissing,
            StatusCode::NOT_FOUND
            | StatusCode::CONFLICT
            | StatusCode::GONE
            | StatusCode::UNPROCESSABLE_ENTITY => ActionErrorCode::InvalidInput,
            status if status.is_server_error() && mutation => ActionErrorCode::DeliveryUnknown,
            status if status.is_server_error() => ActionErrorCode::ProviderUnavailable,
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
        .is_some_and(|length| length as usize > GITHUB_RESPONSE_LIMIT)
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
        if bytes.len().saturating_add(chunk.len()) > GITHUB_RESPONSE_LIMIT {
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
    Ok(GitHubJsonResponse { value, request_id })
}

async fn github_error_indicates_secondary_rate_limit(response: Response) -> bool {
    if response
        .content_length()
        .is_some_and(|length| length as usize > GITHUB_ERROR_HINT_LIMIT)
    {
        return false;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return false;
        };
        if bytes.len().saturating_add(chunk.len()) > GITHUB_ERROR_HINT_LIMIT {
            return false;
        }
        bytes.extend_from_slice(&chunk);
    }
    let message = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    message.contains("secondary rate limit") || message.contains("abuse detection mechanism")
}

struct GitHubJsonResponse<T> {
    value: T,
    request_id: Option<String>,
}

fn github_retry_after(response: &Response) -> Option<u64> {
    if let Some(seconds) = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Some(seconds.min(86_400));
    }
    let remaining = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok());
    if remaining != Some("0") {
        return None;
    }
    response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .map(|reset| (reset - Utc::now().timestamp()).max(1) as u64)
        .map(|seconds| seconds.min(86_400))
}

fn map_auth_transport_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "GitHub is rate limiting authorization attempts. Try again later.",
            true,
        ),
        _ => command_error(
            "github_unavailable",
            "GitHub authorization is temporarily unavailable.",
            true,
        ),
    }
}

fn map_connect_action_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized => command_error(
            "github_authorization_expired",
            "GitHub rejected the authorization. Start a new connection attempt.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "GitHub is rate limiting requests. Try again later.",
            true,
        ),
        _ => command_error(
            "github_unavailable",
            "GitHub could not validate this connection. Try again.",
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

fn github_state_error() -> IntegrationCommandError {
    command_error(
        "github_pairing_failed",
        "The GitHub authorization attempt could not be updated.",
        true,
    )
}

fn github_pairing_expired() -> IntegrationCommandError {
    command_error(
        "github_pairing_expired",
        "This GitHub authorization attempt expired. Start again.",
        false,
    )
}

fn github_invalid_response() -> IntegrationCommandError {
    command_error(
        "github_invalid_response",
        "GitHub returned an invalid authorization response.",
        true,
    )
}

fn connection_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "GitHub was authorized, but the connection metadata could not be saved.",
        true,
    )
}

fn credential_write_error() -> IntegrationCommandError {
    command_error(
        "github_connection_failed",
        "The GitHub credential could not be saved securely.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use tiny_http::{Header, Response as TinyResponse, Server};

    fn test_service(base: String) -> GitHubService {
        GitHubService {
            client_id: Some("Iv1.public-client-id".into()),
            installation_url: Some("https://github.com/apps/alfred/installations/new".into()),
            auth_base: base.clone(),
            api_base: base,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn connection() -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "github".into(),
            display_name: Some("@octocat".into()),
            external_account_id: Some("1".into()),
            external_tenant_id: None,
            connection_mode: "github_app_device".into(),
            identity_key: "identity".into(),
            scopes: vec![
                "metadata:read".into(),
                "issues:read".into(),
                "issues:write".into(),
                "pull_requests:read".into(),
                "pull_requests:write".into(),
            ],
            provider_metadata: BTreeMap::new(),
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
                &CredentialEnvelope::new("ghu_secret_fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    fn json_header() -> Header {
        Header::from_bytes("Content-Type", "application/json").expect("header")
    }

    #[test]
    fn descriptors_are_secret_free_and_do_not_replace_the_legacy_git_host_node() {
        let descriptors = action_descriptors();
        assert_eq!(descriptors.len(), 4);
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.provider_id == "github"));
        assert!(descriptors
            .iter()
            .flat_map(|descriptor| descriptor.fields.iter())
            .all(|field| !field.secret));
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.action_id != "gitHost"));
    }

    #[test]
    fn installation_permissions_expand_write_to_read_without_admin_scope() {
        let scopes = installation_scopes(&[GitHubInstallation {
            id: 1,
            permissions: BTreeMap::from([
                ("metadata".into(), "read".into()),
                ("issues".into(), "write".into()),
                ("administration".into(), "none".into()),
            ]),
            suspended_at: None,
        }]);
        assert_eq!(scopes, vec!["issues:read", "issues:write", "metadata:read"]);
        assert!(!scopes.iter().any(|scope| scope.contains("administration")));
    }

    #[test]
    fn repository_and_event_cursors_are_opaque_bounded_round_trips() {
        let cursor = RepositoryCursor {
            installation_page: 2,
            installation_index: 4,
            repository_page: 7,
        };
        let encoded = encode_repository_cursor(&cursor).expect("encode");
        assert!(encoded.len() < 512);
        let decoded = decode_repository_cursor(&encoded).expect("decode");
        assert_eq!(decoded.installation_page, 2);
        assert_eq!(decoded.installation_index, 4);
        assert_eq!(decoded.repository_page, 7);

        let event = GitHubEventCursor {
            recent_ids: vec!["123".into(), "122".into()],
        };
        let encoded = encode_event_cursor(&event).expect("event encode");
        assert_eq!(
            decode_event_cursor(&encoded)
                .expect("event decode")
                .recent_ids,
            vec!["123", "122"]
        );
        assert!(decode_event_cursor(&URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({ "recent_ids": ["bad id"] })).unwrap()
        ))
        .is_err());
    }

    #[test]
    fn action_inputs_validate_ids_branches_and_bounded_lists() {
        let mut input = BTreeMap::from([
            ("repository".into(), Value::String("42".into())),
            ("number".into(), Value::String("7".into())),
            ("head".into(), Value::String("owner:feature/test".into())),
            ("base".into(), Value::String("main".into())),
            ("assignees".into(), Value::String("octocat,hubot".into())),
        ]);
        assert_eq!(required_positive_u64(&input, "repository").unwrap(), 42);
        assert_eq!(required_positive_u64(&input, "number").unwrap(), 7);
        assert_eq!(
            required_branch(&input, "head", true).unwrap(),
            "owner:feature/test"
        );
        assert_eq!(required_branch(&input, "base", false).unwrap(), "main");
        assert_eq!(
            comma_separated(&input, "assignees", 10, 39, true)
                .unwrap()
                .len(),
            2
        );
        input.insert("repository".into(), Value::String("../repo".into()));
        assert!(required_positive_u64(&input, "repository").is_err());
        assert!(valid_device_verification_uri(
            "https://github.com/login/device"
        ));
        assert!(!valid_device_verification_uri(
            "https://github.com:444/login/device"
        ));
        assert!(!valid_github_install_url(
            "https://user@github.com/apps/alfred/installations/new"
        ));
    }

    #[test]
    fn normalizer_skips_alfred_marked_comments_and_minimizes_external_content() {
        let config = AppTriggerConfig {
            provider_id: "github".into(),
            event_type: "github.issue_comment".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("repositoryId".into(), Value::String("42".into())),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let event = GitHubRepositoryEvent {
            id: "event-1".into(),
            kind: "IssueCommentEvent".into(),
            actor: GitHubEventActor {
                login: "octocat".into(),
            },
            repo: GitHubEventRepository {
                id: 42,
                name: "owner/repo".into(),
            },
            payload: serde_json::json!({
                "action": "created",
                "issue": {
                    "number": 7,
                    "title": "Question",
                    "state": "open",
                    "html_url": "https://github.com/owner/repo/issues/7"
                },
                "comment": { "body": format!("Automated\n{GITHUB_ACTION_MARKER}") }
            }),
            created_at: "2026-08-17T10:00:00Z".into(),
        };
        assert!(normalize_github_event(&config, 42, event)
            .unwrap()
            .is_none());

        let external = GitHubRepositoryEvent {
            id: "event-2".into(),
            kind: "IssueCommentEvent".into(),
            actor: GitHubEventActor {
                login: "octocat".into(),
            },
            repo: GitHubEventRepository {
                id: 42,
                name: "owner/repo".into(),
            },
            payload: serde_json::json!({
                "action": "created",
                "issue": {
                    "number": 7,
                    "title": "Question",
                    "state": "open",
                    "html_url": "https://github.com/owner/repo/issues/7"
                },
                "comment": { "body": "Please investigate" }
            }),
            created_at: "2026-08-17T10:01:00Z".into(),
        };
        let normalized = normalize_github_event(&config, 42, external)
            .unwrap()
            .expect("normalized");
        assert_eq!(normalized.preview.as_deref(), Some("Please investigate"));
        let serialized = serde_json::to_string(&normalized).unwrap();
        assert!(!serialized.contains("\"comment\":"));
        assert!(!serialized.contains("payload"));

        let issue_config = AppTriggerConfig {
            provider_id: "github".into(),
            event_type: "github.issues".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("repositoryId".into(), Value::String("42".into())),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let human_closed_alfred_issue = GitHubRepositoryEvent {
            id: "event-3".into(),
            kind: "IssuesEvent".into(),
            actor: GitHubEventActor {
                login: "maintainer".into(),
            },
            repo: GitHubEventRepository {
                id: 42,
                name: "owner/repo".into(),
            },
            payload: serde_json::json!({
                "action": "closed",
                "issue": {
                    "number": 8,
                    "title": "Created by Alfred",
                    "state": "closed",
                    "html_url": "https://github.com/owner/repo/issues/8",
                    "body": format!("Automated\n{GITHUB_ACTION_MARKER}")
                }
            }),
            created_at: "2026-08-17T10:02:00Z".into(),
        };
        assert!(
            normalize_github_event(&issue_config, 42, human_closed_alfred_issue)
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn event_poll_keeps_an_empty_initial_cursor_and_accepts_the_first_later_event() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            for step in 0..4 {
                let request = server.recv().expect("request");
                match step {
                    0 | 2 => {
                        assert_eq!(request.url(), "/repositories/42");
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"id":42,"full_name":"owner/repo","html_url":"https://github.com/owner/repo","archived":false}"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("repository response");
                    }
                    1 => {
                        assert!(request.url().starts_with("/repos/owner/repo/events?"));
                        request
                            .respond(TinyResponse::from_string("[]").with_header(json_header()))
                            .expect("empty events response");
                    }
                    _ => {
                        assert!(request.url().starts_with("/repos/owner/repo/events?"));
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"[{"id":"101","type":"IssuesEvent","actor":{"login":"octocat"},"repo":{"id":42,"name":"owner/repo"},"payload":{"action":"opened","issue":{"number":9,"title":"First event","state":"open","html_url":"https://github.com/owner/repo/issues/9","body":"External"}},"created_at":"2026-08-17T10:00:00Z"}]"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("event response");
                    }
                }
            }
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "github".into(),
            event_type: "github.issues".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("repositoryId".into(), Value::String("42".into())),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let initial = service
            .poll(
                &config,
                &connection(),
                None,
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("initial poll");
        assert!(initial.events.is_empty());
        let cursor = initial.cursor.expect("empty initial cursor");
        assert!(decode_event_cursor(&cursor).unwrap().recent_ids.is_empty());

        let next = service
            .poll(
                &config,
                &connection(),
                Some(&cursor),
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("next poll");
        responder.join().expect("responder");
        assert_eq!(next.events.len(), 1);
        assert_eq!(next.events[0].external_event_id, "101");
    }

    #[tokio::test]
    async fn event_poll_accepts_a_delayed_event_older_than_the_prior_window() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let repository = server.recv().expect("repository request");
            repository
                .respond(
                    TinyResponse::from_string(
                        r#"{"id":42,"full_name":"owner/repo","html_url":"https://github.com/owner/repo","archived":false}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("repository response");
            let events = server.recv().expect("events request");
            events
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"102","type":"IssuesEvent","actor":{"login":"octocat"},"repo":{"id":42,"name":"owner/repo"},"payload":{"action":"closed","issue":{"number":10,"title":"Delayed","state":"closed","html_url":"https://github.com/owner/repo/issues/10"}},"created_at":"2026-08-17T09:00:00Z"},{"id":"101","type":"IssuesEvent","actor":{"login":"octocat"},"repo":{"id":42,"name":"owner/repo"},"payload":{"action":"opened","issue":{"number":9,"title":"Seen","state":"open","html_url":"https://github.com/owner/repo/issues/9"}},"created_at":"2026-08-17T10:00:00Z"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("events response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "github".into(),
            event_type: "github.issues".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("repositoryId".into(), Value::String("42".into())),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let cursor = encode_event_cursor(&GitHubEventCursor {
            recent_ids: vec!["101".into()],
        })
        .expect("cursor");
        let batch = service
            .poll(
                &config,
                &connection(),
                Some(&cursor),
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("poll");
        responder.join().expect("responder");
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].external_event_id, "102");
    }

    #[tokio::test]
    async fn device_flow_saves_only_validated_installation_scoped_connection() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            for step in 0..4 {
                let mut request = server.recv().expect("request");
                match step {
                    0 => {
                        assert_eq!(request.url(), "/device/code");
                        let mut body = String::new();
                        request.as_reader().read_to_string(&mut body).expect("body");
                        assert!(body.contains("client_id=Iv1.public-client-id"));
                        assert!(!body.contains("client_secret"));
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"device_code":"device-secret-fixture","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("respond");
                    }
                    1 => {
                        assert_eq!(request.url(), "/oauth/access_token");
                        let mut body = String::new();
                        request.as_reader().read_to_string(&mut body).expect("body");
                        assert!(body.contains("device_code=device-secret-fixture"));
                        assert!(!body.contains("client_secret"));
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"access_token":"ghu_secret_fixture","token_type":"bearer","expires_in":28800,"refresh_token":"ghr_refresh_fixture","refresh_token_expires_in":15897600}"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("respond");
                    }
                    2 => {
                        assert_eq!(request.url(), "/user");
                        request
                            .respond(
                                TinyResponse::from_string(r#"{"id":1,"login":"octocat"}"#)
                                    .with_header(json_header()),
                            )
                            .expect("respond");
                    }
                    _ => {
                        assert!(request.url().starts_with("/user/installations?"));
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"installations":[{"id":9,"permissions":{"metadata":"read","issues":"write","pull_requests":"write"},"suspended_at":null}]}"#,
                                )
                                .with_header(json_header()),
                            )
                            .expect("respond");
                    }
                }
            }
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let prepared = service
            .prepare_device_authorization()
            .await
            .expect("prepare");
        assert_eq!(prepared.user_code, "ABCD-EFGH");
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let result = service
            .poll_device_authorization(&db, store.clone(), &prepared.pairing_session_id)
            .await
            .expect("poll");
        responder.join().expect("responder");
        let GitHubDevicePollResult::Connected { connection } = result else {
            panic!("expected connected result");
        };
        assert_eq!(connection.display_name.as_deref(), Some("@octocat"));
        assert!(connection.scopes.contains(&"issues:write".into()));
        assert!(connection.scopes.contains(&"pull_requests:write".into()));
        let saved = db
            .list_app_connections()
            .expect("connections")
            .pop()
            .expect("saved connection");
        assert_eq!(
            store
                .get(&saved.credential_ref)
                .expect("credential")
                .access_token,
            "ghu_secret_fixture"
        );
        let serialized = serde_json::to_string(&connection).expect("DTO");
        assert!(!serialized.contains("ghu_secret_fixture"));
        assert!(!serialized.contains("ghr_refresh_fixture"));
        assert!(!serialized.contains("device-secret-fixture"));
    }

    #[tokio::test]
    async fn create_issue_uses_numeric_repository_resolution_and_minimal_output() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let repository = server.recv().expect("repository request");
            assert_eq!(repository.url(), "/repositories/42");
            repository
                .respond(
                    TinyResponse::from_string(
                        r#"{"id":42,"full_name":"owner/repo","html_url":"https://github.com/owner/repo","archived":false}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("repository response");

            let mut issue = server.recv().expect("issue request");
            assert_eq!(issue.url(), "/repos/owner/repo/issues");
            assert_eq!(issue.method().as_str(), "POST");
            let mut body = String::new();
            issue.as_reader().read_to_string(&mut body).expect("body");
            assert!(body.contains("Release blocker"));
            assert!(body.contains(GITHUB_ACTION_MARKER));
            assert!(!body.contains("ghu_secret_fixture"));
            let request_id =
                Header::from_bytes("X-GitHub-Request-Id", "ABC123").expect("request id");
            issue
                .respond(
                    TinyResponse::from_string(
                        r#"{"number":7,"state":"open","html_url":"https://github.com/owner/repo/issues/7","body":"raw provider body"}"#,
                    )
                    .with_header(json_header())
                    .with_header(request_id),
                )
                .expect("issue response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "github".into(),
                    action_id: "github.create_issue".into(),
                    input: BTreeMap::from([
                        ("repository".into(), Value::String("42".into())),
                        ("title".into(), Value::String("Release blocker".into())),
                        ("body".into(), Value::String("Please investigate".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action");
        responder.join().expect("responder");
        assert_eq!(result.provider_request_id.as_deref(), Some("ABC123"));
        let serialized = serde_json::to_string(&result).expect("result");
        assert!(serialized.contains("issues/7"));
        assert!(!serialized.contains("raw provider body"));
        assert!(!serialized.contains("ghu_secret_fixture"));
    }

    #[tokio::test]
    async fn mutation_server_failure_is_delivery_unknown_and_keeps_request_id() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("mutation request");
            assert_eq!(request.url(), "/repos/owner/repo/issues");
            assert_eq!(request.method().as_str(), "POST");
            request
                .respond(TinyResponse::empty(502).with_header(
                    Header::from_bytes("X-GitHub-Request-Id", "UNKNOWN123").expect("request id"),
                ))
                .expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let error = service
            .post_json::<Value>(
                "ghu_secret_fixture",
                "/repos/owner/repo/issues",
                &serde_json::json!({ "title": "Maybe created" }),
            )
            .await
            .expect_err("ambiguous mutation failure");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::DeliveryUnknown);
        assert_eq!(error.provider_request_id.as_deref(), Some("UNKNOWN123"));
    }

    #[tokio::test]
    async fn repository_selector_lists_only_installation_repositories_and_maps_rate_limits() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let installations = server.recv().expect("installations");
            assert!(installations.url().starts_with("/user/installations?"));
            installations
                .respond(
                    TinyResponse::from_string(
                        r#"{"installations":[{"id":9,"permissions":{"metadata":"read"},"suspended_at":null}]}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("respond");
            let repositories = server.recv().expect("repositories");
            assert!(repositories
                .url()
                .starts_with("/user/installations/9/repositories?"));
            repositories
                .respond(
                    TinyResponse::from_string(
                        r#"{"repositories":[{"id":42,"full_name":"owner/repo","html_url":"https://github.com/owner/repo","archived":false}]}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("respond");
            let limited = server.recv().expect("limited");
            limited
                .respond(
                    TinyResponse::empty(403).with_header(
                        Header::from_bytes("Retry-After", "11").expect("retry header"),
                    ),
                )
                .expect("respond");
            let secondary = server.recv().expect("secondary limit");
            secondary
                .respond(
                    TinyResponse::from_string(
                        r#"{"message":"You have exceeded a secondary rate limit."}"#,
                    )
                    .with_status_code(403)
                    .with_header(json_header()),
                )
                .expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let page = service
            .list_resources(
                "repositories",
                "repository",
                "owner",
                None,
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("resources");
        assert_eq!(
            page.items,
            vec![ActionResourceItem {
                id: "42".into(),
                label: "owner/repo".into(),
            }]
        );
        let error = service
            .get_json::<Value>("ghu_secret_fixture", "/rate_limit", &[])
            .await
            .expect_err("rate limit");
        assert_eq!(error.code, ActionErrorCode::RateLimited);
        assert_eq!(error.retry_after_seconds, Some(11));
        let secondary = service
            .get_json::<Value>("ghu_secret_fixture", "/secondary_limit", &[])
            .await
            .expect_err("secondary rate limit");
        responder.join().expect("responder");
        assert_eq!(secondary.code, ActionErrorCode::RateLimited);
        assert_eq!(secondary.retry_after_seconds, None);
    }
}
