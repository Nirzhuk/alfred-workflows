use rusqlite::Connection;

use super::history::{index_memory, index_run_step};
use super::DbError;

/// Apply additive migrations for databases created with earlier schemas.
/// `CREATE TABLE IF NOT EXISTS` does not alter existing tables.
pub fn apply_migrations(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_folders (
           id TEXT PRIMARY KEY NOT NULL,
           name TEXT NOT NULL,
           sort_order INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );",
    )?;
    drop_trigger_kind_check(conn)?;
    ensure_column(
        conn,
        "runs",
        "trigger_kind",
        "TEXT NOT NULL DEFAULT 'manual'",
    )?;
    ensure_column(conn, "runs", "payload_json", "TEXT")?;
    ensure_column(conn, "run_steps", "skill_name", "TEXT")?;
    ensure_column(conn, "run_steps", "agent_provider", "TEXT")?;
    ensure_column(
        conn,
        "workflows",
        "working_directory",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "workflows",
        "sort_order",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "workflows", "folder_id", "TEXT")?;
    backfill_workflow_sort_order(conn)?;

    // Drop legacy column name if an early schema used `trigger`.
    // SQLite only supports DROP COLUMN on newer versions; ignore failures.
    if table_has_column(conn, "runs", "trigger")? && table_has_column(conn, "runs", "trigger_kind")?
    {
        let _ = conn.execute_batch("ALTER TABLE runs DROP COLUMN trigger;");
    }

    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_workflow_unique ON schedules(workflow_id);",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_workflows_folder_id ON workflows(folder_id);",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_memories_workflow_id ON memories(workflow_id);
         CREATE INDEX IF NOT EXISTS idx_memories_workflow_pinned ON memories(workflow_id, pinned);",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_links (
           id TEXT PRIMARY KEY NOT NULL,
           workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
           memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
           created_at TEXT NOT NULL,
           UNIQUE (workflow_id, memory_id)
         );
         CREATE INDEX IF NOT EXISTS idx_memory_links_workflow_id ON memory_links(workflow_id);
         CREATE INDEX IF NOT EXISTS idx_memory_links_memory_id ON memory_links(memory_id);",
    )?;
    create_search_indexes(conn)?;
    rebuild_search_indexes(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_triggers_workflow_id ON triggers(workflow_id);
         CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled);",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_connections (
           id TEXT PRIMARY KEY NOT NULL,
           provider_id TEXT NOT NULL,
           display_name TEXT,
           external_account_id TEXT,
           external_tenant_id TEXT,
           connection_mode TEXT NOT NULL,
           identity_key TEXT NOT NULL,
           scopes_json TEXT NOT NULL DEFAULT '[]',
           provider_metadata_json TEXT NOT NULL DEFAULT '{}',
           status TEXT NOT NULL DEFAULT 'connected' CHECK (status IN ('connected', 'expired', 'error', 'revoked')),
           expires_at TEXT,
           last_checked_at TEXT,
           last_error_code TEXT,
           credential_ref TEXT NOT NULL UNIQUE,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_app_connections_identity
           ON app_connections(provider_id, connection_mode, identity_key);
         CREATE INDEX IF NOT EXISTS idx_app_connections_provider_id
           ON app_connections(provider_id);",
    )?;
    ensure_column(
        conn,
        "app_connections",
        "provider_metadata_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_trigger_state (
           trigger_id TEXT PRIMARY KEY NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
           cursor TEXT,
           subscription_id TEXT,
           expires_at TEXT,
           last_polled_at TEXT,
           last_success_at TEXT,
           last_error_code TEXT,
           next_attempt_at TEXT,
           retry_count INTEGER NOT NULL DEFAULT 0,
           overrun_count INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS app_event_receipts (
           trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
           external_event_id TEXT NOT NULL,
           received_at TEXT NOT NULL,
           disposition TEXT NOT NULL CHECK (disposition IN ('queued', 'enqueued', 'dropped_overrun', 'rejected_invalid')),
           run_id TEXT,
           reason_code TEXT,
           PRIMARY KEY (trigger_id, external_event_id)
         );
         CREATE TABLE IF NOT EXISTS app_event_queue (
           id TEXT PRIMARY KEY NOT NULL,
           trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
           external_event_id TEXT NOT NULL,
           normalized_event_json TEXT NOT NULL,
           enqueued_at TEXT NOT NULL,
           started_at TEXT,
           UNIQUE (trigger_id, external_event_id)
         );
         CREATE INDEX IF NOT EXISTS idx_app_event_queue_trigger
           ON app_event_queue(trigger_id, enqueued_at);
         CREATE INDEX IF NOT EXISTS idx_app_event_receipts_received
           ON app_event_receipts(received_at);",
    )?;

    Ok(())
}

fn create_search_indexes(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
           memory_id UNINDEXED,
           workflow_id UNINDEXED,
           title,
           body,
           tokenize = 'unicode61 remove_diacritics 2'
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS run_step_fts USING fts5(
           step_id UNINDEXED,
           run_id UNINDEXED,
           workflow_id UNINDEXED,
           node_id UNINDEXED,
           input_text,
           output_text,
           error_text,
           tokenize = 'unicode61 remove_diacritics 2'
         );",
    )?;
    Ok(())
}

