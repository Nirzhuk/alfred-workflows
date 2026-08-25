//! Linear connected-app provider (personal API key mode).
//!
//! The local MVP accepts a user-owned personal API key through a
//! backend-owned secret form. The key is validated against the Linear
//! GraphQL API, stored only in the OS credential store, and never returned
//! to React. Public OAuth and relay webhooks arrive with Plan 011; until
//! then events use local polling of issue updates.

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
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const LINEAR_API_BASE: &str = "https://api.linear.app";
const LINEAR_RESPONSE_LIMIT: usize = 512 * 1024;
const LINEAR_USER_AGENT: &str = "Alfred-Desktop";
const LINEAR_ACTION_MARKER: &str = "<!-- alfred-connected-app -->";
const LINEAR_MAX_TEXT_CHARS: usize = 32 * 1024;
const LINEAR_MAX_CONTEXT_CHARS: usize = 4_000;
const LINEAR_PAGE_SIZE: usize = 50;
const LINEAR_MAX_ISSUES_RECENT: usize = 100;
const LINEAR_HTTP_TIMEOUT: Duration = Duration::from_secs(20);

const SCOPE_WORKSPACE_READ: &str = "workspace:read";
const SCOPE_ISSUES_READ: &str = "issues:read";
const SCOPE_ISSUES_WRITE: &str = "issues:write";
const SCOPE_COMMENTS_WRITE: &str = "comments:write";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearPrivateConnectionInput {
    pub api_key: String,
}

impl Drop for LinearPrivateConnectionInput {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

pub struct LinearService {
    api_base: String,
    client: Client,
}

impl Default for LinearService {
    fn default() -> Self {
        Self::new(LINEAR_API_BASE).expect("Linear HTTP client must be constructible")
    }
}

impl LinearService {
    fn new(api_base: &str) -> Result<Self, ActionError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(LINEAR_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        Ok(Self {
            api_base: api_base.trim_end_matches('/').into(),
            client,
        })
    }

    async fn graphql(
        &self,
        token: &str,
        query: &str,
        variables: Option<&Value>,
        mutation: bool,
    ) -> Result<Value, ActionError> {
        let mut body = serde_json::json!({ "query": query });
        if let Some(variables) = variables {
            body["variables"] = variables.clone();
        }
        let request = self
            .client
            .request(Method::POST, format!("{}/graphql", self.api_base))
            .header("Accept", "application/json")
            .header("Authorization", token)
            .header("User-Agent", LINEAR_USER_AGENT)
            .json(&body);
        let response = request.send().await.map_err(|error| {
            if mutation && (error.is_timeout() || !error.is_connect()) {
                ActionError::new(ActionErrorCode::DeliveryUnknown)
            } else {
                ActionError::new(ActionErrorCode::ProviderUnavailable)
            }
        })?;
        parse_linear_response(response, mutation).await
    }
}

pub fn register(
    actions: &ActionRegistry,
    events: &AppEventRegistry,
    service: Arc<LinearService>,
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
    mut input: LinearPrivateConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let supplied = Zeroizing::new(std::mem::take(&mut input.api_key));
    let token = Zeroizing::new(supplied.trim().to_owned());
    validate_api_key(token.as_str())?;
    connect_private_with_service(db, store, token.as_str(), &LinearService::default()).await
}

async fn connect_private_with_service(
    db: &Db,
    store: Arc<dyn TokenStore>,
    token: &str,
    service: &LinearService,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    let identity = service
        .graphql(token, VALIDATE_QUERY, None, false)
        .await
        .map_err(map_connect_action_error)?;
    let viewer = identity
        .get("data")
        .and_then(|data| data.get("viewer"))
        .and_then(Value::as_object)
        .ok_or_else(linear_identity_error)?;
    let viewer_id = viewer
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| valid_linear_id(value))
        .ok_or_else(linear_identity_error)?
        .to_owned();
    let viewer_name = viewer
        .get("name")
        .and_then(Value::as_str)
        .map(|value| bounded(value, 120))
        .filter(|value| !value.is_empty());
    let organization = viewer
        .get("organization")
        .and_then(Value::as_object)
        .ok_or_else(linear_identity_error)?;
    let organization_id = organization
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| valid_linear_id(value))
        .ok_or_else(linear_identity_error)?
        .to_owned();
    let organization_name = organization
        .get("name")
        .and_then(Value::as_str)
        .map(|value| bounded(value, 120))
        .filter(|value| !value.is_empty());

    let identity_key =
        canonical_identity_key("linear", "personal_token", &[&organization_id, &viewer_id]);
    let existing = db
        .get_app_connection_by_identity("linear", "personal_token", &identity_key)
        .map_err(|_| linear_store_error())?;
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
        .map_err(|_| linear_credential_error())?
        .map_err(map_token_store_connect_error)?;

    let metadata = BTreeMap::from([
        ("organization_id".into(), organization_id.clone()),
        (
            "organization_name".into(),
            organization_name.clone().unwrap_or_default(),
        ),
        ("viewer_id".into(), viewer_id.clone()),
        (
            "viewer_name".into(),
            viewer_name.clone().unwrap_or_default(),
        ),
        ("auth_mode".into(), "personal_token".into()),
        ("webhook_delivery".into(), "relay_required".into()),
    ]);
    let scopes = vec![
        SCOPE_WORKSPACE_READ.into(),
        SCOPE_ISSUES_READ.into(),
        SCOPE_ISSUES_WRITE.into(),
        SCOPE_COMMENTS_WRITE.into(),
    ];
    let connection = db.upsert_app_connection(UpsertAppConnection {
        provider_id: "linear".into(),
        display_name: organization_name.or(viewer_name),
        external_account_id: Some(viewer_id),
        external_tenant_id: Some(organization_id),
        connection_mode: "personal_token".into(),
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
            Err(linear_store_error())
        }
    }
}

const VALIDATE_QUERY: &str = "query LinearValidate { viewer { id name organization { id name } } }";

