use super::{Db, DbError};
use crate::integrations::catalog::ProviderCatalog;
use crate::integrations::models::{
    AppConnection, AppConnectionUsage, ConnectionStatus, ConnectionUsageItem, UpsertAppConnection,
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

const COLUMNS: &str = "id, provider_id, display_name, external_account_id, \
external_tenant_id, connection_mode, identity_key, scopes_json, status, expires_at, \
last_checked_at, last_error_code, credential_ref, created_at, updated_at";

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn map_connection(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppConnection> {
    let scopes_json: String = row.get(7)?;
    let status: String = row.get(8)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let status = ConnectionStatus::from_str(&status).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;
    Ok(AppConnection {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        display_name: row.get(2)?,
        external_account_id: row.get(3)?,
        external_tenant_id: row.get(4)?,
        connection_mode: row.get(5)?,
        identity_key: row.get(6)?,
        scopes,
        status,
        expires_at: row.get(9)?,
        last_checked_at: row.get(10)?,
        last_error_code: row.get(11)?,
        credential_ref: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

impl Db {
    pub fn list_app_connections(&self) -> Result<Vec<AppConnection>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM app_connections ORDER BY provider_id, created_at"
            ))?;
            let rows = stmt
                .query_map([], map_connection)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_app_connection(&self, id: &str) -> Result<Option<AppConnection>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM app_connections WHERE id = ?1"),
                    params![id],
                    map_connection,
                )
                .optional()?)
        })
    }

    #[allow(dead_code)] // Called by provider plans after they validate external identity.
    pub fn upsert_app_connection(
        &self,
        mut input: UpsertAppConnection,
    ) -> Result<AppConnection, DbError> {
        if !ProviderCatalog::default().contains(&input.provider_id) {
            return Err(DbError::Other("unknown connected-app provider".into()));
        }
        if input.provider_id.trim().is_empty()
            || input.connection_mode.trim().is_empty()
            || input.identity_key.trim().is_empty()
            || input.credential_ref.trim().is_empty()
        {
            return Err(DbError::Other(
                "provider, connection mode, identity, and credential reference are required".into(),
            ));
        }
        input.scopes.sort();
        input.scopes.dedup();
        let scopes_json = serde_json::to_string(&input.scopes)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();

        let id = self.with_conn(|conn| {
            let existing: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, credential_ref FROM app_connections
                     WHERE provider_id = ?1 AND connection_mode = ?2 AND identity_key = ?3",
                    params![input.provider_id, input.connection_mode, input.identity_key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            if let Some((id, _credential_ref)) = existing {
                conn.execute(
                    "UPDATE app_connections SET
                       display_name = ?1, external_account_id = ?2, external_tenant_id = ?3,
                       scopes_json = ?4, status = 'connected', expires_at = ?5,
                       last_checked_at = ?6, last_error_code = NULL,
                       updated_at = ?6
                     WHERE id = ?7",
                    params![
                        input.display_name,
                        input.external_account_id,
                        input.external_tenant_id,
                        scopes_json,
                        input.expires_at,
                        updated_at,
                        id
                    ],
                )?;
                return Ok(id);
            }

            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO app_connections (
                   id, provider_id, display_name, external_account_id, external_tenant_id,
                   connection_mode, identity_key, scopes_json, status, expires_at,
                   last_checked_at, credential_ref, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'connected', ?9, ?10, ?11, ?10, ?10)",
                params![
                    id,
                    input.provider_id,
                    input.display_name,
                    input.external_account_id,
                    input.external_tenant_id,
                    input.connection_mode,
                    input.identity_key,
                    scopes_json,
                    input.expires_at,
                    updated_at,
                    input.credential_ref
                ],
            )?;
            Ok(id)
        })?;

        self.get_app_connection(&id)?
            .ok_or_else(|| DbError::Other("failed to load saved app connection".into()))
    }

    pub fn set_app_connection_refresh_state(
        &self,
        id: &str,
        status: ConnectionStatus,
        expires_at: Option<&str>,
        last_error_code: Option<&str>,
    ) -> Result<(), DbError> {
        let checked_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE app_connections
                 SET status = ?1, expires_at = ?2, last_checked_at = ?3,
                     last_error_code = ?4, updated_at = ?3
                 WHERE id = ?5 AND status != 'revoked'",
                params![status.as_str(), expires_at, checked_at, last_error_code, id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("connection not found or revoked".into()));
        }
        Ok(())
    }

    pub fn mark_app_connection_revoked(&self, id: &str) -> Result<(), DbError> {
        let updated_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE app_connections SET status = 'revoked', updated_at = ?1 WHERE id = ?2",
                params![updated_at, id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("connection not found".into()));
        }
        Ok(())
    }

    pub fn delete_app_connection_metadata(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM app_connections WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn get_app_connection_usage(&self, id: &str) -> Result<AppConnectionUsage, DbError> {
        let workflows = self.list_workflows()?;
        let referenced_workflows = workflows
            .into_iter()
            .filter(|workflow| app_action_references(&workflow.graph, id, false))
            .map(|workflow| ConnectionUsageItem {
                id: workflow.id,
                label: workflow.name,
                enabled: true,
            })
            .collect::<Vec<_>>();
        let workflow_ids = referenced_workflows
            .iter()
            .map(|workflow| workflow.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        let schedules = self
            .list_all_schedules()?
            .into_iter()
            .filter(|schedule| workflow_ids.contains(schedule.workflow_id.as_str()))
            .map(|schedule| ConnectionUsageItem {
                id: schedule.id,
                label: schedule.workflow_name,
                enabled: schedule.enabled,
            })
            .collect();

        let triggers = self
            .list_all_triggers()?
            .into_iter()
            .filter(|trigger| value_references(&trigger.config, id))
            .map(|trigger| ConnectionUsageItem {
                id: trigger.id,
                label: if trigger.label.is_empty() {
                    trigger.source
                } else {
                    trigger.label
                },
                enabled: trigger.enabled,
            })
            .collect();

        Ok(AppConnectionUsage {
            workflows: referenced_workflows,
            schedules,
            triggers,
        })
    }
}

fn app_action_references(value: &Value, connection_id: &str, inside_app_action: bool) -> bool {
    match value {
        Value::Object(object) => {
            let inside = inside_app_action
                || object
                    .get("type")
                    .or_else(|| object.get("kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "appAction");
            if inside
                && object
                    .get("connectionId")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == connection_id)
            {
                return true;
            }
            object
                .values()
                .any(|child| app_action_references(child, connection_id, inside))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| app_action_references(child, connection_id, inside_app_action)),
        _ => false,
    }
}

fn value_references(value: &Value, connection_id: &str) -> bool {
    match value {
        Value::Object(object) => {
            if object
                .get("connectionId")
                .and_then(Value::as_str)
                .is_some_and(|value| value == connection_id)
            {
                return true;
            }
            object
                .values()
                .any(|child| value_references(child, connection_id))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| value_references(child, connection_id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, UpsertScheduleInput, UpsertTriggerInput};
    use crate::integrations::models::canonical_identity_key;

    fn input(tenant: Option<&str>, mode: &str, credential_ref: &str) -> UpsertAppConnection {
        let parts = [tenant.unwrap_or("account"), "user"];
        UpsertAppConnection {
            provider_id: "slack".into(),
            display_name: None,
            external_account_id: Some("user".into()),
            external_tenant_id: tenant.map(str::to_owned),
            connection_mode: mode.into(),
            identity_key: canonical_identity_key("slack", mode, &parts),
            scopes: vec!["write".into(), "read".into(), "read".into()],
            expires_at: None,
            credential_ref: credential_ref.into(),
        }
    }

    #[test]
    fn reconnect_upgrades_existing_metadata_and_preserves_id() {
        let db = Db::open_in_memory().expect("database");
        let first = db
            .upsert_app_connection(input(Some("tenant-a"), "native_oauth", "first-ref"))
            .expect("insert");
        let mut reconnect = input(Some("tenant-a"), "native_oauth", "rotated-ref");
        reconnect.display_name = Some("Workspace A".into());
        reconnect.scopes.push("admin".into());
        let second = db.upsert_app_connection(reconnect).expect("reconnect");

        assert_eq!(first.id, second.id);
        assert_eq!(second.credential_ref, "first-ref");
        assert_eq!(second.display_name.as_deref(), Some("Workspace A"));
        assert_eq!(second.scopes, vec!["admin", "read", "write"]);
        assert_eq!(db.list_app_connections().expect("list").len(), 1);
    }

    #[test]
    fn identity_separates_tenants_modes_and_nullable_display_metadata() {
        let db = Db::open_in_memory().expect("database");
        let a = db
            .upsert_app_connection(input(Some("tenant-a"), "native_oauth", "ref-a"))
            .expect("tenant a");
        let b = db
            .upsert_app_connection(input(Some("tenant-b"), "native_oauth", "ref-b"))
            .expect("tenant b");
        let webhook = db
            .upsert_app_connection(input(Some("tenant-a"), "incoming_webhook", "ref-c"))
            .expect("webhook");
        let mut other_account = input(Some("tenant-a"), "native_oauth", "ref-d");
        other_account.external_account_id = Some("different-user".into());
        other_account.identity_key =
            canonical_identity_key("slack", "native_oauth", &["tenant-a", "different-user"]);
        let other_account = db
            .upsert_app_connection(other_account)
            .expect("different account");

        assert_ne!(a.id, b.id);
        assert_ne!(a.id, webhook.id);
        assert_ne!(a.id, other_account.id);
        assert!(a.display_name.is_none());
        assert_eq!(db.list_app_connections().expect("list").len(), 4);
    }

    #[test]
    fn usage_finds_direct_workflow_trigger_and_transitive_schedule() {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Notify team".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: serde_json::json!({
                    "nodes": [{"type": "appAction", "data": {"connectionId": "connection-a"}}],
                    "edges": []
                }),
            })
            .expect("workflow");
        db.upsert_schedule(
            UpsertScheduleInput {
                workflow_id: workflow.id.clone(),
                cron: "0 * * * * *".into(),
                enabled: false,
            },
            None,
        )
        .expect("schedule");
        db.upsert_trigger(UpsertTriggerInput {
            id: None,
            workflow_id: workflow.id,
            source: "app_event".into(),
            label: "Slack event".into(),
            config: serde_json::json!({"connectionId": "connection-a"}),
            enabled: true,
        })
        .expect("trigger");

        let usage = db.get_app_connection_usage("connection-a").expect("usage");
        assert_eq!(usage.workflows.len(), 1);
        assert_eq!(usage.schedules.len(), 1);
        assert!(!usage.schedules[0].enabled);
        assert_eq!(usage.triggers.len(), 1);
    }

    #[test]
    fn rejects_provider_ids_outside_the_rust_catalog() {
        let db = Db::open_in_memory().expect("database");
        let mut unknown = input(None, "native_oauth", "unknown-ref");
        unknown.provider_id = "unregistered".into();
        unknown.identity_key = canonical_identity_key("unregistered", "native_oauth", &["account"]);
        assert!(db.upsert_app_connection(unknown).is_err());
    }
}
