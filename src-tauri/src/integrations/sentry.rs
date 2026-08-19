//! Sentry incident connector (local auth-token mode).
//!
//! The local mode accepts a user-owned Sentry auth token through a
//! backend-owned secret form. Alfred uses the narrowest organization and
//! project scopes the token grants, never reads stack traces or event data
//! by default, and never persists Sentry-scrubbed secrets. Public OAuth and
//! webhook installation arrive with Plan 011; until then events use local
//! polling of organization issue lists.

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
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const SENTRY_API_BASE: &str = "https://sentry.io/api/0/";
const SENTRY_RESPONSE_LIMIT: usize = 512 * 1024;
const SENTRY_USER_AGENT: &str = "Alfred-Desktop";
const SENTRY_MAX_CONTEXT_CHARS: usize = 1_000;
const SENTRY_MAX_ORGS: usize = 10;
const SENTRY_MAX_ORG_METADATA_BYTES: usize = 8 * 1024;
const SENTRY_MAX_RECENT_ISSUES: usize = 100;
const SENTRY_HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const SENTRY_PROJECT_CACHE_TTL: Duration = Duration::from_secs(300);

const SCOPE_EVENT_READ: &str = "event:read";
const SCOPE_EVENT_WRITE: &str = "event:write";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryAuthTokenConnectionInput {
    pub auth_token: String,
}

impl Drop for SentryAuthTokenConnectionInput {
    fn drop(&mut self) {
        self.auth_token.zeroize();
    }
}

#[derive(Clone)]
struct SentryProject {
    id: String,
    slug: String,
    name: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct SentryOrganization {
    id: String,
    slug: String,
    name: String,
}

#[derive(Clone)]
struct ProjectCache {
    projects: Vec<SentryProject>,
    inserted_at: Instant,
}

pub struct SentryService {
    api_base: String,
    client: Client,
    project_cache: Mutex<HashMap<String, ProjectCache>>,
}

impl Default for SentryService {
    fn default() -> Self {
        Self::new(SENTRY_API_BASE).expect("Sentry HTTP client must be constructible")
    }
}

impl SentryService {
    fn new(api_base: &str) -> Result<Self, ActionError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(SENTRY_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        Ok(Self {
            api_base: api_base.trim_end_matches('/').to_owned() + "/",
            client,
            project_cache: Mutex::new(HashMap::new()),
        })
    }

    async fn request_json(
        &self,
        method: Method,
        path: &str,
        token: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
        mutation: bool,
    ) -> Result<Value, ActionError> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.api_base, path))
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", SENTRY_USER_AGENT)
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| {
            if mutation && (error.is_timeout() || !error.is_connect()) {
                ActionError::new(ActionErrorCode::DeliveryUnknown)
            } else {
                ActionError::new(ActionErrorCode::ProviderUnavailable)
            }
        })?;
        parse_sentry_response(response, mutation).await
    }
}

pub fn register(
    actions: &ActionRegistry,
    events: &AppEventRegistry,
    service: Arc<SentryService>,
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

pub async fn connect_private(
    db: &Db,
    store: Arc<dyn TokenStore>,
    mut input: SentryAuthTokenConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let supplied = Zeroizing::new(std::mem::take(&mut input.auth_token));
    let token = Zeroizing::new(supplied.trim().to_owned());
    validate_auth_token(token.as_str())?;
    connect_private_with_service(db, store, token.as_str(), &SentryService::default()).await
}

#[derive(Deserialize)]
struct SentryRootResponse {
    #[serde(default)]
    user: Option<SentryRootUser>,
    #[serde(default)]
    organizations: Vec<SentryRootOrganization>,
}

#[derive(Deserialize)]
struct SentryRootUser {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct SentryRootOrganization {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct SentryOrgDetail {
    #[serde(default)]
    access: Option<Vec<String>>,
}

async fn connect_private_with_service(
    db: &Db,
    store: Arc<dyn TokenStore>,
    token: &str,
    service: &SentryService,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let root_value = service
        .request_json(Method::GET, "", token, &[], None, false)
        .await
        .map_err(map_connect_action_error)?;
    let root: SentryRootResponse =
        serde_json::from_value(root_value).map_err(|_| sentry_identity_error())?;
    let organizations = root
        .organizations
        .into_iter()
        .take(SENTRY_MAX_ORGS + 1)
        .filter_map(|organization| {
            let id = organization.id.filter(|id| valid_sentry_id(id))?;
            let slug = organization
                .slug
                .filter(|slug| valid_sentry_slug(slug))?;
            let name = organization
                .name
                .map(|name| bounded(&name, 200))
                .filter(|name| !name.is_empty())?;
            Some(SentryOrganization { id, slug, name })
        })
        .collect::<Vec<_>>();
    if organizations.is_empty() || organizations.len() > SENTRY_MAX_ORGS {
        return Err(sentry_identity_error());
    }
    let user = root
        .user
        .and_then(|user| {
            Some((
                user.id.filter(|id| valid_sentry_id(id))?,
                user.name
                    .map(|name| bounded(&name, 200))
                    .filter(|name| !name.is_empty()),
            ))
        });
    let mut scopes = HashSet::new();
    for organization in &organizations {
        let detail_value = service
            .request_json(
                Method::GET,
                &format!("organizations/{}/", organization.slug),
                token,
                &[],
                None,
                false,
            )
            .await
            .map_err(map_connect_action_error)?;
        let detail: SentryOrgDetail =
            serde_json::from_value(detail_value).map_err(|_| sentry_identity_error())?;
        for scope in detail.access.unwrap_or_default() {
            if valid_sentry_scope(&scope) {
                scopes.insert(scope);
            }
        }
    }
    if !scopes.contains(SCOPE_EVENT_READ) {
        return Err(command_error(
            "sentry_scopes_missing",
            "This Sentry token does not grant issue access. Use a token with event:read.",
            false,
        ));
    }
    let mut scopes = scopes.into_iter().collect::<Vec<_>>();
    scopes.sort();
    let metadata_organizations =
        serde_json::to_string(&organizations).map_err(|_| sentry_identity_error())?;
    if metadata_organizations.len() > SENTRY_MAX_ORG_METADATA_BYTES {
        return Err(sentry_identity_error());
    }
    let identity_parts = user
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| {
            let mut ids = organizations
                .iter()
                .map(|organization| organization.id.clone())
                .collect::<Vec<_>>();
            ids.sort();
            ids.join(",")
        });
    let identity_key =
        canonical_identity_key("sentry", "auth_token", &[&identity_parts]);
    let existing = db
        .get_app_connection_by_identity("sentry", "auth_token", &identity_key)
        .map_err(|_| sentry_store_error())?;
    let credential_ref = existing
        .as_ref()
        .map(|connection| connection.credential_ref.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let is_new = existing.is_none();
    let prior_credential = if is_new {
        None
    } else {
        let prior_store = store.clone();
        let prior_ref = credential_ref.clone();
        tauri::async_runtime::spawn_blocking(move || prior_store.get(&prior_ref))
            .await
            .ok()
            .and_then(Result::ok)
    };
    let envelope = CredentialEnvelope::new(token.to_owned());
    let save_store = store.clone();
    let save_ref = credential_ref.clone();
    tauri::async_runtime::spawn_blocking(move || save_store.put(&save_ref, &envelope))
        .await
        .map_err(|_| sentry_credential_error())?
        .map_err(map_token_store_connect_error)?;

    let display_name = user
        .as_ref()
        .and_then(|(_, name)| name.clone())
        .or_else(|| {
            let names = organizations
                .iter()
                .map(|organization| organization.name.clone())
                .collect::<Vec<_>>();
            (!names.is_empty()).then(|| bounded(&names.join(", "), 200))
        });
    let metadata = BTreeMap::from([
        ("organizations".into(), metadata_organizations),
        ("organization_count".into(), organizations.len().to_string()),
        ("auth_mode".into(), "auth_token".into()),
        ("webhook_delivery".into(), "relay_required".into()),
    ]);
    let connection = db.upsert_app_connection(UpsertAppConnection {
        provider_id: "sentry".into(),
        display_name,
        external_account_id: user.as_ref().map(|(id, _)| id.clone()),
        external_tenant_id: None,
        connection_mode: "auth_token".into(),
        identity_key,
        scopes,
        provider_metadata: metadata,
        expires_at: None,
        credential_ref: credential_ref.clone(),
    });
    match connection {
        Ok(connection) => Ok(AppConnectionDto::from(connection)),
        Err(_) => {
            if is_new {
                let cleanup_store = store;
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    cleanup_store.delete(&credential_ref)
                })
                .await;
            } else if let Some(prior) = prior_credential {
                let rollback_store = store;
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    rollback_store.put(&credential_ref, &prior)
                })
                .await;
            }
            Err(sentry_store_error())
        }
    }
}