fn action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            provider_id: "linear".into(),
            action_id: "linear.create_issue".into(),
            label: "Create Linear issue".into(),
            description: "Create an issue in a team of the connected Linear workspace.".into(),
            fields: vec![
                resource_field(
                    "team",
                    "Team",
                    "A team in the connected Linear workspace.",
                    "teams",
                ),
                text_field("title", "Title", "Issue title.", true),
                textarea_field(
                    "description",
                    "Description",
                    "Issue description in Linear Markdown.",
                    false,
                ),
                enum_field(
                    "priority",
                    "Priority",
                    "Issue priority.",
                    "no_priority",
                    &[
                        ("no_priority", "No priority"),
                        ("urgent", "Urgent"),
                        ("high", "High"),
                        ("medium", "Medium"),
                        ("low", "Low"),
                    ],
                ),
                resource_field(
                    "assignee",
                    "Assignee",
                    "Optional assignee from the connected workspace.",
                    "assignees",
                ),
                text_field(
                    "labels",
                    "Labels",
                    "Optional comma-separated label names from the selected team.",
                    false,
                ),
            ],
            required_scopes: vec![SCOPE_ISSUES_READ.into(), SCOPE_ISSUES_WRITE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "linear".into(),
            action_id: "linear.comment_on_issue".into(),
            label: "Comment on Linear issue".into(),
            description: "Add a comment to an issue in the connected Linear workspace.".into(),
            fields: vec![
                resource_field(
                    "issue",
                    "Issue",
                    "An issue in the connected workspace.",
                    "issues",
                ),
                textarea_field("body", "Comment", "Comment in Linear Markdown.", true),
            ],
            required_scopes: vec![SCOPE_ISSUES_READ.into(), SCOPE_COMMENTS_WRITE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "linear".into(),
            action_id: "linear.update_issue_status".into(),
            label: "Update Linear issue status".into(),
            description: "Move an issue to another workflow state of its team.".into(),
            fields: vec![
                resource_field(
                    "issue",
                    "Issue",
                    "An issue in the connected workspace.",
                    "issues",
                ),
                resource_field(
                    "status",
                    "Status",
                    "A workflow state from the issue's team.",
                    "states",
                ),
            ],
            required_scopes: vec![SCOPE_ISSUES_READ.into(), SCOPE_ISSUES_WRITE.into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        },
        ActionDescriptor {
            provider_id: "linear".into(),
            action_id: "linear.get_issue".into(),
            label: "Get Linear issue".into(),
            description: "Fetch bounded issue context for a later workflow step.".into(),
            fields: vec![resource_field(
                "issue",
                "Issue",
                "An issue in the connected workspace.",
                "issues",
            )],
            required_scopes: vec![SCOPE_ISSUES_READ.into()],
            output_schema_version: 1,
            output_is_untrusted: true,
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
        default: (!required).then(|| Value::String(String::new())),
        secret: false,
        option_source: None,
        options: vec![],
        supports_interpolation: true,
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

impl ActionExecutor for LinearService {
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
                "linear.create_issue" => {
                    let team_id = required_linear_id(&request.input, "team")?;
                    let title = required_bounded_text(&request.input, "title", 256)?;
                    let description = optional_bounded_text(
                        &request.input,
                        "description",
                        LINEAR_MAX_TEXT_CHARS,
                    )?;
                    let priority = linear_priority(
                        required_bounded_text(&request.input, "priority", 32)?.as_str(),
                    )?;
                    let assignee_id = optional_linear_id(&request.input, "assignee")?;
                    let label_ids = self
                        .resolve_label_ids(token.as_str(), &team_id, &request.input)
                        .await?;
                    if cancellation.is_cancelled() {
                        return Err(ActionError::new(ActionErrorCode::Cancelled));
                    }
                    let variables = serde_json::json!({
                        "input": {
                            "teamId": team_id.clone(),
                            "title": title,
                            "description": marked_body(&description),
                            "priority": priority,
                            "assigneeId": assignee_id,
                            "labelIds": label_ids,
                        }
                    });
                    let response = self
                        .graphql(token.as_str(), ISSUE_CREATE_QUERY, Some(&variables), true)
                        .await?;
                    let payload = response
                        .get("data")
                        .and_then(|data| data.get("issueCreate"))
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    if payload.get("success").and_then(Value::as_bool) != Some(true) {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    let issue = payload
                        .get("issue")
                        .and_then(Value::as_object)
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let (issue_id, identifier, url, state) = issue_summary(issue)?;
                    let artifact_label = format!("Linear {identifier}");
                    let artifact_uri = url.clone();
                    Ok(ActionResult {
                        summary: format!("Created Linear issue {identifier}"),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "teamId": team_id,
                            "issueId": issue_id,
                            "identifier": identifier,
                            "state": state,
                            "url": url,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: artifact_label,
                            uri: artifact_uri,
                        }],
                        provider_request_id: None,
                    })
                }
                "linear.comment_on_issue" => {
                    let issue_id = required_linear_id(&request.input, "issue")?;
                    let body =
                        required_bounded_text(&request.input, "body", LINEAR_MAX_TEXT_CHARS)?;
                    let variables = serde_json::json!({
                        "input": {
                            "issueId": issue_id.clone(),
                            "body": marked_body(&body),
                        }
                    });
                    let response = self
                        .graphql(token.as_str(), COMMENT_CREATE_QUERY, Some(&variables), true)
                        .await?;
                    let payload = response
                        .get("data")
                        .and_then(|data| data.get("commentCreate"))
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    if payload.get("success").and_then(Value::as_bool) != Some(true) {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    let comment = payload
                        .get("comment")
                        .and_then(Value::as_object)
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let comment_id = comment
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|value| valid_linear_id(value))
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let url = comment
                        .get("url")
                        .and_then(Value::as_str)
                        .filter(|value| valid_linear_url(value))
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let created_at = comment
                        .get("createdAt")
                        .and_then(Value::as_str)
                        .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    Ok(ActionResult {
                        summary: format!("Commented on Linear issue {issue_id}"),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "issueId": issue_id,
                            "commentId": comment_id,
                            "url": url,
                            "createdAt": created_at,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: "Linear comment".into(),
                            uri: url.into(),
                        }],
                        provider_request_id: None,
                    })
                }
                "linear.update_issue_status" => {
                    let issue_id = required_linear_id(&request.input, "issue")?;
                    let state_id = required_linear_id(&request.input, "status")?;
                    let variables = serde_json::json!({
                        "id": issue_id.clone(),
                        "input": { "stateId": state_id },
                    });
                    let response = self
                        .graphql(token.as_str(), ISSUE_UPDATE_QUERY, Some(&variables), true)
                        .await?;
                    let payload = response
                        .get("data")
                        .and_then(|data| data.get("issueUpdate"))
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    if payload.get("success").and_then(Value::as_bool) != Some(true) {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    let issue = payload
                        .get("issue")
                        .and_then(Value::as_object)
                        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
                    let (returned_id, identifier, url, state) = issue_summary(issue)?;
                    if returned_id != issue_id {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    let artifact_label = format!("Linear {identifier}");
                    let artifact_uri = url.clone();
                    Ok(ActionResult {
                        summary: format!("Updated Linear {identifier} to {state}"),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "issueId": returned_id,
                            "identifier": identifier,
                            "state": state,
                            "url": url,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: artifact_label,
                            uri: artifact_uri,
                        }],
                        provider_request_id: None,
                    })
                }
                "linear.get_issue" => {
                    let issue_id = required_linear_id(&request.input, "issue")?;
                    let variables = serde_json::json!({ "id": issue_id.clone() });
                    let response = self
                        .graphql(token.as_str(), ISSUE_GET_QUERY, Some(&variables), false)
                        .await?;
                    let issue = response
                        .get("data")
                        .and_then(|data| data.get("issue"))
                        .and_then(Value::as_object)
                        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
                    let context = linear_issue_context(issue)?;
                    if context.id != issue_id {
                        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
                    }
                    Ok(ActionResult {
                        summary: format!("Fetched Linear {}", context.identifier),
                        output: serde_json::json!({
                            "schemaVersion": 1,
                            "issueId": context.id,
                            "identifier": context.identifier,
                            "title": context.title,
                            "description": context.description,
                            "state": context.state,
                            "url": context.url,
                            "team": context.team,
                            "project": context.project,
                            "assignee": context.assignee,
                            "labels": context.labels,
                            "priority": context.priority,
                            "createdAt": context.created_at,
                            "updatedAt": context.updated_at,
                        }),
                        artifacts: vec![ActionArtifact {
                            kind: "url".into(),
                            label: format!("Linear {}", context.identifier),
                            uri: context.url,
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
            if cancellation.is_cancelled() {
                return Err(ActionError::new(ActionErrorCode::Cancelled));
            }
            let token = Zeroizing::new(
                tokens.with_credential(|credential| credential.access_token.clone())?,
            );
            let cursor = match page_token {
                Some(token) => decode_page_cursor(token)?,
                None => None,
            };
            let page = match source {
                "teams" => {
                    self.list_teams(token.as_str(), query, cursor.as_deref())
                        .await?
                }
                "assignees" => {
                    self.list_users(token.as_str(), query, cursor.as_deref())
                        .await?
                }
                "states" => {
                    self.list_states(token.as_str(), query, cursor.as_deref())
                        .await?
                }
                "issues" => {
                    self.list_issues(token.as_str(), query, cursor.as_deref())
                        .await?
                }
                _ => return Err(ActionError::new(ActionErrorCode::InvalidInput)),
            };
            let next_page_token = page
                .next_cursor
                .map(|cursor| encode_page_cursor(&cursor))
                .transpose()?;
            Ok(ActionResourcePage {
                items: page
                    .items
                    .into_iter()
                    .map(|item| ActionResourceItem {
                        id: item.id,
                        label: item.label,
                    })
                    .collect(),
                next_page_token,
            })
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PageCursor {
    #[serde(default)]
    cursor: String,
}

struct LinearResourcePage {
    items: Vec<AppEventResourceItem>,
    next_cursor: Option<String>,
}

fn encode_page_cursor(cursor: &str) -> Result<String, ActionError> {
    let value = serde_json::to_vec(&PageCursor {
        cursor: cursor.to_owned(),
    })
    .map(|value| URL_SAFE_NO_PAD.encode(value))
    .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?;
    Ok(value)
}

fn decode_page_cursor(value: &str) -> Result<Option<String>, ActionError> {
    let cursor = URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PageCursor>(&bytes).ok())
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
    let cursor = cursor.cursor;
    if cursor.is_empty() {
        return Ok(None);
    }
    if cursor.len() > 512
        || cursor
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\0')
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(Some(cursor))
}

const TEAMS_QUERY: &str = "query LinearTeams($first: Int!, $after: String, $filter: TeamFilter) { teams(first: $first, after: $after, filter: $filter) { nodes { id name } pageInfo { hasNextPage endCursor } } }";

const USERS_QUERY: &str = "query LinearUsers($first: Int!, $after: String, $filter: UserFilter) { users(first: $first, after: $after, filter: $filter) { nodes { id name } pageInfo { hasNextPage endCursor } } }";

const STATES_QUERY: &str = "query LinearStates($first: Int!, $after: String) { workflowStates(first: $first, after: $after) { nodes { id name team { id name } } pageInfo { hasNextPage endCursor } } }";

const ISSUES_QUERY: &str = "query LinearIssues($first: Int!, $after: String, $filter: IssueFilter) { issues(first: $first, after: $after, filter: $filter, orderBy: updatedAt) { nodes { id identifier title url } pageInfo { hasNextPage endCursor } } }";

const ISSUE_CREATE_QUERY: &str = "mutation LinearIssueCreate($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier title url state { name } } } }";

const COMMENT_CREATE_QUERY: &str = "mutation LinearCommentCreate($input: CommentCreateInput!) { commentCreate(input: $input) { success comment { id url createdAt } } }";

const ISSUE_UPDATE_QUERY: &str = "mutation LinearIssueUpdate($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success issue { id identifier url state { name } } } }";

const ISSUE_GET_QUERY: &str = "query LinearIssue($id: String!) { issue(id: $id) { id identifier title description url state { name } team { id name } project { id name } assignee { name } labels { nodes { name } } priority createdAt updatedAt } }";

const TEAM_ISSUES_QUERY: &str = "query LinearTeamIssues($first: Int!, $filter: IssueFilter) { issues(first: $first, orderBy: updatedAt, filter: $filter) { nodes { id identifier title description url createdAt updatedAt state { name } team { id } project { id name } assignee { name } } pageInfo { hasNextPage } } }";

const TEAM_LABELS_QUERY: &str =
    "query LinearTeamLabels($id: String!) { team(id: $id) { labels { nodes { id name } } } }";

impl LinearService {
    async fn list_teams(
        &self,
        token: &str,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<LinearResourcePage, ActionError> {
        let variables = serde_json::json!({
            "first": LINEAR_PAGE_SIZE,
            "after": cursor,
            "filter": if query.trim().is_empty() { Value::Null } else {
                serde_json::json!({ "name": { "containsIgnoreCase": query.trim() } })
            },
        });
        let response = self
            .graphql(token, TEAMS_QUERY, Some(&variables), false)
            .await?;
        let nodes = paginated_nodes(&response, "teams")?;
        let items = nodes
            .into_iter()
            .filter_map(|node| {
                Some(AppEventResourceItem {
                    id: node
                        .get("id")?
                        .as_str()
                        .filter(|id| valid_linear_id(id))?
                        .to_owned(),
                    label: node
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| bounded(name, 200))
                        .filter(|name| !name.is_empty())?,
                })
            })
            .collect();
        Ok(LinearResourcePage {
            items,
            next_cursor: paginated_end_cursor(&response, "teams"),
        })
    }

    async fn list_users(
        &self,
        token: &str,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<LinearResourcePage, ActionError> {
        let variables = serde_json::json!({
            "first": LINEAR_PAGE_SIZE,
            "after": cursor,
            "filter": if query.trim().is_empty() { Value::Null } else {
                serde_json::json!({ "name": { "containsIgnoreCase": query.trim() } })
            },
        });
        let response = self
            .graphql(token, USERS_QUERY, Some(&variables), false)
            .await?;
        let nodes = paginated_nodes(&response, "users")?;
        let items = nodes
            .into_iter()
            .filter_map(|node| {
                Some(AppEventResourceItem {
                    id: node
                        .get("id")?
                        .as_str()
                        .filter(|id| valid_linear_id(id))?
                        .to_owned(),
                    label: node
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| bounded(name, 200))
                        .filter(|name| !name.is_empty())?,
                })
            })
            .collect();
        Ok(LinearResourcePage {
            items,
            next_cursor: paginated_end_cursor(&response, "users"),
        })
    }

    async fn list_states(
        &self,
        token: &str,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<LinearResourcePage, ActionError> {
        let variables = serde_json::json!({ "first": LINEAR_PAGE_SIZE, "after": cursor });
        let response = self
            .graphql(token, STATES_QUERY, Some(&variables), false)
            .await?;
        let nodes = paginated_nodes(&response, "workflowStates")?;
        let query = query.trim().to_ascii_lowercase();
        let items = nodes
            .into_iter()
            .filter_map(|node| {
                let id = node
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| valid_linear_id(id))?
                    .to_owned();
                let name = node
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|name| bounded(name, 200))
                    .filter(|name| !name.is_empty())?;
                let team = node
                    .get("team")
                    .and_then(|team| team.get("name"))
                    .and_then(Value::as_str)
                    .map(|name| bounded(name, 200))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Team".into());
                let label = format!("{name} · {team}");
                if !query.is_empty()
                    && !name.to_ascii_lowercase().contains(&query)
                    && !team.to_ascii_lowercase().contains(&query)
                {
                    return None;
                }
                Some(AppEventResourceItem { id, label })
            })
            .collect();
        Ok(LinearResourcePage {
            items,
            next_cursor: paginated_end_cursor(&response, "workflowStates"),
        })
    }

    async fn list_issues(
        &self,
        token: &str,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<LinearResourcePage, ActionError> {
        let trimmed = query.trim();
        let filter = if trimmed.is_empty() {
            Value::Null
        } else if valid_linear_identifier(trimmed) {
            serde_json::json!({ "identifier": { "eq": trimmed.to_ascii_uppercase() } })
        } else {
            serde_json::json!({ "title": { "containsIgnoreCase": trimmed } })
        };
        let variables =
            serde_json::json!({ "first": LINEAR_PAGE_SIZE, "after": cursor, "filter": filter });
        let response = self
            .graphql(token, ISSUES_QUERY, Some(&variables), false)
            .await?;
        let nodes = paginated_nodes(&response, "issues")?;
        let items = nodes
            .into_iter()
            .filter_map(|node| {
                let id = node
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| valid_linear_id(id))?
                    .to_owned();
                let identifier = node
                    .get("identifier")
                    .and_then(Value::as_str)
                    .filter(|value| valid_linear_identifier(value))?;
                let title = node
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|title| bounded(title, 200))
                    .filter(|title| !title.is_empty())?;
                Some(AppEventResourceItem {
                    id,
                    label: format!("{identifier}: {title}"),
                })
            })
            .collect();
        Ok(LinearResourcePage {
            items,
            next_cursor: paginated_end_cursor(&response, "issues"),
        })
    }

    async fn resolve_label_ids(
        &self,
        token: &str,
        team_id: &str,
        input: &BTreeMap<String, Value>,
    ) -> Result<Option<Vec<String>>, ActionError> {
        let raw = optional_bounded_text(input, "labels", 2_000)?;
        let names = raw
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Ok(None);
        }
        if names.len() > 20 || names.iter().any(|name| name.chars().count() > 100) {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let variables = serde_json::json!({ "id": team_id });
        let response = self
            .graphql(token, TEAM_LABELS_QUERY, Some(&variables), false)
            .await?;
        let labels = response
            .get("data")
            .and_then(|data| data.get("team"))
            .and_then(|team| team.get("labels"))
            .and_then(|labels| labels.get("nodes"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut by_name = HashMap::new();
        for label in labels {
            let Some(name) = label.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = label
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_linear_id(id))
            else {
                continue;
            };
            by_name
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| id.to_owned());
        }
        let mut ids = Vec::new();
        for name in &names {
            let id = by_name
                .get(&name.to_ascii_lowercase())
                .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
            ids.push(id.clone());
        }
        Ok(Some(ids))
    }
}

fn paginated_nodes<'a>(
    response: &'a Value,
    collection: &str,
) -> Result<Vec<&'a Value>, ActionError> {
    response
        .get("data")
        .and_then(|data| data.get(collection))
        .and_then(|collection| collection.get("nodes"))
        .and_then(Value::as_array)
        .map(|nodes| nodes.iter().collect())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))
}

