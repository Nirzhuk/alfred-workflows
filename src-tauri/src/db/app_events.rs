//! Durable state machine for connected-app events.
//!
//! Provider adapters hand this layer an already normalized, bounded event.
//! Receipt and queue insertion is atomic, as is promotion from the queue into
//! a pending workflow run. That makes a crash recoverable without delivering
//! the same provider event twice.

use super::{Db, DbError};
use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_APP_EVENT_PENDING_CAP: usize = 100;
const RECEIPT_RETENTION_DAYS: i64 = 30;
const RECEIPT_CAP_PER_TRIGGER: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordAppEventOutcome {
    Queued,
    Duplicate,
    Backpressure,
    DroppedOverrun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppTriggerState {
    pub trigger_id: String,
    pub cursor: Option<String>,
    pub subscription_id: Option<String>,
    pub expires_at: Option<String>,
    pub last_polled_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_code: Option<String>,
    pub next_attempt_at: Option<String>,
    pub retry_count: u32,
    pub overrun_count: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppTriggerStatus {
    pub trigger_id: String,
    pub cursor_present: bool,
    pub subscription_active: bool,
    pub expires_at: Option<String>,
    pub last_polled_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_code: Option<String>,
    pub next_attempt_at: Option<String>,
    pub overrun_count: u64,
    pub pending_count: u64,
}

#[derive(Debug, Clone)]
pub struct AppTriggerCheckpointUpdate {
    pub cursor: Option<String>,
    pub subscription_id: Option<String>,
    pub expires_at: Option<String>,
    pub polled_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedAppEventRun {
    pub run_id: String,
    pub workflow_id: String,
    pub created_at: String,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn map_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppTriggerState> {
    let retry_count: i64 = row.get(8)?;
    let overrun_count: i64 = row.get(9)?;
    Ok(AppTriggerState {
        trigger_id: row.get(0)?,
        cursor: row.get(1)?,
        subscription_id: row.get(2)?,
        expires_at: row.get(3)?,
        last_polled_at: row.get(4)?,
        last_success_at: row.get(5)?,
        last_error_code: row.get(6)?,
        next_attempt_at: row.get(7)?,
        retry_count: retry_count.max(0) as u32,
        overrun_count: overrun_count.max(0) as u64,
        updated_at: row.get(10)?,
    })
}

impl Db {
    pub fn get_app_trigger_state(
        &self,
        trigger_id: &str,
    ) -> Result<Option<AppTriggerState>, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT trigger_id, cursor, subscription_id, expires_at,
                        last_polled_at, last_success_at, last_error_code,
                        next_attempt_at, retry_count, overrun_count, updated_at
                   FROM app_trigger_state WHERE trigger_id = ?1",
                params![trigger_id],
                map_state,
            )
            .optional()
            .map_err(DbError::from)
        })
    }

    pub fn list_app_trigger_statuses(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<AppTriggerStatus>, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT t.id,
                        CASE WHEN s.cursor IS NULL THEN 0 ELSE 1 END,
                        CASE WHEN s.subscription_id IS NULL THEN 0 ELSE 1 END,
                        s.expires_at, s.last_polled_at, s.last_success_at,
                        s.last_error_code, s.next_attempt_at,
                        COALESCE(s.overrun_count, 0),
                        (SELECT COUNT(*) FROM app_event_queue q WHERE q.trigger_id = t.id)
                   FROM triggers t
                   LEFT JOIN app_trigger_state s ON s.trigger_id = t.id
                  WHERE t.workflow_id = ?1 AND t.source = 'app'
                  ORDER BY t.created_at ASC",
            )?;
            let rows = statement
                .query_map(params![workflow_id], |row| {
                    let cursor_present: i64 = row.get(1)?;
                    let subscription_active: i64 = row.get(2)?;
                    let overrun_count: i64 = row.get(8)?;
                    let pending_count: i64 = row.get(9)?;
                    Ok(AppTriggerStatus {
                        trigger_id: row.get(0)?,
                        cursor_present: cursor_present != 0,
                        subscription_active: subscription_active != 0,
                        expires_at: row.get(3)?,
                        last_polled_at: row.get(4)?,
                        last_success_at: row.get(5)?,
                        last_error_code: row.get(6)?,
                        next_attempt_at: row.get(7)?,
                        overrun_count: overrun_count.max(0) as u64,
                        pending_count: pending_count.max(0) as u64,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn save_app_trigger_checkpoint(
        &self,
        trigger_id: &str,
        update: &AppTriggerCheckpointUpdate,
    ) -> Result<(), DbError> {
        let updated_at = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_trigger_state
                   (trigger_id, cursor, subscription_id, expires_at,
                    last_polled_at, last_success_at, last_error_code,
                    next_attempt_at, retry_count, overrun_count, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, NULL, NULL, 0, 0, ?6)
                 ON CONFLICT(trigger_id) DO UPDATE SET
                   cursor = excluded.cursor,
                   subscription_id = COALESCE(excluded.subscription_id, app_trigger_state.subscription_id),
                   expires_at = COALESCE(excluded.expires_at, app_trigger_state.expires_at),
                   last_polled_at = excluded.last_polled_at,
                   last_success_at = excluded.last_success_at,
                   last_error_code = NULL,
                   next_attempt_at = NULL,
                   retry_count = 0,
                   updated_at = excluded.updated_at",
                params![
                    trigger_id,
                    update.cursor,
                    update.subscription_id,
                    update.expires_at,
                    update.polled_at,
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn mark_app_trigger_error(
        &self,
        trigger_id: &str,
        error_code: &str,
        next_attempt_at: Option<&str>,
    ) -> Result<(), DbError> {
        let updated_at = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_trigger_state
                   (trigger_id, last_error_code, next_attempt_at, retry_count, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(trigger_id) DO UPDATE SET
                   last_error_code = excluded.last_error_code,
                   next_attempt_at = excluded.next_attempt_at,
                   retry_count = app_trigger_state.retry_count + 1,
                   updated_at = excluded.updated_at",
                params![trigger_id, error_code, next_attempt_at, updated_at],
            )?;
            Ok(())
        })
    }

    pub fn record_rejected_app_event(
        &self,
        trigger_id: &str,
        external_event_id: &str,
        reason_code: &str,
    ) -> Result<bool, DbError> {
        let received_at = now();
        self.with_conn(|conn| {
            let changed = conn.execute(
                "INSERT OR IGNORE INTO app_event_receipts
                   (trigger_id, external_event_id, received_at, disposition, reason_code)
                 VALUES (?1, ?2, ?3, 'rejected_invalid', ?4)",
                params![trigger_id, external_event_id, received_at, reason_code],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn record_app_event(
        &self,
        trigger_id: &str,
        external_event_id: &str,
        normalized_event_json: &str,
        replayable: bool,
        pending_cap: usize,
    ) -> Result<RecordAppEventOutcome, DbError> {
        let pending_cap = pending_cap.max(1);
        let received_at = now();
        let queue_id = Uuid::new_v4().to_string();
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM app_event_receipts
                     WHERE trigger_id = ?1 AND external_event_id = ?2
                 )",
                params![trigger_id, external_event_id],
                |row| row.get(0),
            )?;
            if exists {
                transaction.commit()?;
                return Ok(RecordAppEventOutcome::Duplicate);
            }

            let pending: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM app_event_queue WHERE trigger_id = ?1",
                params![trigger_id],
                |row| row.get(0),
            )?;
            if pending as usize >= pending_cap {
                if replayable {
                    transaction.commit()?;
                    return Ok(RecordAppEventOutcome::Backpressure);
                }
                transaction.execute(
                    "INSERT INTO app_event_receipts
                       (trigger_id, external_event_id, received_at, disposition, reason_code)
                     VALUES (?1, ?2, ?3, 'dropped_overrun', 'pending_cap')",
                    params![trigger_id, external_event_id, received_at],
                )?;
                transaction.execute(
                    "INSERT INTO app_trigger_state
                       (trigger_id, overrun_count, updated_at)
                     VALUES (?1, 1, ?2)
                     ON CONFLICT(trigger_id) DO UPDATE SET
                       overrun_count = app_trigger_state.overrun_count + 1,
                       updated_at = excluded.updated_at",
                    params![trigger_id, received_at],
                )?;
                transaction.commit()?;
                return Ok(RecordAppEventOutcome::DroppedOverrun);
            }

            transaction.execute(
                "INSERT INTO app_event_receipts
                   (trigger_id, external_event_id, received_at, disposition)
                 VALUES (?1, ?2, ?3, 'queued')",
                params![trigger_id, external_event_id, received_at],
            )?;
            transaction.execute(
                "INSERT INTO app_event_queue
                   (id, trigger_id, external_event_id, normalized_event_json, enqueued_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    queue_id,
                    trigger_id,
                    external_event_id,
                    normalized_event_json,
                    received_at
                ],
            )?;
            transaction.commit()?;
            Ok(RecordAppEventOutcome::Queued)
        })
    }

    /// Atomically convert the oldest queued event into a pending workflow run.
    /// The caller may launch it immediately or recover it after restart.
    pub fn promote_next_app_event(
        &self,
        workflow_id: &str,
    ) -> Result<Option<PromotedAppEventRun>, DbError> {
        let run_id = Uuid::new_v4().to_string();
        let created_at = now();
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let queued: Option<(String, String, String, String)> = transaction
                .query_row(
                    "SELECT q.id, q.trigger_id, q.external_event_id, q.normalized_event_json
                       FROM app_event_queue q
                       JOIN triggers t ON t.id = q.trigger_id
                      WHERE t.workflow_id = ?1 AND t.enabled = 1
                      ORDER BY q.enqueued_at ASC, q.id ASC
                      LIMIT 1",
                    params![workflow_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            let Some((queue_id, trigger_id, external_event_id, payload)) = queued else {
                transaction.commit()?;
                return Ok(None);
            };

            transaction.execute(
                "INSERT INTO runs
                   (id, workflow_id, trigger_kind, status, payload_json, created_at)
                 VALUES (?1, ?2, 'app', 'pending', ?3, ?4)",
                params![run_id, workflow_id, payload, created_at],
            )?;
            transaction.execute(
                "UPDATE app_event_receipts
                    SET disposition = 'enqueued', run_id = ?1
                  WHERE trigger_id = ?2 AND external_event_id = ?3",
                params![run_id, trigger_id, external_event_id],
            )?;
            transaction.execute(
                "DELETE FROM app_event_queue WHERE id = ?1",
                params![queue_id],
            )?;
            transaction.commit()?;
            Ok(Some(PromotedAppEventRun {
                run_id,
                workflow_id: workflow_id.to_owned(),
                created_at,
            }))
        })
    }

    pub fn list_pending_app_event_runs(&self) -> Result<Vec<PromotedAppEventRun>, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT DISTINCT r.id, r.workflow_id, r.created_at
                   FROM runs r
                   JOIN app_event_receipts receipt ON receipt.run_id = r.id
                  WHERE r.trigger_kind = 'app' AND r.status = 'pending'
                  ORDER BY r.created_at ASC",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok(PromotedAppEventRun {
                        run_id: row.get(0)?,
                        workflow_id: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// A process crash can leave an app-event run marked `running`. It is a
    /// terminal one-shot delivery: surface interruption, never put its event
    /// back on the queue automatically.
    pub fn fail_interrupted_app_event_runs(&self) -> Result<usize, DbError> {
        let finished_at = now();
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE runs
                    SET status = 'failed', finished_at = ?1,
                        error = 'Interrupted when Alfred exited'
                  WHERE trigger_kind = 'app' AND status = 'running'
                    AND EXISTS (
                      SELECT 1 FROM app_event_receipts receipt
                       WHERE receipt.run_id = runs.id
                    )",
                params![finished_at],
            )?;
            Ok(changed)
        })
    }

    pub fn app_event_queue_workflow_ids(&self) -> Result<Vec<String>, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT DISTINCT t.workflow_id
                   FROM app_event_queue q
                   JOIN triggers t ON t.id = q.trigger_id
                  WHERE t.enabled = 1
                  GROUP BY t.workflow_id
                  ORDER BY MIN(q.enqueued_at)",
            )?;
            let rows = statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn prune_app_event_receipts(&self) -> Result<usize, DbError> {
        let cutoff = (Utc::now() - Duration::days(RECEIPT_RETENTION_DAYS)).to_rfc3339();
        self.with_conn(|conn| {
            let mut removed = conn.execute(
                "DELETE FROM app_event_receipts
                  WHERE received_at < ?1
                    AND disposition <> 'queued'
                    AND NOT EXISTS (
                      SELECT 1 FROM app_event_queue q
                       WHERE q.trigger_id = app_event_receipts.trigger_id
                         AND q.external_event_id = app_event_receipts.external_event_id
                    )",
                params![cutoff],
            )?;
            let trigger_ids = {
                let mut statement = conn.prepare(
                    "SELECT trigger_id FROM app_event_receipts
                      GROUP BY trigger_id HAVING COUNT(*) > ?1",
                )?;
                let values = statement
                    .query_map(params![RECEIPT_CAP_PER_TRIGGER as i64], |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                values
            };
            for trigger_id in trigger_ids {
                removed += conn.execute(
                    "DELETE FROM app_event_receipts
                      WHERE trigger_id = ?1
                        AND disposition <> 'queued'
                        AND NOT EXISTS (
                          SELECT 1 FROM app_event_queue q
                           WHERE q.trigger_id = app_event_receipts.trigger_id
                             AND q.external_event_id = app_event_receipts.external_event_id
                        )
                        AND rowid IN (
                          SELECT rowid FROM app_event_receipts
                           WHERE trigger_id = ?1 AND disposition <> 'queued'
                           ORDER BY received_at DESC
                           LIMIT -1 OFFSET ?2
                        )",
                    params![trigger_id, RECEIPT_CAP_PER_TRIGGER as i64],
                )?;
            }
            Ok(removed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, UpsertTriggerInput};
    use serde_json::json;

    fn fixture() -> (Db, String, String) {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Events".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [{"id": "input", "type": "input", "data": {"label": "Input", "prompt": ""}}], "edges": []}),
            })
            .expect("workflow");
        let trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: workflow.id.clone(),
                source: "app".into(),
                label: "Event".into(),
                config: json!({}),
                enabled: true,
            })
            .expect("trigger");
        (db, workflow.id, trigger.id)
    }

    #[test]
    fn duplicate_delivery_is_recorded_once() {
        let (db, _, trigger_id) = fixture();
        assert_eq!(
            db.record_app_event(&trigger_id, "evt-1", "{}", true, 10)
                .expect("first"),
            RecordAppEventOutcome::Queued
        );
        assert_eq!(
            db.record_app_event(&trigger_id, "evt-1", "{}", true, 10)
                .expect("duplicate"),
            RecordAppEventOutcome::Duplicate
        );
    }

    #[test]
    fn pull_backpressure_preserves_cursor_and_socket_overrun_is_durable() {
        let (db, _, trigger_id) = fixture();
        db.record_app_event(&trigger_id, "evt-1", "{}", true, 1)
            .expect("queue first");
        assert_eq!(
            db.record_app_event(&trigger_id, "evt-2", "{}", true, 1)
                .expect("pull backpressure"),
            RecordAppEventOutcome::Backpressure
        );
        assert_eq!(
            db.record_app_event(&trigger_id, "evt-3", "{}", false, 1)
                .expect("socket overrun"),
            RecordAppEventOutcome::DroppedOverrun
        );
        assert_eq!(
            db.get_app_trigger_state(&trigger_id)
                .expect("state")
                .expect("state row")
                .overrun_count,
            1
        );
    }

    #[test]
    fn queue_promotion_and_pending_run_are_one_transaction() {
        let (db, workflow_id, trigger_id) = fixture();
        db.record_app_event(&trigger_id, "evt-1", "{\"preview\":\"safe\"}", true, 10)
            .expect("queue");
        let promoted = db
            .promote_next_app_event(&workflow_id)
            .expect("promote")
            .expect("run");
        assert_eq!(promoted.workflow_id, workflow_id);
        let pending = db.list_pending_app_event_runs().expect("pending");
        assert_eq!(pending, vec![promoted]);
        assert!(db
            .promote_next_app_event(&workflow_id)
            .expect("empty")
            .is_none());
    }

    #[test]
    fn receipt_pruning_never_removes_a_pending_queue_receipt() {
        let (db, _, trigger_id) = fixture();
        db.record_app_event(&trigger_id, "evt-1", "{}", true, 10)
            .expect("queue");
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE app_event_receipts SET received_at = '2000-01-01T00:00:00Z'",
                [],
            )?;
            Ok(())
        })
        .expect("age receipt");
        db.prune_app_event_receipts().expect("prune");
        assert_eq!(
            db.record_app_event(&trigger_id, "evt-1", "{}", true, 10)
                .expect("still duplicate"),
            RecordAppEventOutcome::Duplicate
        );
    }

    #[test]
    fn interrupted_or_failed_runs_are_terminal_and_never_requeued() {
        let (db, workflow_id, trigger_id) = fixture();
        db.record_app_event(&trigger_id, "evt-1", "{}", true, 10)
            .expect("queue");
        let promoted = db
            .promote_next_app_event(&workflow_id)
            .expect("promote")
            .expect("pending");
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE runs SET status = 'running' WHERE id = ?1",
                params![promoted.run_id],
            )?;
            Ok(())
        })
        .expect("simulate interrupted run");
        assert_eq!(db.fail_interrupted_app_event_runs().expect("recover"), 1);
        assert!(db
            .list_pending_app_event_runs()
            .expect("pending")
            .is_empty());
        assert!(db
            .promote_next_app_event(&workflow_id)
            .expect("queue remains empty")
            .is_none());
    }

    #[test]
    fn out_of_order_events_are_each_accepted_once() {
        let (db, _, trigger_id) = fixture();
        db.record_app_event(&trigger_id, "event-new", "{}", true, 10)
            .expect("newer");
        db.record_app_event(&trigger_id, "event-old", "{}", true, 10)
            .expect("older");
        let count: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM app_event_queue WHERE trigger_id = ?1",
                    params![trigger_id],
                    |row| row.get(0),
                )
                .map_err(DbError::from)
            })
            .expect("count");
        assert_eq!(count, 2);
    }
}