fn action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            provider_id: "sentry".into(),
            action_id: "sentry.get_issue".into(),
            label: "Get Sentry issue".into(),
            description:
                "Fetch issue metadata and a bounded latest-event summary. Never includes stack traces."
                    .into(),
            fields: vec![
                resource_field(
                    "project",
                    "Project",
                    "A project in the connected Sentry organizations.",
                    "projects",
                ),
                text_field(
                    "issue",
                    "Issue",
                    "Issue short ID like BACKEND-123 or a numeric issue ID.",
                    true,
                ),
            ],
            required_scopes: vec![SCOPE_EVENT_READ.into()],
            output_schema_version: 1,
            output_is_untrusted: true,
        },
        ActionDescriptor {
            provider_id: "sentry".into(),
            action_id: "sentry.update_issue_status".into(),
            label: "Update Sentry issue status".into(),
            description: "Resolve, ignore, or unresolve an issue with an explicit status change."
                .into(),
            fields: vec![
                resource_field(
                    "project",
                    "Project",
                    "A project in the connected Sentry organizations.",
                    "projects",
                ),
                text_field(
                    "issue",
                    "Issue",
                    "Issue short ID like BACKEND-123 or a numeric issue ID.",
                    true,
                ),
                enum_field(
                    "status",
                    "Status",
                    "The new issue status.",
                    "resolved",
                    &[
                        ("resolved", "Resolved"),
                        ("unresolved", "Unresolved"),
                        ("ignored", "Ignored"),
                    ],
                ),
                enum_field(
                    "ignore_until",
                    "Ignore until",
                    "Used only when status is Ignored.",
                    "forever",
                    &[
                        ("forever", "Forever"),
                        ("one_hour", "1 hour"),
                        ("one_day", "1 day"),
                        ("one_week", "1 week"),
                    ],
                ),
            ],
            required_scopes: vec![SCOPE_EVENT_WRITE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
    ]
}

fn resource_field(
    key: &str,
    label: &str,
    description: &str,
    source: &str,
) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::ResourceSelector,
        required: true,
        default: None,
        secret: false,
        option_source: Some(source.into()),
        options: vec![],
        supports_interpolation: false,
    }
}

fn text_field(key: &str, label: &str, description: &str, required: bool) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::Text,
        required,
        default: None,
        secret: false,
        option_source: None,
        options: vec![],
        supports_interpolation: true,
    }
}