pub(crate) fn rebuild_search_indexes(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
           key TEXT PRIMARY KEY NOT NULL,
           value TEXT NOT NULL
         );",
    )?;
    let already_backfilled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_meta WHERE key = 'search_fts_backfill_v1')",
        [],
        |row| row.get(0),
    )?;
    if already_backfilled {
        return Ok(());
    }

    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM memory_fts", [])?;
    transaction.execute("DELETE FROM run_step_fts", [])?;

    let memory_ids = {
        let mut statement =
            transaction.prepare("SELECT id FROM memories ORDER BY created_at, id")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for memory_id in memory_ids {
        index_memory(&transaction, &memory_id)?;
    }

    let run_steps = {
        let mut statement = transaction.prepare(
            "SELECT rs.id, rs.run_id, r.workflow_id, rs.node_id,
                    rs.input_json, rs.output_json, rs.error
             FROM run_steps rs
             JOIN runs r ON r.id = rs.run_id
             ORDER BY rs.created_at, rs.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (step_id, run_id, workflow_id, node_id, input_json, output_json, error) in run_steps {
        let input = serde_json::from_str(&input_json).unwrap_or_else(|_| serde_json::json!({}));
        let output = serde_json::from_str(&output_json).unwrap_or_else(|_| serde_json::json!({}));
        index_run_step(
            &transaction,
            &step_id,
            &run_id,
            &workflow_id,
            &node_id,
            &input,
            &output,
            error.as_deref(),
        )?;
    }
    transaction.execute(
        "INSERT INTO schema_meta(key, value) VALUES ('search_fts_backfill_v1', 'complete')",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Early schemas pinned `trigger_kind` to ('manual','schedule'), which rejects
/// event-sourced runs. SQLite can't ALTER a CHECK, so rebuild the table once.
fn drop_trigger_kind_check(conn: &Connection) -> Result<(), DbError> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
            [],
            |row| row.get(0),
        )
        .ok();

    let Some(sql) = sql else { return Ok(()) };
    if !sql.contains("CHECK (trigger_kind") {
        return Ok(());
    }

    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         BEGIN;
         CREATE TABLE runs_migrated (
           id TEXT PRIMARY KEY NOT NULL,
           workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
           trigger_kind TEXT NOT NULL DEFAULT 'manual',
           status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
           payload_json TEXT,
           error TEXT,
           started_at TEXT,
           finished_at TEXT,
           created_at TEXT NOT NULL
         );
         INSERT INTO runs_migrated (id, workflow_id, trigger_kind, status, error, started_at, finished_at, created_at)
           SELECT id, workflow_id, trigger_kind, status, error, started_at, finished_at, created_at FROM runs;
         DROP TABLE runs;
         ALTER TABLE runs_migrated RENAME TO runs;
         COMMIT;
         PRAGMA foreign_keys = ON;
         CREATE INDEX IF NOT EXISTS idx_runs_workflow_id ON runs(workflow_id);",
    )?;

    Ok(())
}

/// One-time: give existing rows a stable order from newest → oldest.
fn backfill_workflow_sort_order(conn: &Connection) -> Result<(), DbError> {
    let needs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workflows WHERE sort_order = 0",
        [],
        |row| row.get(0),
    )?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM workflows", [], |row| row.get(0))?;
    // Only backfill when every row still sits at the default (fresh column).
    if total == 0 || needs != total {
        return Ok(());
    }

    let ids: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT id FROM workflows ORDER BY updated_at DESC, created_at DESC")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    for (index, id) in ids.into_iter().enumerate() {
        conn.execute(
            "UPDATE workflows SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![index as i64, id],
        )?;
    }
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<(), DbError> {
    if table_has_column(conn, table, column)? {
        return Ok(());
    }

    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
        [],
    )?;
    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, DbError> {
    // PRAGMA table_info cannot take bound params for the table name.
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("prepare table info");
        stmt.query_map([], |row| row.get(1))
            .expect("query columns")
            .collect::<Result<Vec<String>, _>>()
            .expect("collect columns")
    }

    #[test]
    fn initializes_app_connections_on_an_empty_database() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");

        let names = columns(&conn, "app_connections");
        assert!(names.contains(&"provider_id".to_owned()));
        assert!(names.contains(&"credential_ref".to_owned()));
        assert!(!names.iter().any(|name| {
            let name = name.to_ascii_lowercase();
            name.contains("token") || name.contains("secret") || name.contains("authorization_code")
        }));
    }

    #[test]
    fn upgrades_an_existing_schema_without_connection_metadata() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize legacy fixture");
        conn.execute_batch("DROP TABLE app_connections;")
            .expect("remove new table from fixture");

        apply_migrations(&conn).expect("upgrade fixture");

        assert!(columns(&conn, "app_connections").contains(&"identity_key".to_owned()));
        assert!(columns(&conn, "app_connections").contains(&"provider_metadata_json".to_owned()));
    }

    #[test]
    fn initializes_app_event_delivery_tables() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");

        assert!(columns(&conn, "app_trigger_state").contains(&"cursor".to_owned()));
        assert!(columns(&conn, "app_event_receipts").contains(&"disposition".to_owned()));
        assert!(columns(&conn, "app_event_queue").contains(&"normalized_event_json".to_owned()));
    }

    #[test]
    fn initializes_search_indexes() {
        let conn = Connection::open_in_memory().expect("open database");
        let fts5_enabled: i64 = conn
            .query_row(
                "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                [],
                |row| row.get(0),
            )
            .expect("check FTS5 compile option");
        assert_eq!(fts5_enabled, 1, "bundled SQLite must include FTS5");

        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");
        conn.execute(
            "INSERT INTO memory_fts(memory_id, workflow_id, title, body)
             VALUES ('memory-1', 'workflow-1', 'Café notes', 'local search')",
            [],
        )
        .expect("insert search fixture");

        let hit: String = conn
            .query_row(
                "SELECT memory_id FROM memory_fts WHERE memory_fts MATCH '\"cafe\"*'",
                [],
                |row| row.get(0),
            )
            .expect("query FTS5 fixture");
        assert_eq!(hit, "memory-1");
    }
}