fn paginated_end_cursor(response: &Value, collection: &str) -> Option<String> {
    let page_info = response.get("data")?.get(collection)?.get("pageInfo")?;
    let has_more = page_info.get("hasNextPage")?.as_bool()?;
    if !has_more {
        return None;
    }
    page_info
        .get("endCursor")?
        .as_str()
        .filter(|cursor| {
            !cursor.is_empty()
                && cursor.len() <= 512
                && !cursor
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte == b'\0')
        })
        .map(str::to_owned)
}

fn issue_summary(
    issue: &serde_json::Map<String, Value>,
) -> Result<(String, String, String, String), ActionError> {
    let id = issue
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| valid_linear_id(value))
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let identifier = issue
        .get("identifier")
        .and_then(Value::as_str)
        .filter(|value| valid_linear_identifier(value))
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let url = issue
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| valid_linear_url(value))
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let state = issue
        .get("state")
        .and_then(|state| state.get("name"))
        .and_then(Value::as_str)
        .map(|name| bounded(name, 128))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    Ok((id.into(), identifier.into(), url.into(), state))
}

#[derive(Default)]
struct LinearIssueContext {
    id: String,
    identifier: String,
    title: String,
    description: Option<String>,
    state: String,
    url: String,
    team: Option<String>,
    project: Option<String>,
    assignee: Option<String>,
    labels: Vec<String>,
    priority: Option<u64>,
    created_at: String,
    updated_at: String,
}