fn enum_field(
    key: &str,
    label: &str,
    description: &str,
    default: &str,
    options: &[(&str, &str)],
) -> ActionFieldDescriptor {
    ActionFieldDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: ActionFieldKind::Enum,
        required: true,
        default: Some(Value::String(default.into())),
        secret: false,
        option_source: None,
        options: options
            .iter()
            .map(|(id, label)| ActionOption {
                id: (*id).into(),
                label: (*label).into(),
            })
            .collect(),
        supports_interpolation: false,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SentryIssueDetail {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    short_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    culprit: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    first_seen: Option<String>,
    #[serde(default)]
    last_seen: Option<String>,
    #[serde(default)]
    count: Option<SentryCount>,
    #[serde(default)]
    user_count: Option<u64>,
    #[serde(default)]
    permalink: Option<String>,
    #[serde(default)]
    project: Option<SentryIssueProject>,
    #[serde(default)]
    metadata: Option<SentryIssueMetadata>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SentryCount {
    Number(u64),
    Text(String),
}

impl SentryCount {
    fn value(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.trim().parse().ok(),
        }
    }
}

#[derive(Deserialize)]
struct SentryIssueProject {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct SentryIssueMetadata {
    #[serde(rename = "type")]
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

struct SentryIssueSummary {
    id: String,
    short_id: Option<String>,
    title: String,
    culprit: Option<String>,
    level: Option<String>,
    status: Option<String>,
    first_seen: Option<String>,
    last_seen: Option<String>,
    count: Option<u64>,
    user_count: Option<u64>,
    permalink: Option<String>,
    project: Option<SentryIssueProject>,
    metadata: Option<SentryIssueMetadata>,
}

fn issue_summary(detail: SentryIssueDetail) -> Result<SentryIssueSummary, ActionError> {
    let id = detail
        .id
        .filter(|id| valid_sentry_id(id))
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let title = detail
        .title
        .map(|title| bounded(&title, 512))
        .filter(|title| !title.is_empty())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    if let Some(short_id) = detail.short_id.as_deref() {
        if !valid_sentry_short_id(short_id) {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    if detail
        .first_seen
        .as_deref()
        .is_some_and(|value| DateTime::parse_from_rfc3339(value).is_err())
        || detail
            .last_seen
            .as_deref()
            .is_some_and(|value| DateTime::parse_from_rfc3339(value).is_err())
    {
        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
    }
    if let Some(level) = detail.level.as_deref() {
        if !valid_sentry_level(level) {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    if let Some(status) = detail.status.as_deref() {
        if !valid_sentry_status(status) {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    if let Some(permalink) = detail.permalink.as_deref() {
        if !valid_sentry_url(permalink) {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    if let Some(project) = detail.project.as_ref() {
        if project
            .id
            .as_deref()
            .is_none_or(|id| !valid_sentry_id(id))
            || project
                .slug
                .as_deref()
                .is_some_and(|slug| !valid_sentry_slug(slug))
        {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    if let Some(metadata) = detail.metadata.as_ref() {
        if metadata
            .kind
            .as_deref()
            .is_some_and(|kind| kind.chars().count() > 128)
            || metadata
                .title
                .as_deref()
                .is_some_and(|title| title.chars().count() > 512)
        {
            return Err(ActionError::new(ActionErrorCode::OutputInvalid));
        }
    }
    Ok(SentryIssueSummary {
        id,
        short_id: detail.short_id,
        title,
        culprit: detail.culprit.map(|culprit| bounded(&culprit, 512)),
        level: detail.level,
        status: detail.status,
        first_seen: detail.first_seen,
        last_seen: detail.last_seen,
        count: detail.count.as_ref().and_then(SentryCount::value),
        user_count: detail.user_count,
        permalink: detail.permalink,
        project: detail.project,
        metadata: detail.metadata,
    })
}

impl ActionExecutor for SentryService {
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
            let project_id = required_bounded_text(&request.input, "project", 64)?;
            let issue_ref = required_bounded_text(&request.input, "issue", 128)?;
            let org_slug = self
                .resolve_project_org(token.as_str(), connection, &project_id)
                .await?;
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            match request.action_id.as_str() {
                "sentry.get_issue" => {
                    let issue = self
                        .resolve_issue(token.as_str(), &org_slug, &project_id, &issue_ref)
                        .await?;
                    let metadata = issue.metadata.as_ref().map(|metadata| {
                        serde_json::json!({
                            "type": metadata.kind,
                            "title": metadata.title,
                            "value": metadata.value.as_deref().map(|value| bounded(value, SENTRY_MAX_CONTEXT_CHARS)),
                        })
                    });
                    let project = issue.project.as_ref().map(|project| {
                        serde_json::json!({
                            "id": project.id,
                            "slug": project.slug,
                            "name": project.name,
                        })
                    });
                    let artifact_uri = issue.permalink.clone();
                    Ok(ActionResult {
                        summary: format!(
                            "Fetched Sentry issue {}",
                            issue.short_id.as_deref().unwrap_or(&issue.id)
                        ),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "issueId": issue.id,
                            "shortId": issue.short_id,
                            "title": issue.title,
                            "culprit": issue.culprit,
                            "level": issue.level,
                            "status": issue.status,
                            "firstSeen": issue.first_seen,
                            "lastSeen": issue.last_seen,
                            "count": issue.count,
                            "userCount": issue.user_count,
                            "permalink": issue.permalink,
                            "project": project,
                            "metadata": metadata,
                        }),
                        artifacts: artifact_uri
                            .map(|uri| vec![ActionArtifact {
                                kind: "url".into(),
                                label: "Sentry issue".into(),
                                uri,
                            }])
                            .unwrap_or_default(),
                        provider_request_id: None,
                    })
                }
                "sentry.update_issue_status" => {
                    let status = required_bounded_text(&request.input, "status", 32)?;
                    if !valid_sentry_status(&status) {
                        return Err(ActionError::new(ActionErrorCode::InvalidInput));
                    }
                    let ignore_until = required_bounded_text(&request.input, "ignore_until", 32)?;
                    if status != "ignored" && ignore_until != "forever" {
                        return Err(ActionError::new(ActionErrorCode::InvalidInput));
                    }
                    let status_details = ignore_details(&status, &ignore_until)?;
                    let issue = self
                        .resolve_issue(token.as_str(), &org_slug, &project_id, &issue_ref)
                        .await?;
                    let mut body = serde_json::json!({ "status": status });
                    if let Some(details) = status_details {
                        body["statusDetails"] = details;
                    }
                    let response = self
                        .request_json(
                            Method::PUT,
                            &format!("issues/{}/", issue.id),
                            token.as_str(),
                            &[],
                            Some(&body),
                            true,
                        )
                        .await?;
                    let updated: SentryIssueDetail = serde_json::from_value(response)
                        .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let updated = issue_summary(updated)?;
                    if updated.id != issue.id {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    if updated.status.as_deref() != Some(status.as_str()) {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    let short_id = updated
                        .short_id
                        .clone()
                        .unwrap_or_else(|| updated.id.clone());
                    let artifact_uri = updated.permalink.clone();
                    Ok(ActionResult {
                        summary: format!("Updated Sentry issue {short_id} to {status}"),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "issueId": updated.id,
                            "shortId": updated.short_id,
                            "status": updated.status,
                            "permalink": updated.permalink,
                        }),
                        artifacts: artifact_uri
                            .map(|uri| vec![ActionArtifact {
                                kind: "url".into(),
                                label: format!("Sentry {short_id}"),
                                uri,
                            }])
                            .unwrap_or_default(),
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
        _page_token: Option<&'a str>,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async move {
            if source != "projects" || cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
            let token = Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            let projects = self
                .list_projects(token.as_str(), connection)
                .await?;
            let query = query.trim().to_ascii_lowercase();
            let items = projects
                .into_iter()
                .filter(|project| {
                    query.is_empty()
                        || project.name.to_ascii_lowercase().contains(&query)
                        || project.slug.to_ascii_lowercase().contains(&query)
                })
                .take(100)
                .map(|project| ActionResourceItem {
                    id: project.id,
                    label: bounded(&project.name, 256),
                })
                .collect();
            Ok(ActionResourcePage {
                items,
                next_page_token: None,
            })
        })
    }
}

impl SentryService {
    async fn resolve_issue(
        &self,
        token: &str,
        org_slug: &str,
        project_id: &str,
        issue_ref: &str,
    ) -> Result<SentryIssueSummary, ActionError> {
        let summary = if issue_ref.bytes().all(|byte| byte.is_ascii_digit()) {
            let value = self
                .request_json(
                    Method::GET,
                    &format!("issues/{issue_ref}/"),
                    token,
                    &[],
                    None,
                    false,
                )
                .await?;
            let detail: SentryIssueDetail = serde_json::from_value(value)
                .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?;
            issue_summary(detail)?
        } else if valid_sentry_short_id(issue_ref) {
            let response = self
                .request_json(
                    Method::GET,
                    &format!("organizations/{org_slug}/issues/"),
                    token,
                    &[("query", issue_ref), ("shortIdLookup", "true")],
                    None,
                    false,
                )
                .await?;
            let issues = response
                .as_array()
                .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
            let detail: SentryIssueDetail = serde_json::from_value(
                issues
                    .first()
                    .cloned()
                    .unwrap_or(Value::Null),
            )
            .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?;
            issue_summary(detail)?
        } else {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        };
        let belongs = summary
            .project
            .as_ref()
            .and_then(|project| project.id.as_deref())
            .is_some_and(|id| id == project_id);
        if !belongs {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        Ok(summary)
    }

    async fn list_projects(
        &self,
        token: &str,
        connection: &AppConnection,
    ) -> Result<Vec<SentryProject>, ActionError> {
        let organizations = connection_organizations(connection)?;
        let mut all = Vec::new();
        for organization in organizations {
            let cached = self
                .project_cache
                .lock()
                .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
                .get(&organization.slug)
                .filter(|cache| cache.inserted_at.elapsed() < SENTRY_PROJECT_CACHE_TTL)
                .cloned()
                .map(|cache| cache.projects);
            if let Some(projects) = cached {
                all.extend(projects);
                continue;
            }
            let response = self
                .request_json(
                    Method::GET,
                    &format!("organizations/{}/projects/", organization.slug),
                    token,
                    &[],
                    None,
                    false,
                )
                .await?;
            let projects = parse_projects(&response);
            self.project_cache
                .lock()
                .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
                .insert(
                    organization.slug.clone(),
                    ProjectCache {
                        projects: projects.clone(),
                        inserted_at: Instant::now(),
                    },
                );
            all.extend(projects);
        }
        Ok(all)
    }

    async fn resolve_project_org(
        &self,
        token: &str,
        connection: &AppConnection,
        project_id: &str,
    ) -> Result<String, ActionError> {
        let organizations = connection_organizations(connection)?;
        for organization in organizations {
            let projects = self
                .list_projects_for_org(token, &organization.slug)
                .await?;
            if projects
                .iter()
                .any(|project| project.id == project_id)
            {
                return Ok(organization.slug);
            }
        }
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    }

    async fn list_projects_for_org(
        &self,
        token: &str,
        org_slug: &str,
    ) -> Result<Vec<SentryProject>, ActionError> {
        if let Some(cache) = self
            .project_cache
            .lock()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
            .get(org_slug)
            .filter(|cache| cache.inserted_at.elapsed() < SENTRY_PROJECT_CACHE_TTL)
        {
            return Ok(cache.projects.clone());
        }
        let response = self
            .request_json(
                Method::GET,
                &format!("organizations/{org_slug}/projects/"),
                token,
                &[],
                None,
                false,
            )
            .await?;
        let projects = parse_projects(&response);
        self.project_cache
            .lock()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
            .insert(
                org_slug.to_owned(),
                ProjectCache {
                    projects: projects.clone(),
                    inserted_at: Instant::now(),
                },
            );
        Ok(projects)
    }
}

fn parse_projects(response: &Value) -> Vec<SentryProject> {
    response
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            let id = project
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_sentry_id(id))?;
            let slug = project
                .get("slug")
                .and_then(Value::as_str)
                .filter(|slug| valid_sentry_slug(slug))?;
            let name = project
                .get("name")
                .and_then(Value::as_str)
                .map(|name| bounded(name, 256))
                .filter(|name| !name.is_empty())?;
            Some(SentryProject {
                id: id.to_owned(),
                slug: slug.to_owned(),
                name,
            })
        })
        .collect()
}

fn connection_organizations(connection: &AppConnection) -> Result<Vec<SentryOrganization>, ActionError> {
    let raw = connection
        .provider_metadata
        .get("organizations")
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
    let organizations: Vec<SentryOrganization> = serde_json::from_str(raw)
        .map_err(|_| ActionError::new(ActionErrorCode::InvalidInput))?;
    if organizations.is_empty() || organizations.len() > SENTRY_MAX_ORGS {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    if organizations.iter().any(|organization| {
        !valid_sentry_id(&organization.id) || !valid_sentry_slug(&organization.slug)
    }) {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(organizations)
}

fn ignore_details(status: &str, ignore_until: &str) -> Result<Option<Value>, ActionError> {
    if status != "ignored" {
        return Ok(None);
    }
    let minutes = match ignore_until {
        "forever" => return Ok(None),
        "one_hour" => 60,
        "one_day" => 1_440,
        "one_week" => 10_080,
        _ => return Err(ActionError::new(ActionErrorCode::InvalidInput)),
    };
    Ok(Some(serde_json::json!({ "ignoreDuration": minutes })))
}

fn event_descriptors() -> Vec<AppEventDescriptor> {
    vec![AppEventDescriptor {
        provider_id: "sentry".into(),
        event_type: "sentry.issue_alert".into(),
        label: "Sentry issue alert".into(),
        description:
            "Run when an issue is created, resolved, regressed, or updated in a selected project."
                .into(),
        required_scopes: vec![SCOPE_EVENT_READ.into()],
        delivery_modes: vec![AppEventDeliveryMode::Polling],
        filter_fields: vec![
            resource_field(
                "projectId",
                "Project",
                "A project in the connected Sentry organizations.",
                "projects",
            ),
            enum_field(
                "action",
                "Action",
                "Optionally limit which Sentry change starts the workflow.",
                "any",
                &[
                    ("any", "Any supported action"),
                    ("created", "Issue created"),
                    ("resolved", "Issue resolved"),
                    ("regressed", "Issue regressed"),
                    ("updated", "Issue updated"),
                ],
            ),
        ],
        fetches_resource_content: false,
        descriptor_version: 1,
        external_event_id_required: true,
        allowed_attribute_keys: vec![
            "projectId".into(),
            "action".into(),
            "level".into(),
            "status".into(),
            "shortId".into(),
        ],
        poll_interval_seconds: 60,
        pending_cap: 100,
    }]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SentryRecentIssue {
    id: String,
    status: String,
    #[serde(rename = "lastSeen")]
    last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SentryEventCursor {
    #[serde(default)]
    recent: Vec<SentryRecentIssue>,
    #[serde(default)]
    watermark: String,
}

impl AppEventAdapter for SentryService {
    fn poll<'a>(
        &'a self,
        config: &'a AppTriggerConfig,
        connection: &'a AppConnection,
        cursor: Option<&'a str>,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::Cancelled));
            }
            let project_id = config
                .filters
                .get("projectId")
                .and_then(Value::as_str)
                .filter(|value| valid_sentry_id(value))
                .ok_or_else(|| AppEventError::new(AppEventErrorCode::InvalidInput))?
                .to_owned();
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let org_slug = self
                .resolve_project_org(token.as_str(), connection, &project_id)
                .await
                .map_err(map_action_error_to_event)?;
            let response = self
                .request_json(
                    Method::GET,
                    &format!("organizations/{org_slug}/issues/"),
                    token.as_str(),
                    &[
                        ("project", project_id.as_str()),
                        ("limit", "100"),
                        ("statsPeriod", "90d"),
                        ("sort", "date"),
                    ],
                    None,
                    false,
                )
                .await
                .map_err(map_action_error_to_event)?;
            let details = response
                .as_array()
                .cloned()
                .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
            let issues = details
                .into_iter()
                .filter_map(|value| {
                    let detail: SentryIssueDetail = serde_json::from_value(value).ok()?;
                    let summary = issue_summary(detail).ok()?;
                    let status = summary.status.clone()?;
                    let last_seen = summary.last_seen.clone()?;
                    if summary
                        .project
                        .as_ref()
                        .and_then(|project| project.id.as_deref())
                        != Some(project_id.as_str())
                    {
                        return None;
                    }
                    Some(SentryPollIssue {
                        summary,
                        status,
                        last_seen,
                    })
                })
                .collect::<Vec<_>>();
            if issues.len() > SENTRY_MAX_RECENT_ISSUES {
                return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
            }
            let page_watermark = issues
                .iter()
                .filter_map(|issue| DateTime::parse_from_rfc3339(&issue.last_seen).ok())
                .max()
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            let recent = issues
                .iter()
                .map(|issue| SentryRecentIssue {
                    id: issue.summary.id.clone(),
                    status: issue.status.clone(),
                    last_seen: issue.last_seen.clone(),
                })
                .collect::<Vec<_>>();
            let next_cursor = encode_event_cursor(&SentryEventCursor {
                watermark: page_watermark.clone(),
                recent,
            })?;
            let Some(cursor) = cursor else {
                // Connecting a trigger establishes "now" and does not replay
                // the project's recent issue history.
                return Ok(AppEventBatch {
                    cursor: Some(next_cursor),
                    ..Default::default()
                });
            };
            let prior = decode_event_cursor(cursor)?;
            let prior_watermark = prior
                .watermark
                .parse::<DateTime<Utc>>()
                .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?;
            let prior_status = prior
                .recent
                .into_iter()
                .map(|entry| (entry.id, entry.status))
                .collect::<HashMap<_, _>>();
            let mut normalized = Vec::new();
            for issue in issues.into_iter().rev() {
                if cancellation.is_cancelled() {
                    return Err(AppEventError::new(AppEventErrorCode::Cancelled));
                }
                let action = match prior_status.get(&issue.summary.id) {
                    None => {
                        let first_seen = issue
                            .summary
                            .first_seen
                            .as_deref()
                            .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
                        if first_seen.is_some_and(|first| first >= prior_watermark) {
                            "created"
                        } else {
                            "updated"
                        }
                    }
                    Some(previous) if *previous != issue.status => {
                        if issue.status == "resolved" {
                            "resolved"
                        } else if previous == "resolved" && issue.status == "unresolved" {
                            "regressed"
                        } else {
                            "updated"
                        }
                    }
                    Some(_) => "updated",
                };
                if let Some(event) =
                    normalize_sentry_issue(config, project_id.as_str(), &issue.summary, action)?
                {
                    normalized.push(event);
                }
            }
            Ok(AppEventBatch {
                events: normalized,
                cursor: Some(next_cursor),
                ..Default::default()
            })
        })
    }

    fn list_filter_resources<'a>(
        &'a self,
        field_key: &'a str,
        query: &'a str,
        _page_token: Option<&'a str>,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async move {
            if field_key != "projectId" || cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
            }
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let projects = self
                .list_projects(token.as_str(), connection)
                .await
                .map_err(map_action_error_to_event)?;
            let query = query.trim().to_ascii_lowercase();
            let items = projects
                .into_iter()
                .filter(|project| {
                    query.is_empty()
                        || project.name.to_ascii_lowercase().contains(&query)
                        || project.slug.to_ascii_lowercase().contains(&query)
                })
                .take(100)
                .map(|project| AppEventResourceItem {
                    id: project.id,
                    label: bounded(&project.name, 256),
                })
                .collect();
            Ok(AppEventResourcePage {
                items,
                next_page_token: None,
            })
        })
    }
}

struct SentryPollIssue {
    summary: SentryIssueSummary,
    status: String,
    last_seen: String,
}

fn normalize_sentry_issue(
    config: &AppTriggerConfig,
    project_id: &str,
    issue: &SentryIssueSummary,
    action: &str,
) -> Result<Option<NormalizedAppEvent>, AppEventError> {
    let filter = config
        .filters
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("any");
    if filter != "any" && filter != action {
        return Ok(None);
    }
    let mut attributes = BTreeMap::from([
        ("projectId".into(), Value::String(project_id.to_owned())),
        ("action".into(), Value::String(action.into())),
    ]);
    if let Some(short_id) = issue.short_id.as_deref() {
        attributes.insert("shortId".into(), Value::String(bounded(short_id, 64)));
    }
    if let Some(level) = issue.level.as_deref() {
        attributes.insert("level".into(), Value::String(level.into()));
    }
    if let Some(status) = issue.status.as_deref() {
        attributes.insert("status".into(), Value::String(status.into()));
    }
    let occurred_at = issue
        .last_seen
        .as_deref()
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    Ok(Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "sentry".into(),
        event_type: config.event_type.clone(),
        connection_id: config.connection_id.clone(),
        external_event_id: format!("{}@{}", issue.id, occurred_at),
        occurred_at: occurred_at.into(),
        subject: Some(issue.title.clone()),
        actor: None,
        resource_url: issue.permalink.clone(),
        preview: None,
        attributes,
    }))
}

fn encode_event_cursor(cursor: &SentryEventCursor) -> Result<String, AppEventError> {
    serde_json::to_vec(cursor)
        .map(|value| URL_SAFE_NO_PAD.encode(value))
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn decode_event_cursor(value: &str) -> Result<SentryEventCursor, AppEventError> {
    let cursor = URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SentryEventCursor>(&bytes).ok())
        .filter(valid_event_cursor)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    Ok(cursor)
}

fn valid_event_cursor(cursor: &SentryEventCursor) -> bool {
    DateTime::parse_from_rfc3339(&cursor.watermark).is_ok()
        && cursor.recent.len() <= SENTRY_MAX_RECENT_ISSUES
        && cursor.recent.iter().all(|entry| {
            valid_sentry_id(&entry.id)
                && valid_sentry_status(&entry.status)
                && DateTime::parse_from_rfc3339(&entry.last_seen).is_ok()
        })
}

fn required_bounded_text(
    input: &BTreeMap<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ActionError> {
    let value = input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(value.to_owned())
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

fn valid_sentry_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_sentry_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_sentry_short_id(value: &str) -> bool {
    let Some((prefix, number)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.len() <= 50
        && prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && !number.is_empty()
        && number.len() <= 10
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_sentry_level(value: &str) -> bool {
    matches!(
        value,
        "fatal" | "error" | "warning" | "info" | "debug" | "sample"
    )
}

fn valid_sentry_status(value: &str) -> bool {
    matches!(value, "resolved" | "unresolved" | "ignored")
}

fn valid_sentry_scope(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b':' | b'_' | b'-'))
}

fn valid_sentry_url(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
            && value.len() <= 2_048
    })
}

fn validate_auth_token(token: &str) -> Result<(), IntegrationCommandError> {
    let prefix = token
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sntrys_") || prefix.eq_ignore_ascii_case("sntryu_"));
    let valid = prefix
        && token.len() >= 40
        && token.len() <= 512
        && !token.chars().any(char::is_whitespace)
        && !token.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(command_error(
            "sentry_token_invalid",
            "Enter a valid Sentry auth token beginning with sntrys_ or sntryu_.",
            false,
        ))
    }
}

async fn parse_sentry_response(response: Response, mutation: bool) -> Result<Value, ActionError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.min(86_400));
        let code = match status {
            StatusCode::UNAUTHORIZED => ActionErrorCode::ProviderUnauthorized,
            StatusCode::FORBIDDEN => ActionErrorCode::ScopeMissing,
            StatusCode::TOO_MANY_REQUESTS => ActionErrorCode::RateLimited,
            StatusCode::BAD_REQUEST
            | StatusCode::NOT_FOUND
            | StatusCode::CONFLICT
            | StatusCode::GONE
            | StatusCode::UNPROCESSABLE_ENTITY => ActionErrorCode::InvalidInput,
            status if status.is_server_error() && mutation => ActionErrorCode::DeliveryUnknown,
            status if status.is_server_error() => ActionErrorCode::ProviderUnavailable,
            _ => ActionErrorCode::ProviderUnavailable,
        };
        if code == ActionErrorCode::RateLimited {
            return Err(ActionError::rate_limited(retry_after));
        }
        return Err(ActionError::new(code));
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > SENTRY_RESPONSE_LIMIT)
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
        if bytes.len().saturating_add(chunk.len()) > SENTRY_RESPONSE_LIMIT {
            return Err(ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::OutputTooLarge
            }));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        ActionError::new(if mutation {
            ActionErrorCode::DeliveryUnknown
        } else {
            ActionErrorCode::OutputInvalid
        })
    })
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

