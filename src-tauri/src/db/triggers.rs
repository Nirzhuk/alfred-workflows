//! Event triggers — the non-schedule ways a workflow starts.
//!
//! A workflow can have many (a file watcher plus two webhooks, say), unlike
//! `schedules`, which is 1:1. `source` is a plain string so adding a source
//! never needs a migration.

use super::{Db, DbError};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    pub id: String,
    pub workflow_id: String,
    /// `file` | `webhook`
    pub source: String,
    pub label: String,
    pub config: Value,
    /// Shared token for `webhook` triggers; `None` for sources that don't need one.
    pub secret: Option<String>,
    pub enabled: bool,
    pub last_fired_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTriggerInput {
    /// Omit to create.
    #[serde(default)]
    pub id: Option<String>,
    pub workflow_id: String,
    pub source: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

const COLUMNS: &str = "id, workflow_id, source, label, config_json, secret, enabled, \
                       last_fired_at, created_at, updated_at";

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Trigger> {
    let config_json: String = row.get(4)?;
    let enabled: i64 = row.get(6)?;
    Ok(Trigger {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        source: row.get(2)?,
        label: row.get(3)?,
        config: serde_json::from_str(&config_json).unwrap_or(Value::Null),
        secret: row.get(5)?,
        enabled: enabled != 0,
        last_fired_at: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

impl Db {
    pub fn list_all_triggers(&self) -> Result<Vec<Trigger>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM triggers ORDER BY created_at ASC"
            ))?;
            let rows = stmt
                .query_map([], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn list_triggers(&self, workflow_id: &str) -> Result<Vec<Trigger>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM triggers WHERE workflow_id = ?1 ORDER BY created_at ASC"
            ))?;
            let rows = stmt
                .query_map(params![workflow_id], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Every enabled trigger across all workflows — what the watchers bind to.
    pub fn list_enabled_triggers(&self, source: Option<&str>) -> Result<Vec<Trigger>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM triggers
                 WHERE enabled = 1 AND (?1 IS NULL OR source = ?1)
                 ORDER BY created_at ASC"
            ))?;
            let rows = stmt
                .query_map(params![source], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_trigger(&self, id: &str) -> Result<Option<Trigger>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM triggers WHERE id = ?1 LIMIT 1"
            ))?;
            let mut rows = stmt.query_map(params![id], map_row)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn upsert_trigger(&self, input: UpsertTriggerInput) -> Result<Trigger, DbError> {
        self.get_workflow(&input.workflow_id)?
            .ok_or_else(|| DbError::Other(format!("workflow not found: {}", input.workflow_id)))?;

        let config_json = serde_json::to_string(&input.config)
            .map_err(|e| DbError::Other(format!("invalid trigger config: {e}")))?;
        let updated_at = now();
        let enabled = if input.enabled { 1 } else { 0 };

        let existing = match input.id.as_deref() {
            Some(id) => self.get_trigger(id)?,
            None => None,
        };

        let id = match existing {
            Some(existing) => {
                self.with_conn(|conn| {
                    conn.execute(
                        "UPDATE triggers
                         SET source = ?1, label = ?2, config_json = ?3, enabled = ?4, updated_at = ?5
                         WHERE id = ?6",
                        params![
                            input.source,
                            input.label,
                            config_json,
                            enabled,
                            updated_at,
                            existing.id
                        ],
                    )?;
                    Ok(())
                })?;
                existing.id
            }
            None => {
                let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
                // Webhooks are reachable by anyone who can hit localhost, so every
                // one gets its own bearer token at creation time.
                let secret = if input.source == "webhook" {
                    Some(Uuid::new_v4().to_string().replace('-', ""))
                } else {
                    None
                };
                let created_at = updated_at.clone();
                self.with_conn(|conn| {
                    conn.execute(
                        "INSERT INTO triggers
                         (id, workflow_id, source, label, config_json, secret, enabled, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                        params![
                            id,
                            input.workflow_id,
                            input.source,
                            input.label,
                            config_json,
                            secret,
                            enabled,
                            created_at
                        ],
                    )?;
                    Ok(())
                })?;
                id
            }
        };

        self.get_trigger(&id)?
            .ok_or_else(|| DbError::Other("failed to load saved trigger".into()))
    }

    pub fn delete_trigger(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM triggers WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn mark_trigger_fired(&self, id: &str) -> Result<(), DbError> {
        let at = now();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE triggers SET last_fired_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![at, id],
            )?;
            Ok(())
        })
    }
}