fn linear_issue_context(
    issue: &serde_json::Map<String, Value>,
) -> Result<LinearIssueContext, ActionError> {
    let (id, identifier, url, state) = issue_summary(issue)?;
    let title = issue
        .get("title")
        .and_then(Value::as_str)
        .map(|title| bounded(title, 512))
        .filter(|title| !title.is_empty())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let description = issue
        .get("description")
        .and_then(Value::as_str)
        .map(|description| bounded(description, LINEAR_MAX_CONTEXT_CHARS));
    let team = issue
        .get("team")
        .and_then(|team| team.get("name"))
        .and_then(Value::as_str)
        .map(|name| bounded(name, 200))
        .filter(|name| !name.is_empty());
    let project = issue
        .get("project")
        .and_then(|project| project.get("name"))
        .and_then(Value::as_str)
        .map(|name| bounded(name, 200))
        .filter(|name| !name.is_empty());
    let assignee = issue
        .get("assignee")
        .and_then(|assignee| assignee.get("name"))
        .and_then(Value::as_str)
        .map(|name| bounded(name, 200))
        .filter(|name| !name.is_empty());
    let labels = issue
        .get("labels")
        .and_then(|labels| labels.get("nodes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|label| label.get("name").and_then(Value::as_str))
        .map(|name| bounded(name, 200))
        .take(20)
        .collect();
    let priority = issue.get("priority").and_then(Value::as_u64);
    let created_at = issue
        .get("createdAt")
        .and_then(Value::as_str)
        .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    let updated_at = issue
        .get("updatedAt")
        .and_then(Value::as_str)
        .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
        .ok_or_else(|| ActionError::new(ActionErrorCode::OutputInvalid))?;
    Ok(LinearIssueContext {
        id,
        identifier,
        title,
        description,
        state,
        url,
        team,
        project,
        assignee,
        labels,
        priority,
        created_at: created_at.into(),
        updated_at: updated_at.into(),
    })
}

fn event_descriptors() -> Vec<AppEventDescriptor> {
    vec![AppEventDescriptor {
        provider_id: "linear".into(),
        event_type: "linear.issue_activity".into(),
        label: "Linear issue activity".into(),
        description: "Run when an issue is created or updated in a selected team.".into(),
        required_scopes: vec![SCOPE_ISSUES_READ.into()],
        delivery_modes: vec![AppEventDeliveryMode::Polling],
        filter_fields: vec![
            resource_field(
                "teamId",
                "Team",
                "A team in the connected Linear workspace.",
                "teams",
            ),
            enum_field(
                "action",
                "Action",
                "Optionally limit which Linear activity starts the workflow.",
                "any",
                &[
                    ("any", "Any supported action"),
                    ("created", "Issue created"),
                    ("updated", "Issue updated"),
                ],
            ),
        ],
        fetches_resource_content: false,
        descriptor_version: 1,
        external_event_id_required: true,
        allowed_attribute_keys: vec![
            "teamId".into(),
            "identifier".into(),
            "action".into(),
            "status".into(),
            "projectId".into(),
        ],
        poll_interval_seconds: 60,
        pending_cap: 100,
    }]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LinearRecentIssue {
    id: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LinearEventCursor {
    #[serde(default)]
    recent: Vec<LinearRecentIssue>,
    #[serde(default)]
    watermark: String,
}

#[derive(Clone, Deserialize)]
struct LinearPollIssue {
    id: String,
    identifier: String,
    title: String,
    description: Option<String>,
    url: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    state: Option<LinearPollState>,
    team: Option<LinearPollTeam>,
    project: Option<LinearPollProject>,
    assignee: Option<LinearPollAssignee>,
}

#[derive(Clone, Deserialize)]
struct LinearPollState {
    name: String,
}

#[derive(Clone, Deserialize)]
struct LinearPollTeam {
    id: String,
}

#[derive(Clone, Deserialize)]
struct LinearPollProject {
    id: String,
}

#[derive(Clone, Deserialize)]
struct LinearPollAssignee {
    name: String,
}

impl AppEventAdapter for LinearService {
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
            let team_id = config
                .filters
                .get("teamId")
                .and_then(Value::as_str)
                .filter(|value| valid_linear_id(value))
                .ok_or_else(|| AppEventError::new(AppEventErrorCode::InvalidInput))?
                .to_owned();
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let variables = serde_json::json!({
                "first": LINEAR_PAGE_SIZE,
                "filter": { "team": { "id": { "eq": team_id.clone() } } },
            });
            let response = self
                .graphql(token.as_str(), TEAM_ISSUES_QUERY, Some(&variables), false)
                .await
                .map_err(map_action_error_to_event)?;
            let nodes = response
                .get("data")
                .and_then(|data| data.get("issues"))
                .and_then(|issues| issues.get("nodes"))
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
            let issues = nodes
                .into_iter()
                .filter_map(|node| serde_json::from_value::<LinearPollIssue>(node).ok())
                .filter(valid_poll_issue)
                .collect::<Vec<_>>();
            if issues.len() > LINEAR_MAX_ISSUES_RECENT
                || issues.iter().any(|issue| !valid_linear_id(&issue.id))
            {
                return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
            }
            let page_watermark = issues
                .iter()
                .filter_map(|issue| DateTime::parse_from_rfc3339(&issue.updated_at).ok())
                .max()
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            let recent = issues
                .iter()
                .map(|issue| LinearRecentIssue {
                    id: issue.id.clone(),
                    updated_at: issue.updated_at.clone(),
                })
                .collect::<Vec<_>>();
            let next_cursor = encode_event_cursor(&LinearEventCursor {
                watermark: page_watermark.clone(),
                recent,
            })?;
            let Some(cursor) = cursor else {
                // Connecting a trigger establishes "now" and does not replay
                // the team's recent activity history.
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
            let seen = prior
                .recent
                .into_iter()
                .map(|entry| issue_key(&entry.id, &entry.updated_at))
                .collect::<HashSet<_>>();
            let mut normalized = Vec::new();
            for issue in issues.into_iter().rev() {
                if cancellation.is_cancelled() {
                    return Err(AppEventError::new(AppEventErrorCode::Cancelled));
                }
                if seen.contains(&issue_key(&issue.id, &issue.updated_at)) {
                    continue;
                }
                let action = if DateTime::parse_from_rfc3339(&issue.created_at)
                    .ok()
                    .is_some_and(|created| created >= prior_watermark)
                {
                    "created"
                } else {
                    "updated"
                };
                if action == "created"
                    && issue
                        .description
                        .as_deref()
                        .is_some_and(|description| description.contains(LINEAR_ACTION_MARKER))
                {
                    continue;
                }
                if let Some(event) =
                    normalize_linear_issue(config, team_id.as_str(), &issue, action)?
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
        page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async move {
            if field_key != "teamId" || cancellation.is_cancelled() {
                return Err(AppEventError::new(AppEventErrorCode::InvalidInput));
            }
            let token = Zeroizing::new(
                tokens
                    .with_credential(|credential| credential.access_token.clone())
                    .map_err(map_action_error_to_event)?,
            );
            let cursor = match page_token {
                Some(token) => decode_page_cursor(token)
                    .map_err(|_| AppEventError::new(AppEventErrorCode::InvalidInput))?,
                None => None,
            };
            let page = self
                .list_teams(token.as_str(), query, cursor.as_deref())
                .await
                .map_err(map_action_error_to_event)?;
            Ok(AppEventResourcePage {
                items: page.items,
                next_page_token: page
                    .next_cursor
                    .map(|cursor| encode_page_cursor(&cursor))
                    .transpose()
                    .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))?,
            })
        })
    }
}

fn issue_key(id: &str, updated_at: &str) -> String {
    format!("{id}@{updated_at}")
}

fn encode_event_cursor(cursor: &LinearEventCursor) -> Result<String, AppEventError> {
    serde_json::to_vec(cursor)
        .map(|value| URL_SAFE_NO_PAD.encode(value))
        .map_err(|_| AppEventError::new(AppEventErrorCode::EventInvalid))
}

fn decode_event_cursor(value: &str) -> Result<LinearEventCursor, AppEventError> {
    let cursor = URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LinearEventCursor>(&bytes).ok())
        .filter(valid_event_cursor)
        .ok_or_else(|| AppEventError::new(AppEventErrorCode::EventInvalid))?;
    Ok(cursor)
}

fn valid_event_cursor(cursor: &LinearEventCursor) -> bool {
    DateTime::parse_from_rfc3339(&cursor.watermark).is_ok()
        && cursor.recent.len() <= LINEAR_MAX_ISSUES_RECENT
        && cursor.recent.iter().all(|entry| {
            valid_linear_id(&entry.id) && DateTime::parse_from_rfc3339(&entry.updated_at).is_ok()
        })
}

fn valid_poll_issue(issue: &LinearPollIssue) -> bool {
    valid_linear_id(&issue.id)
        && valid_linear_identifier(&issue.identifier)
        && !issue.title.is_empty()
        && issue.title.chars().count() <= 512
        && DateTime::parse_from_rfc3339(&issue.created_at).is_ok()
        && DateTime::parse_from_rfc3339(&issue.updated_at).is_ok()
        && issue.url.as_deref().is_none_or(valid_linear_url)
        && issue
            .team
            .as_ref()
            .is_some_and(|team| valid_linear_id(&team.id))
        && issue
            .state
            .as_ref()
            .is_some_and(|state| !state.name.is_empty() && state.name.chars().count() <= 128)
}

fn normalize_linear_issue(
    config: &AppTriggerConfig,
    team_id: &str,
    issue: &LinearPollIssue,
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
    if issue.team.as_ref().map(|team| team.id.as_str()) != Some(team_id) {
        return Err(AppEventError::new(AppEventErrorCode::EventInvalid));
    }
    let status = issue
        .state
        .as_ref()
        .map(|state| bounded(&state.name, 128))
        .filter(|status| !status.is_empty());
    let mut attributes = BTreeMap::from([
        ("teamId".into(), Value::String(team_id.to_owned())),
        ("identifier".into(), Value::String(issue.identifier.clone())),
        ("action".into(), Value::String(action.into())),
    ]);
    if let Some(status) = status {
        attributes.insert("status".into(), Value::String(status));
    }
    if let Some(project) = &issue.project {
        if valid_linear_id(&project.id) {
            attributes.insert("projectId".into(), Value::String(project.id.clone()));
        }
    }
    let actor = issue
        .assignee
        .as_ref()
        .map(|assignee| bounded(&assignee.name, 200))
        .filter(|name| !name.is_empty());
    Ok(Some(NormalizedAppEvent {
        schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
        provider_id: "linear".into(),
        event_type: config.event_type.clone(),
        connection_id: config.connection_id.clone(),
        external_event_id: issue_key(&issue.id, &issue.updated_at),
        occurred_at: issue.updated_at.clone(),
        subject: Some(bounded(&issue.title, 512)),
        actor,
        resource_url: issue.url.clone().filter(|url| valid_linear_url(url)),
        preview: None,
        attributes,
    }))
}

fn linear_priority(value: &str) -> Result<Option<u64>, ActionError> {
    match value {
        "no_priority" => Ok(None),
        "urgent" => Ok(Some(1)),
        "high" => Ok(Some(2)),
        "medium" => Ok(Some(3)),
        "low" => Ok(Some(4)),
        _ => Err(ActionError::new(ActionErrorCode::InvalidInput)),
    }
}

fn required_linear_id(input: &BTreeMap<String, Value>, key: &str) -> Result<String, ActionError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| valid_linear_id(value))
        .map(str::to_owned)
        .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))
}