fn map_connect_action_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized => command_error(
            "sentry_token_invalid",
            "Sentry rejected this auth token.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Sentry is rate limiting connection checks. Try again later.",
            true,
        ),
        _ => command_error(
            "sentry_connection_failed",
            "Sentry could not validate this auth token.",
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
        _ => sentry_credential_error(),
    }
}

fn command_error(code: &str, message: &str, recoverable: bool) -> IntegrationCommandError {
    IntegrationCommandError::new(code, message, recoverable)
}

fn sentry_identity_error() -> IntegrationCommandError {
    command_error(
        "sentry_identity_invalid",
        "Sentry did not return a valid user and organization identity.",
        false,
    )
}

fn sentry_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "Sentry was validated, but the connection metadata could not be saved.",
        true,
    )
}

fn sentry_credential_error() -> IntegrationCommandError {
    command_error(
        "sentry_connection_failed",
        "Sentry was validated, but its credential could not be saved.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use tiny_http::{Header, Response as TinyResponse, Server};

    fn test_service(base: String) -> SentryService {
        SentryService::new(&base).expect("test service")
    }

    fn connection() -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "sentry".into(),
            display_name: Some("Ada".into()),
            external_account_id: Some("123".into()),
            external_tenant_id: None,
            connection_mode: "auth_token".into(),
            identity_key: "identity".into(),
            scopes: vec![SCOPE_EVENT_READ.into(), SCOPE_EVENT_WRITE.into()],
            provider_metadata: BTreeMap::from([(
                "organizations".into(),
                serde_json::to_string(&vec![SentryOrganization {
                    id: "1".into(),
                    slug: "acme".into(),
                    name: "Acme".into(),
                }])
                .expect("metadata"),
            )]),
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
                &CredentialEnvelope::new("sntryu_secret_fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    fn json_header() -> Header {
        Header::from_bytes("Content-Type", "application/json").expect("header")
    }

    fn issue_detail_json() -> &'static str {
        r#"{"id":"12345","shortId":"BACKEND-1","title":"TypeError: cannot read","culprit":"app.py in main","level":"error","status":"unresolved","firstSeen":"2099-01-01T09:00:00Z","lastSeen":"2099-01-01T10:00:00Z","count":"3","userCount":2,"permalink":"https://acme.sentry.io/issues/12345/","project":{"id":"450","slug":"backend","name":"Backend"},"metadata":{"type":"TypeError","title":"TypeError","value":"raw message body"},"latestEvent":{"entries":[{"type":"exception","data":{"values":[{"stacktrace":{"frames":[{"filename":"app.py"}]}}]}}]}}"#
    }

    #[test]
    fn descriptors_are_secret_free_and_scoped_to_sentry() {
        let descriptors = action_descriptors();
        assert_eq!(descriptors.len(), 2);
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.provider_id == "sentry"));
        assert!(descriptors
            .iter()
            .flat_map(|descriptor| descriptor.fields.iter())
            .all(|field| !field.secret));
        assert!(descriptors
            .iter()
            .find(|descriptor| descriptor.action_id == "sentry.get_issue")
            .is_some_and(|descriptor| descriptor.output_is_untrusted));
        let events = event_descriptors();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].delivery_modes,
            vec![AppEventDeliveryMode::Polling]
        );
        assert!(events[0]
            .allowed_attribute_keys
            .iter()
            .all(|key| !secret_like_key(key)));
    }

    fn secret_like_key(key: &str) -> bool {
        let compact = key.to_ascii_lowercase().replace(['-', '_'], "");
        ["token", "secret", "authorization", "signature", "password"]
            .iter()
            .any(|needle| compact.contains(needle))
    }

    #[test]
    fn sentry_identifiers_and_cursors_validate_strictly() {
        assert!(valid_sentry_id("450"));
        assert!(valid_sentry_id("450abcdef-123"));
        assert!(!valid_sentry_id("../etc/passwd"));
        assert!(valid_sentry_slug("backend-1"));
        assert!(!valid_sentry_slug("Backend"));
        assert!(valid_sentry_short_id("BACKEND-123"));
        assert!(valid_sentry_short_id("javascript.backend-42"));
        assert!(!valid_sentry_short_id("no-number"));
        assert!(valid_sentry_level("error"));
        assert!(!valid_sentry_level("trace"));
        assert!(valid_sentry_status("resolved"));
        assert!(!valid_sentry_status("muted"));
        assert!(valid_sentry_scope("project:read"));
        assert!(!valid_sentry_scope("Project Read"));

        let cursor = SentryEventCursor {
            recent: vec![SentryRecentIssue {
                id: "12345".into(),
                status: "unresolved".into(),
                last_seen: "2099-01-01T10:00:00Z".into(),
            }],
            watermark: "2099-01-01T10:00:00Z".into(),
        };
        let encoded = encode_event_cursor(&cursor).expect("encode");
        assert!(encoded.len() < 1024);
        let decoded = decode_event_cursor(&encoded).expect("decode");
        assert_eq!(decoded.recent.len(), 1);
        assert!(decode_event_cursor("not-a-cursor").is_err());
    }

    #[tokio::test]
    async fn connect_saves_only_validated_scoped_identity() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let root = server.recv().expect("root request");
            assert_eq!(root.url(), "/");
            assert_eq!(
                root.headers()
                    .iter()
                    .find(|header| header.field.equiv("Authorization"))
                    .expect("auth header")
                    .value
                    .as_str(),
                "Bearer sntryu_secret_fixture"
            );
            root.respond(
                TinyResponse::from_string(
                    r#"{"user":{"id":"123","name":"Ada"},"organizations":[{"id":"1","slug":"acme","name":"Acme"}]}"#,
                )
                .with_header(json_header()),
            )
            .expect("root response");
            let org = server.recv().expect("org request");
            assert_eq!(org.url(), "/organizations/acme/");
            org.respond(
                TinyResponse::from_string(
                    r#"{"id":"1","slug":"acme","name":"Acme","access":["org:read","project:read","event:read","event:write"]}"#,
                )
                .with_header(json_header()),
            )
            .expect("org response");
        });
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let connected = connect_private_with_service(
            &db,
            store.clone(),
            "sntryu_secret_fixture",
            &test_service(format!("http://127.0.0.1:{port}")),
        )
        .await
        .expect("connect");
        responder.join().expect("responder");
        assert_eq!(connected.display_name.as_deref(), Some("Ada"));
        assert!(connected.scopes.contains(&SCOPE_EVENT_READ.into()));
        assert!(connected.scopes.contains(&SCOPE_EVENT_WRITE.into()));
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
            "sntryu_secret_fixture"
        );
        let serialized = serde_json::to_string(&connected).expect("DTO");
        assert!(!serialized.contains("sntryu_secret_fixture"));
    }

    #[tokio::test]
    async fn connect_requires_event_read_scope() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let root = server.recv().expect("root request");
            root.respond(
                TinyResponse::from_string(
                    r#"{"user":{"id":"123","name":"Ada"},"organizations":[{"id":"1","slug":"acme","name":"Acme"}]}"#,
                )
                .with_header(json_header()),
            )
            .expect("root response");
            let org = server.recv().expect("org request");
            org.respond(
                TinyResponse::from_string(
                    r#"{"access":["org:read","project:read"]}"#,
                )
                .with_header(json_header()),
            )
            .expect("org response");
        });
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let error = connect_private_with_service(
            &db,
            store,
            "sntryu_secret_fixture",
            &test_service(format!("http://127.0.0.1:{port}")),
        )
        .await
        .expect_err("missing scope");
        responder.join().expect("responder");
        assert_eq!(error.code, "sentry_scopes_missing");
    }

    #[tokio::test]
    async fn get_issue_resolves_project_and_strips_stack_traces() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let projects = server.recv().expect("projects request");
            assert_eq!(projects.url(), "/organizations/acme/projects/");
            projects
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"450","slug":"backend","name":"Backend"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("projects response");
            let issue = server.recv().expect("issue request");
            assert_eq!(issue.url(), "/issues/12345/");
            issue
                .respond(
                    TinyResponse::from_string(issue_detail_json()).with_header(json_header()),
                )
                .expect("issue response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "sentry".into(),
                    action_id: "sentry.get_issue".into(),
                    input: BTreeMap::from([
                        ("project".into(), Value::String("450".into())),
                        ("issue".into(), Value::String("12345".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action");
        responder.join().expect("responder");
        assert_eq!(result.summary, "Fetched Sentry issue BACKEND-1");
        let serialized = serde_json::to_string(&result).expect("result");
        assert!(serialized.contains("raw message body"));
        assert!(!serialized.contains("stacktrace"));
        assert!(!serialized.contains("frames"));
        assert!(!serialized.contains("latestEvent"));
        assert!(!serialized.contains("sntryu_secret_fixture"));
    }

    #[tokio::test]
    async fn get_issue_rejects_issues_outside_the_selected_project() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let projects = server.recv().expect("projects request");
            projects
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"450","slug":"backend","name":"Backend"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("projects response");
            let issue = server.recv().expect("issue request");
            issue
                .respond(
                    TinyResponse::from_string(
                        r#"{"id":"12345","shortId":"OTHER-1","title":"Other project","culprit":null,"level":"error","status":"unresolved","firstSeen":"2099-01-01T09:00:00Z","lastSeen":"2099-01-01T10:00:00Z","count":"1","userCount":1,"permalink":"https://acme.sentry.io/issues/12345/","project":{"id":"999","slug":"other","name":"Other"}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("issue response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let error = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "sentry".into(),
                    action_id: "sentry.get_issue".into(),
                    input: BTreeMap::from([
                        ("project".into(), Value::String("450".into())),
                        ("issue".into(), Value::String("12345".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect_err("project isolation");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn update_issue_status_sends_ignore_duration_and_validates_echo() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let projects = server.recv().expect("projects request");
            projects
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"450","slug":"backend","name":"Backend"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("projects response");
            let lookup = server.recv().expect("lookup request");
            lookup
                .respond(
                    TinyResponse::from_string(format!("[{}]", issue_detail_json()))
                        .with_header(json_header()),
                )
                .expect("lookup response");
            let mut update = server.recv().expect("update request");
            assert_eq!(update.url(), "/issues/12345/");
            assert_eq!(update.method().as_str(), "PUT");
            let mut body = String::new();
            update.as_reader().read_to_string(&mut body).expect("body");
            assert!(body.contains("\"status\":\"ignored\""));
            assert!(body.contains("\"ignoreDuration\":1440"));
            assert!(!body.contains("sntryu_secret_fixture"));
            update
                .respond(
                    TinyResponse::from_string(
                        r#"{"id":"12345","shortId":"BACKEND-1","title":"TypeError","level":"error","status":"ignored","firstSeen":"2099-01-01T09:00:00Z","lastSeen":"2099-01-01T10:00:00Z","count":"3","permalink":"https://acme.sentry.io/issues/12345/","project":{"id":"450","slug":"backend","name":"Backend"}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("update response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "sentry".into(),
                    action_id: "sentry.update_issue_status".into(),
                    input: BTreeMap::from([
                        ("project".into(), Value::String("450".into())),
                        ("issue".into(), Value::String("BACKEND-1".into())),
                        ("status".into(), Value::String("ignored".into())),
                        ("ignore_until".into(), Value::String("one_day".into())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action");
        responder.join().expect("responder");
        assert_eq!(result.summary, "Updated Sentry issue BACKEND-1 to ignored");
    }

    #[tokio::test]
    async fn event_poll_detects_created_resolved_and_regressed_transitions() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            // Request sequence: the second poll reuses the cached project
            // list, so exactly one projects request precedes two issue-list
            // requests.
            let projects = server.recv().expect("projects request");
            projects
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"450","slug":"backend","name":"Backend"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("projects response");
            for step in 0..2 {
                let issues = server.recv().expect("issues request");
                assert!(issues.url().starts_with("/organizations/acme/issues/?"));
                assert!(issues.url().contains("project=450"));
                if step == 0 {
                    issues
                        .respond(TinyResponse::from_string("[]").with_header(json_header()))
                        .expect("empty response");
                } else {
                    issues
                        .respond(
                            TinyResponse::from_string(
                                r#"[{"id":"12345","shortId":"BACKEND-1","title":"Fresh crash","culprit":null,"level":"error","status":"unresolved","firstSeen":"2099-01-02T10:00:00Z","lastSeen":"2099-01-02T10:00:00Z","count":"1","userCount":1,"permalink":"https://acme.sentry.io/issues/12345/","project":{"id":"450","slug":"backend","name":"Backend"}},{"id":"12346","shortId":"BACKEND-2","title":"Old crash","culprit":null,"level":"warning","status":"resolved","firstSeen":"2099-01-01T09:00:00Z","lastSeen":"2099-01-02T09:00:00Z","count":"4","userCount":1,"permalink":"https://acme.sentry.io/issues/12346/","project":{"id":"450","slug":"backend","name":"Backend"}}]"#,
                            )
                            .with_header(json_header()),
                        )
                        .expect("issues response");
                }
            }
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "sentry".into(),
            event_type: "sentry.issue_alert".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("projectId".into(), Value::String("450".into())),
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

        // Second round: BACKEND-1 is created; BACKEND-2 was previously
        // unresolved and is now resolved.
        let prior = SentryEventCursor {
            watermark: "2099-01-01T12:00:00Z".into(),
            recent: vec![
                SentryRecentIssue {
                    id: "12346".into(),
                    status: "unresolved".into(),
                    last_seen: "2099-01-01T09:00:00Z".into(),
                },
            ],
        };
        let prior_cursor = encode_event_cursor(&prior).expect("prior cursor");
        let next = service
            .poll(
                &config,
                &connection(),
                Some(&prior_cursor),
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("next poll");
        responder.join().expect("responder");
        assert_eq!(next.events.len(), 2);
        let created = next
            .events
            .iter()
            .find(|event| event.external_event_id.starts_with("12345"))
            .expect("created event");
        assert_eq!(
            created.attributes.get("action").and_then(Value::as_str),
            Some("created")
        );
        let resolved = next
            .events
            .iter()
            .find(|event| event.external_event_id.starts_with("12346"))
            .expect("resolved event");
        assert_eq!(
            resolved.attributes.get("action").and_then(Value::as_str),
            Some("resolved")
        );
        let serialized = serde_json::to_string(&next.events).expect("events");
        assert!(!serialized.contains("sntryu_secret_fixture"));
    }

    #[tokio::test]
    async fn event_poll_reports_regressed_when_resolved_issue_reopens() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let projects = server.recv().expect("projects request");
            projects
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"450","slug":"backend","name":"Backend"}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("projects response");
            let issues = server.recv().expect("issues request");
            issues
                .respond(
                    TinyResponse::from_string(
                        r#"[{"id":"12346","shortId":"BACKEND-2","title":"Old crash","culprit":null,"level":"error","status":"unresolved","firstSeen":"2099-01-01T09:00:00Z","lastSeen":"2099-01-03T09:00:00Z","count":"5","userCount":1,"permalink":"https://acme.sentry.io/issues/12346/","project":{"id":"450","slug":"backend","name":"Backend"}}]"#,
                    )
                    .with_header(json_header()),
                )
                .expect("issues response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "sentry".into(),
            event_type: "sentry.issue_alert".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                ("projectId".into(), Value::String("450".into())),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let prior = SentryEventCursor {
            watermark: "2099-01-02T12:00:00Z".into(),
            recent: vec![SentryRecentIssue {
                id: "12346".into(),
                status: "resolved".into(),
                last_seen: "2099-01-02T09:00:00Z".into(),
            }],
        };
        let prior_cursor = encode_event_cursor(&prior).expect("prior cursor");
        let batch = service
            .poll(
                &config,
                &connection(),
                Some(&prior_cursor),
                token_capability().await,
                AppEventCancellation::never(),
            )
            .await
            .expect("poll");
        responder.join().expect("responder");
        assert_eq!(batch.events.len(), 1);
        assert_eq!(
            batch.events[0]
                .attributes
                .get("action")
                .and_then(Value::as_str),
            Some("regressed")
        );
    }

    #[tokio::test]
    async fn rate_limited_429_maps_with_retry_after() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            request
                .respond(
                    TinyResponse::empty(429).with_header(
                        Header::from_bytes("Retry-After", "60").expect("retry after"),
                    ),
                )
                .expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let error = service
            .request_json(Method::GET, "", "sntryu_secret_fixture", &[], None, false)
            .await
            .expect_err("rate limited");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::RateLimited);
        assert_eq!(error.retry_after_seconds, Some(60));
    }
}