fn optional_linear_id(
    input: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<String>, ActionError> {
    let Some(value) = input.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().map(str::trim).unwrap_or_default();
    if value.is_empty() {
        return Ok(None);
    }
    if valid_linear_id(value) {
        Ok(Some(value.to_owned()))
    } else {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    }
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

fn marked_body(body: &str) -> String {
    if body.is_empty() {
        LINEAR_ACTION_MARKER.into()
    } else {
        format!("{body}\n\n{LINEAR_ACTION_MARKER}")
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

fn valid_linear_id(value: &str) -> bool {
    (16..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_linear_identifier(value: &str) -> bool {
    let mut parts = value.splitn(2, '-');
    let Some(team_key) = parts.next() else {
        return false;
    };
    let Some(number) = parts.next() else {
        return false;
    };
    !team_key.is_empty()
        && team_key.len() <= 10
        && team_key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && !number.is_empty()
        && number.len() <= 10
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_linear_url(value: &str) -> bool {
    Url::parse(value)
        .ok()
        .is_some_and(|url| url.scheme() == "https" && url.host_str() == Some("linear.app"))
}

fn validate_api_key(token: &str) -> Result<(), IntegrationCommandError> {
    let valid = token.starts_with("lin_")
        && token.len() >= 30
        && token.len() <= 512
        && !token.chars().any(char::is_whitespace)
        && !token.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(command_error(
            "linear_token_invalid",
            "Enter a valid Linear personal API key beginning with lin_.",
            false,
        ))
    }
}

async fn parse_linear_response(response: Response, mutation: bool) -> Result<Value, ActionError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = retry_after_seconds(&response);
        let code = match status {
            StatusCode::UNAUTHORIZED => ActionErrorCode::ProviderUnauthorized,
            StatusCode::FORBIDDEN => ActionErrorCode::ScopeMissing,
            StatusCode::TOO_MANY_REQUESTS => ActionErrorCode::RateLimited,
            StatusCode::BAD_REQUEST
            | StatusCode::NOT_FOUND
            | StatusCode::CONFLICT
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
        .is_some_and(|length| length as usize > LINEAR_RESPONSE_LIMIT)
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
        if bytes.len().saturating_add(chunk.len()) > LINEAR_RESPONSE_LIMIT {
            return Err(ActionError::new(if mutation {
                ActionErrorCode::DeliveryUnknown
            } else {
                ActionErrorCode::OutputTooLarge
            }));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        ActionError::new(if mutation {
            ActionErrorCode::DeliveryUnknown
        } else {
            ActionErrorCode::OutputInvalid
        })
    })?;
    if let Some(errors) = value.get("errors").and_then(Value::as_array) {
        return Err(classify_graphql_errors(errors));
    }
    if value.get("data").is_none() || value.get("data") == Some(&Value::Null) {
        return Err(ActionError::new(if mutation {
            ActionErrorCode::DeliveryUnknown
        } else {
            ActionErrorCode::OutputInvalid
        }));
    }
    Ok(value)
}

fn classify_graphql_errors(errors: &[Value]) -> ActionError {
    for error in errors {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let code = error
            .get("extensions")
            .and_then(|extensions| extensions.get("code"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let combined = format!("{code} {message}");
        if combined.contains("rate") || combined.contains("complexity") {
            return ActionError::new(ActionErrorCode::RateLimited);
        }
        if combined.contains("unauthenticated")
            || combined.contains("invalid authentication")
            || combined.contains("authentication required")
        {
            return ActionError::new(ActionErrorCode::ProviderUnauthorized);
        }
        if combined.contains("forbidden")
            || combined.contains("permission")
            || combined.contains("not authorized")
        {
            return ActionError::new(ActionErrorCode::ScopeMissing);
        }
    }
    // A 200 with an errors array is a deterministic provider-side refusal,
    // never an ambiguous outcome.
    ActionError::new(ActionErrorCode::InvalidInput)
}

fn retry_after_seconds(response: &Response) -> Option<u64> {
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

fn map_connect_action_error(error: ActionError) -> IntegrationCommandError {
    match error.code {
        ActionErrorCode::ProviderUnauthorized => command_error(
            "linear_token_invalid",
            "Linear rejected this personal API key.",
            false,
        ),
        ActionErrorCode::RateLimited => command_error(
            "rate_limited",
            "Linear is rate limiting connection checks. Try again later.",
            true,
        ),
        _ => command_error(
            "linear_connection_failed",
            "Linear could not validate this personal API key.",
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
        _ => linear_credential_error(),
    }
}

fn command_error(code: &str, message: &str, recoverable: bool) -> IntegrationCommandError {
    IntegrationCommandError::new(code, message, recoverable)
}

fn linear_identity_error() -> IntegrationCommandError {
    command_error(
        "linear_identity_invalid",
        "Linear did not return a valid user and workspace identity.",
        false,
    )
}

fn linear_store_error() -> IntegrationCommandError {
    command_error(
        "connection_store_failed",
        "Linear was validated, but the connection metadata could not be saved.",
        true,
    )
}

fn linear_credential_error() -> IntegrationCommandError {
    command_error(
        "linear_connection_failed",
        "Linear was validated, but its credential could not be saved.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::ConnectionStatus;
    use crate::integrations::token_store::InMemoryTokenStore;
    use tiny_http::{Header, Response as TinyResponse, Server};

    fn test_service(base: String) -> LinearService {
        LinearService::new(&base).expect("test service")
    }

    fn connection() -> AppConnection {
        AppConnection {
            id: "connection".into(),
            provider_id: "linear".into(),
            display_name: Some("Workspace".into()),
            external_account_id: Some("viewer".into()),
            external_tenant_id: Some("org".into()),
            connection_mode: "personal_token".into(),
            identity_key: "identity".into(),
            scopes: vec![
                SCOPE_WORKSPACE_READ.into(),
                SCOPE_ISSUES_READ.into(),
                SCOPE_ISSUES_WRITE.into(),
                SCOPE_COMMENTS_WRITE.into(),
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
                &CredentialEnvelope::new("lin_secret_fixture".into()),
            )
            .expect("credential");
        TokenAccessCapability::load(store, "credential".into())
            .await
            .expect("token capability")
    }

    fn json_header() -> Header {
        Header::from_bytes("Content-Type", "application/json").expect("header")
    }

    fn read_body(request: &mut tiny_http::Request) -> String {
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).expect("body");
        body
    }

    #[test]
    fn descriptors_are_secret_free_and_scoped_to_linear() {
        let descriptors = action_descriptors();
        assert_eq!(descriptors.len(), 4);
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.provider_id == "linear"));
        assert!(descriptors
            .iter()
            .flat_map(|descriptor| descriptor.fields.iter())
            .all(|field| !field.secret));
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.action_id.starts_with("linear.")));
        assert!(descriptors
            .iter()
            .any(|descriptor| descriptor.action_id == "linear.create_issue"
                && descriptor.fields.iter().any(|field| {
                    field.key == "team" && field.option_source.as_deref() == Some("teams")
                })));
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
    fn linear_ids_identifiers_and_cursors_validate_strictly() {
        assert!(valid_linear_id("9cfb482a-81e3-4154-b5b9-2c805e70a02d"));
        assert!(!valid_linear_id("../etc/passwd"));
        assert!(valid_linear_identifier("ENG-123"));
        assert!(valid_linear_identifier("ABC1-9999999999"));
        assert!(!valid_linear_identifier("eng-123"));
        assert!(!valid_linear_identifier("ENG"));
        assert!(valid_linear_url("https://linear.app/eng/issue/ENG-1/x"));
        assert!(!valid_linear_url("https://evil.example/issue"));

        let cursor = LinearEventCursor {
            recent: vec![LinearRecentIssue {
                id: "9cfb482a-81e3-4154-b5b9-2c805e70a02d".into(),
                updated_at: "2026-08-17T10:00:00Z".into(),
            }],
            watermark: "2026-08-17T10:00:00Z".into(),
        };
        let encoded = encode_event_cursor(&cursor).expect("encode");
        assert!(encoded.len() < 1024);
        let decoded = decode_event_cursor(&encoded).expect("decode");
        assert_eq!(decoded.watermark, "2026-08-17T10:00:00Z");
        assert_eq!(decoded.recent.len(), 1);
        assert!(decode_event_cursor(&URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({ "recent": [{ "id": "bad id", "updatedAt": "now" }], "watermark": "now" }))
                .unwrap(),
        ))
        .is_err());
    }

    #[test]
    fn action_inputs_validate_ids_priority_and_bounded_text() {
        let mut input = BTreeMap::from([
            (
                "team".into(),
                Value::String("9cfb482a-81e3-4154-b5b9-2c805e70a02d".into()),
            ),
            (
                "issue".into(),
                Value::String("9cfb482a-81e3-4154-b5b9-2c805e70a02d".into()),
            ),
            ("priority".into(), Value::String("high".into())),
            ("title".into(), Value::String("Release blocker".into())),
        ]);
        assert_eq!(
            required_linear_id(&input, "team").unwrap(),
            "9cfb482a-81e3-4154-b5b9-2c805e70a02d"
        );
        assert_eq!(linear_priority("high").unwrap(), Some(2));
        assert_eq!(linear_priority("no_priority").unwrap(), None);
        assert!(linear_priority("critical").is_err());
        assert_eq!(
            required_bounded_text(&input, "title", 256).unwrap(),
            "Release blocker"
        );
        input.insert("team".into(), Value::String("not-an-id".into()));
        assert!(required_linear_id(&input, "team").is_err());
        input.insert("assignee".into(), Value::String(String::new()));
        assert_eq!(optional_linear_id(&input, "assignee").unwrap(), None);
    }

    #[tokio::test]
    async fn connect_saves_only_validated_workspace_identity() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut request = server.recv().expect("request");
            assert_eq!(request.url(), "/graphql");
            let body = read_body(&mut request);
            assert!(body.contains("LinearValidate"));
            assert!(!body.contains("lin_secret_fixture"));
            request
                .respond(
                    TinyResponse::from_string(
                        r#"{"data":{"viewer":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02d","name":"Ada","organization":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02e","name":"Acme"}}}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("respond");
        });
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let connected = connect_private_with_service(
            &db,
            store.clone(),
            "lin_secret_fixture",
            &test_service(format!("http://127.0.0.1:{port}")),
        )
        .await
        .expect("connect");
        responder.join().expect("responder");
        assert_eq!(connected.display_name.as_deref(), Some("Acme"));
        assert!(connected.scopes.contains(&SCOPE_ISSUES_WRITE.into()));
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
            "lin_secret_fixture"
        );
        let serialized = serde_json::to_string(&connected).expect("DTO");
        assert!(!serialized.contains("lin_secret_fixture"));
    }

    #[tokio::test]
    async fn create_issue_marks_body_and_returns_minimal_output() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut request = server.recv().expect("request");
            assert_eq!(request.url(), "/graphql");
            assert_eq!(request.method().as_str(), "POST");
            let body = read_body(&mut request);
            assert!(body.contains("LinearIssueCreate"));
            assert!(body.contains("Release blocker"));
            assert!(body.contains(LINEAR_ACTION_MARKER));
            assert!(!body.contains("lin_secret_fixture"));
            request
                .respond(
                    TinyResponse::from_string(
                        r#"{"data":{"issueCreate":{"success":true,"issue":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02f","identifier":"ENG-7","title":"Release blocker","url":"https://linear.app/acme/issue/ENG-7/x","state":{"name":"Todo"},"description":"raw provider body"}}}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let result = service
            .execute(
                &ValidatedActionRequest {
                    connection_id: "connection".into(),
                    provider_id: "linear".into(),
                    action_id: "linear.create_issue".into(),
                    input: BTreeMap::from([
                        (
                            "team".into(),
                            Value::String("9cfb482a-81e3-4154-b5b9-2c805e70a02d".into()),
                        ),
                        ("title".into(), Value::String("Release blocker".into())),
                        ("body".into(), Value::String("Please investigate".into())),
                        ("priority".into(), Value::String("medium".into())),
                        ("labels".into(), Value::String(String::new())),
                    ]),
                },
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("action");
        responder.join().expect("responder");
        assert_eq!(result.summary, "Created Linear issue ENG-7");
        let serialized = serde_json::to_string(&result).expect("result");
        assert!(serialized.contains("ENG-7"));
        assert!(!serialized.contains("raw provider body"));
        assert!(!serialized.contains("lin_secret_fixture"));
    }

    #[tokio::test]
    async fn mutation_server_failure_is_delivery_unknown() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("mutation request");
            let mut body = String::new();
            let mut cloned = request;
            cloned.as_reader().read_to_string(&mut body).expect("body");
            assert!(body.contains("LinearCommentCreate"));
            cloned.respond(TinyResponse::empty(502)).expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let error = service
            .graphql(
                "lin_secret_fixture",
                COMMENT_CREATE_QUERY,
                Some(&serde_json::json!({
                    "input": { "issueId": "9cfb482a-81e3-4154-b5b9-2c805e70a02d", "body": "x" }
                })),
                true,
            )
            .await
            .expect_err("ambiguous mutation failure");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::DeliveryUnknown);
    }

    #[tokio::test]
    async fn graphql_errors_map_rate_limits_and_permissions_even_with_200() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let rate = server.recv().expect("rate request");
            rate.respond(
                TinyResponse::from_string(
                    r#"{"data":null,"errors":[{"message":"RATELIMITED: API rate limit exceeded"}]}"#,
                )
                .with_header(json_header()),
            )
            .expect("rate response");
            let permission = server.recv().expect("permission request");
            permission
                .respond(
                    TinyResponse::from_string(
                        r#"{"data":null,"errors":[{"message":"You don't have permission to view this"}]}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("permission response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let rate = service
            .graphql(
                "lin_secret_fixture",
                TEAMS_QUERY,
                Some(&serde_json::json!({"first": 50, "after": null, "filter": null})),
                false,
            )
            .await
            .expect_err("rate limited");
        assert_eq!(rate.code, ActionErrorCode::RateLimited);
        let permission = service
            .graphql(
                "lin_secret_fixture",
                TEAMS_QUERY,
                Some(&serde_json::json!({"first": 50, "after": null, "filter": null})),
                false,
            )
            .await
            .expect_err("scope missing");
        responder.join().expect("responder");
        assert_eq!(permission.code, ActionErrorCode::ScopeMissing);
    }

    #[tokio::test]
    async fn event_poll_establishes_then_accepts_created_and_skips_alfred_issues() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            for step in 0..2 {
                let mut request = server.recv().expect("request");
                assert!(read_body(&mut request).contains("LinearTeamIssues"));
                match step {
                    0 => {
                        request
                            .respond(TinyResponse::from_string(r#"{"data":{"issues":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}"#).with_header(json_header()))
                            .expect("empty response");
                    }
                    _ => {
                        request
                            .respond(
                                TinyResponse::from_string(
                                    r#"{"data":{"issues":{"nodes":[{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02f","identifier":"ENG-9","title":"First event","description":null,"url":"https://linear.app/acme/issue/ENG-9/x","createdAt":"2099-01-01T10:00:00Z","updatedAt":"2099-01-01T10:00:00Z","state":{"name":"Todo"},"team":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02d"},"project":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02e"},"assignee":null},{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a10","identifier":"ENG-10","title":"Alfred created","description":"Automated\n<!-- alfred-connected-app -->","url":"https://linear.app/acme/issue/ENG-10/x","createdAt":"2099-01-01T11:00:00Z","updatedAt":"2099-01-01T11:00:00Z","state":{"name":"Todo"},"team":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02d"},"project":null,"assignee":null}],"pageInfo":{"hasNextPage":false}}}}"#,
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
            provider_id: "linear".into(),
            event_type: "linear.issue_activity".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                (
                    "teamId".into(),
                    Value::String("9cfb482a-81e3-4154-b5b9-2c805e70a02d".into()),
                ),
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
        let cursor = initial.cursor.expect("initial cursor");
        assert_eq!(decode_event_cursor(&cursor).unwrap().recent.len(), 0);

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
        assert_eq!(
            next.events[0].external_event_id,
            "9cfb482a-81e3-4154-b5b9-2c805e70a02f@2099-01-01T10:00:00Z"
        );
        assert_eq!(
            next.events[0]
                .attributes
                .get("action")
                .and_then(Value::as_str),
            Some("created")
        );
        let serialized = serde_json::to_string(&next.events).expect("events");
        assert!(!serialized.contains("description"));
        assert!(!serialized.contains("lin_secret_fixture"));
    }

    #[tokio::test]
    async fn event_poll_reports_human_updates_of_alfred_created_issues() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            request
                .respond(
                    TinyResponse::from_string(
                        r#"{"data":{"issues":{"nodes":[{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a10","identifier":"ENG-10","title":"Alfred created","description":"Automated\n<!-- alfred-connected-app -->","url":"https://linear.app/acme/issue/ENG-10/x","createdAt":"2026-08-17T09:00:00Z","updatedAt":"2026-08-18T11:00:00Z","state":{"name":"Done"},"team":{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02d"},"project":null,"assignee":{"name":"Grace"}}],"pageInfo":{"hasNextPage":false}}}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("event response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let config = AppTriggerConfig {
            provider_id: "linear".into(),
            event_type: "linear.issue_activity".into(),
            connection_id: "connection".into(),
            filters: BTreeMap::from([
                (
                    "teamId".into(),
                    Value::String("9cfb482a-81e3-4154-b5b9-2c805e70a02d".into()),
                ),
                ("action".into(), Value::String("any".into())),
            ]),
            descriptor_version: 1,
        };
        let cursor = encode_event_cursor(&LinearEventCursor {
            watermark: "2026-08-18T10:00:00Z".into(),
            recent: vec![LinearRecentIssue {
                id: "9cfb482a-81e3-4154-b5b9-2c805e70a10".into(),
                updated_at: "2026-08-17T09:00:00Z".into(),
            }],
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
        assert_eq!(
            batch.events[0]
                .attributes
                .get("action")
                .and_then(Value::as_str),
            Some("updated")
        );
        assert_eq!(batch.events[0].actor.as_deref(), Some("Grace"));
        assert_eq!(
            batch.events[0]
                .attributes
                .get("status")
                .and_then(Value::as_str),
            Some("Done")
        );
    }

    #[tokio::test]
    async fn resource_selectors_return_bounded_pages() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let mut request = server.recv().expect("teams request");
            let body = read_body(&mut request);
            assert!(body.contains("LinearTeams"));
            assert!(!body.contains("lin_secret_fixture"));
            request
                .respond(
                    TinyResponse::from_string(
                        r#"{"data":{"teams":{"nodes":[{"id":"9cfb482a-81e3-4154-b5b9-2c805e70a02d","name":"Engineering"}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}"#,
                    )
                    .with_header(json_header()),
                )
                .expect("teams response");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let page = service
            .list_resources(
                "teams",
                "team",
                "",
                None,
                &connection(),
                token_capability().await,
                ActionCancellation::never(),
            )
            .await
            .expect("teams page");
        responder.join().expect("responder");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].label, "Engineering");
        assert_eq!(page.next_page_token, None);
    }

    #[tokio::test]
    async fn rate_limited_429_maps_with_retry_after() {
        let server = Server::http(("127.0.0.1", 0)).expect("server");
        let port = server.server_addr().to_ip().expect("address").port();
        let responder = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            request
                .respond(
                    TinyResponse::empty(429)
                        .with_header(Header::from_bytes("Retry-After", "30").expect("retry after")),
                )
                .expect("respond");
        });
        let service = test_service(format!("http://127.0.0.1:{port}"));
        let error = service
            .graphql(
                "lin_secret_fixture",
                TEAMS_QUERY,
                Some(&serde_json::json!({"first": 50, "after": null, "filter": null})),
                false,
            )
            .await
            .expect_err("rate limited");
        responder.join().expect("responder");
        assert_eq!(error.code, ActionErrorCode::RateLimited);
        assert_eq!(error.retry_after_seconds, Some(30));
    }
}
