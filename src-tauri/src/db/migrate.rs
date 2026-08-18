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
    migrate_scoped_memories(conn)?;
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

const SCOPED_MEMORIES_TABLE: &str = "
  CREATE TABLE memories_scoped (
    id TEXT PRIMARY KEY NOT NULL,
    workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL,
    run_id TEXT,
    node_id TEXT,
    scope_type TEXT NOT NULL DEFAULT 'workflow'
      CHECK (scope_type IN ('user', 'workspace', 'workflow')),
    scope_key TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'text'
      CHECK (kind IN ('text', 'note', 'artifact')),
    memory_type TEXT NOT NULL DEFAULT 'output'
      CHECK (memory_type IN (
        'preference', 'fact', 'decision', 'constraint', 'lesson', 'episode',
        'checkpoint', 'note', 'output', 'artifact'
      )),
    source TEXT NOT NULL DEFAULT 'run'
      CHECK (source IN ('run', 'manual', 'import', 'review')),
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    artifact_path TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 1.0 CHECK (confidence >= 0 AND confidence <= 1),
    salience INTEGER NOT NULL DEFAULT 50 CHECK (salience >= 0 AND salience <= 100),
    status TEXT NOT NULL DEFAULT 'active'
      CHECK (status IN ('active', 'superseded', 'retracted')),
    supersedes_id TEXT REFERENCES memories_scoped(id) ON DELETE SET NULL,
    last_confirmed_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
      (scope_type = 'workflow' AND workflow_id IS NOT NULL AND scope_key = workflow_id)
      OR (scope_type = 'workspace' AND length(trim(scope_key)) > 0)
      OR (scope_type = 'user' AND scope_key = 'local-user')
    )
  );";

fn ensure_scoped_memory_indexes(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_memories_workflow_id;
         DROP INDEX IF EXISTS idx_memories_workflow_pinned;
         CREATE INDEX IF NOT EXISTS idx_memories_scope_status
           ON memories(scope_type, scope_key, status);
         CREATE INDEX IF NOT EXISTS idx_memories_active_pins
           ON memories(scope_type, scope_key, salience DESC, updated_at DESC)
           WHERE pinned = 1 AND status = 'active';
         CREATE INDEX IF NOT EXISTS idx_memories_expiry ON memories(expires_at);
         CREATE INDEX IF NOT EXISTS idx_memories_supersedes_id ON memories(supersedes_id);",
    )?;
    Ok(())
}

fn migrate_scoped_memories(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
           key TEXT PRIMARY KEY NOT NULL,
           value TEXT NOT NULL
         );",
    )?;
    let already_migrated: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_meta WHERE key = 'scoped_memory_v1')",
        [],
        |row| row.get(0),
    )?;
    if already_migrated {
        ensure_scoped_memory_indexes(conn)?;
        return Ok(());
    }

    if table_has_column(conn, "memories", "scope_type")? {
        ensure_scoped_memory_indexes(conn)?;
        conn.execute(
            "INSERT INTO schema_meta(key, value) VALUES ('scoped_memory_v1', 'complete')",
            [],
        )?;
        return Ok(());
    }

    // Foreign keys must be disabled before BEGIN for SQLite table rebuilds.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let migration_result = (|| -> Result<(), DbError> {
        let transaction = conn.unchecked_transaction()?;
        transaction.execute_batch(SCOPED_MEMORIES_TABLE)?;
        transaction.execute_batch(
            "INSERT INTO memories_scoped (
               id, workflow_id, run_id, node_id, scope_type, scope_key, kind,
               memory_type, source, title, body, artifact_path, pinned,
               confidence, salience, status, supersedes_id, last_confirmed_at,
               expires_at, created_at, updated_at
             )
             SELECT id, workflow_id, run_id, node_id, 'workflow', workflow_id, kind,
                    CASE
                      WHEN kind = 'artifact' THEN 'artifact'
                      WHEN source = 'manual' THEN 'note'
                      ELSE 'output'
                    END,
                    source, title, body, artifact_path, pinned,
                    1.0, 50, 'active', NULL, NULL, NULL, created_at, updated_at
             FROM memories;
             CREATE TABLE memory_links_scoped (
               id TEXT PRIMARY KEY NOT NULL,
               workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
               memory_id TEXT NOT NULL REFERENCES memories_scoped(id) ON DELETE CASCADE,
               created_at TEXT NOT NULL,
               UNIQUE (workflow_id, memory_id)
             );
             INSERT INTO memory_links_scoped (id, workflow_id, memory_id, created_at)
               SELECT id, workflow_id, memory_id, created_at FROM memory_links;
             DROP TABLE memory_links;
             DROP TABLE memories;
             ALTER TABLE memories_scoped RENAME TO memories;
             ALTER TABLE memory_links_scoped RENAME TO memory_links;
             CREATE INDEX idx_memories_scope_status
               ON memories(scope_type, scope_key, status);
             CREATE INDEX idx_memories_active_pins
               ON memories(scope_type, scope_key, salience DESC, updated_at DESC)
               WHERE pinned = 1 AND status = 'active';
             CREATE INDEX idx_memories_expiry ON memories(expires_at);
             CREATE INDEX idx_memories_supersedes_id ON memories(supersedes_id);
             CREATE INDEX idx_memory_links_workflow_id ON memory_links(workflow_id);
             CREATE INDEX idx_memory_links_memory_id ON memory_links(memory_id);
             DELETE FROM memory_fts;",
        )?;

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
        transaction.execute(
            "INSERT INTO schema_meta(key, value) VALUES ('scoped_memory_v1', 'complete')",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    })();
    let enable_result = conn.execute_batch("PRAGMA foreign_keys = ON;");
    migration_result?;
    enable_result?;
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
    use rusqlite::params;

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("prepare table info");
        stmt.query_map([], |row| row.get(1))
            .expect("query columns")
            .collect::<Result<Vec<String>, _>>()
            .expect("collect columns")
    }

    fn legacy_memory_fixture() -> Connection {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize supporting schema");
        conn.execute_batch(
            "DROP TABLE memory_links;
             DROP TABLE memory_fts;
             DROP TABLE memories;
             CREATE TABLE memories (
               id TEXT PRIMARY KEY NOT NULL,
               workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
               run_id TEXT,
               node_id TEXT,
               kind TEXT NOT NULL DEFAULT 'text' CHECK (kind IN ('text', 'note', 'artifact')),
               source TEXT NOT NULL DEFAULT 'run' CHECK (source IN ('run', 'manual', 'import')),
               title TEXT NOT NULL,
               body TEXT NOT NULL DEFAULT '',
               artifact_path TEXT,
               pinned INTEGER NOT NULL DEFAULT 0,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             CREATE TABLE memory_links (
               id TEXT PRIMARY KEY NOT NULL,
               workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
               memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
               created_at TEXT NOT NULL,
               UNIQUE (workflow_id, memory_id)
             );
             CREATE VIRTUAL TABLE memory_fts USING fts5(
               memory_id UNINDEXED,
               workflow_id UNINDEXED,
               title,
               body,
               tokenize = 'unicode61 remove_diacritics 2'
             );",
        )
        .expect("initialize legacy memory schema");
        conn.execute_batch(
            "INSERT INTO workflows
               (id, name, description, working_directory, sort_order, graph_json, created_at, updated_at)
             VALUES
               ('workflow-a', 'Alpha', '', '/tmp/alpha', 0, '{\"nodes\":[],\"edges\":[]}',
                '2026-08-18T08:00:00Z', '2026-08-18T08:00:00Z'),
               ('workflow-b', 'Beta', '', '/tmp/beta', 1, '{\"nodes\":[],\"edges\":[]}',
                '2026-08-18T08:00:00Z', '2026-08-18T08:00:00Z');
             INSERT INTO memories
               (id, workflow_id, run_id, node_id, kind, source, title, body,
                artifact_path, pinned, created_at, updated_at)
             VALUES
               ('output-1', 'workflow-a', 'run-1', 'node-1', 'text', 'run',
                'Ordinary output', 'ordinary searchable body', NULL, 0,
                '2026-08-18T09:00:00Z', '2026-08-18T09:01:00Z'),
               ('note-1', 'workflow-a', NULL, NULL, 'note', 'manual',
                'Manual note', 'manual searchable body', NULL, 0,
                '2026-08-18T09:02:00Z', '2026-08-18T09:03:00Z'),
               ('artifact-1', 'workflow-a', 'run-2', 'node-2', 'artifact', 'run',
                'Artifact output', 'artifact searchable preview', '/tmp/legacy-artifact.txt', 0,
                '2026-08-18T09:04:00Z', '2026-08-18T09:05:00Z'),
               ('pinned-1', 'workflow-a', NULL, NULL, 'text', 'import',
                'Pinned import', 'pinned searchable body', NULL, 1,
                '2026-08-18T09:06:00Z', '2026-08-18T09:07:00Z');
             INSERT INTO memory_links (id, workflow_id, memory_id, created_at)
             VALUES ('link-1', 'workflow-b', 'output-1', '2026-08-18T09:08:00Z');
             INSERT INTO memory_fts (memory_id, workflow_id, title, body)
             SELECT id, workflow_id, title, body FROM memories;",
        )
        .expect("insert legacy memory fixture");
        conn
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

    #[test]
    fn migrates_legacy_memories_without_losing_canonical_or_search_data() {
        let conn = legacy_memory_fixture();

        apply_migrations(&conn).expect("migrate legacy memories");
        apply_migrations(&conn).expect("repeat migration is a no-op");

        let mut statement = conn
            .prepare(
                "SELECT id, workflow_id, scope_type, scope_key, kind, memory_type,
                        source, title, body, artifact_path, pinned, created_at, updated_at
                 FROM memories ORDER BY created_at",
            )
            .expect("prepare migrated memories");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                ))
            })
            .expect("query migrated memories")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect migrated memories");

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, "output-1");
        assert_eq!(rows[0].1.as_deref(), Some("workflow-a"));
        assert_eq!(
            (&rows[0].2, &rows[0].3, &rows[0].5),
            (&"workflow".into(), &"workflow-a".into(), &"output".into())
        );
        assert_eq!(
            (&rows[0].6, &rows[0].8, &rows[0].11, &rows[0].12),
            (
                &"run".into(),
                &"ordinary searchable body".into(),
                &"2026-08-18T09:00:00Z".into(),
                &"2026-08-18T09:01:00Z".into()
            )
        );
        assert_eq!(
            (&rows[1].0, &rows[1].4, &rows[1].5, &rows[1].6),
            (
                &"note-1".into(),
                &"note".into(),
                &"note".into(),
                &"manual".into()
            )
        );
        assert_eq!(
            (&rows[2].0, &rows[2].5, rows[2].9.as_deref()),
            (
                &"artifact-1".into(),
                &"artifact".into(),
                Some("/tmp/legacy-artifact.txt")
            )
        );
        assert_eq!((&rows[3].0, rows[3].10), (&"pinned-1".into(), 1));

        let link: (String, String, String, String) = conn
            .query_row(
                "SELECT id, workflow_id, memory_id, created_at FROM memory_links WHERE id = ?1",
                params!["link-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load preserved memory link");
        assert_eq!(
            link,
            (
                "link-1".into(),
                "workflow-b".into(),
                "output-1".into(),
                "2026-08-18T09:08:00Z".into()
            )
        );

        for (term, expected_id) in [
            ("ordinary", "output-1"),
            ("manual", "note-1"),
            ("artifact", "artifact-1"),
            ("pinned", "pinned-1"),
        ] {
            let hit: String = conn
                .query_row(
                    "SELECT memory_id FROM memory_fts WHERE memory_fts MATCH ?1",
                    params![term],
                    |row| row.get(0),
                )
                .expect("search migrated memory");
            assert_eq!(hit, expected_id);
        }

        let fk_violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .expect("check foreign keys");
        assert_eq!(fk_violations, 0);
    }

    #[test]
    fn fresh_schema_enforces_scoped_memory_constraints_and_indexes() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");

        let names = columns(&conn, "memories");
        for expected in [
            "scope_type",
            "scope_key",
            "memory_type",
            "confidence",
            "salience",
            "status",
            "supersedes_id",
            "last_confirmed_at",
            "expires_at",
        ] {
            assert!(names.contains(&expected.to_owned()), "missing {expected}");
        }

        let indexes = {
            let mut statement = conn
                .prepare("SELECT name FROM pragma_index_list('memories')")
                .expect("prepare index list");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query index list")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect indexes")
        };
        for expected in [
            "idx_memories_scope_status",
            "idx_memories_active_pins",
            "idx_memories_expiry",
            "idx_memories_supersedes_id",
        ] {
            assert!(indexes.contains(&expected.to_owned()), "missing {expected}");
        }

        conn.execute(
            "INSERT INTO workflows
               (id, name, description, working_directory, sort_order, graph_json, created_at, updated_at)
             VALUES ('workflow-1', 'One', '', '/tmp/one', 0, '{}', 'now', 'now')",
            [],
        )
        .expect("insert workflow");
        conn.execute(
            "INSERT INTO memories
               (id, workflow_id, scope_type, scope_key, memory_type, source, title, body,
                created_at, updated_at)
             VALUES ('user-1', NULL, 'user', 'local-user', 'preference', 'review',
                     'Preference', 'Body', 'now', 'now')",
            [],
        )
        .expect("insert valid user memory");
        assert!(conn
            .execute(
                "INSERT INTO memories
                   (id, workflow_id, scope_type, scope_key, title, created_at, updated_at)
                 VALUES ('invalid-1', NULL, 'workflow', 'workflow-1', 'Invalid', 'now', 'now')",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "INSERT INTO memories
                   (id, workflow_id, scope_type, scope_key, title, confidence, created_at, updated_at)
                 VALUES ('invalid-2', 'workflow-1', 'workflow', 'workflow-1',
                         'Invalid', 1.1, 'now', 'now')",
                [],
            )
            .is_err());

        let marker: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'scoped_memory_v1'",
                [],
                |row| row.get(0),
            )
            .expect("load migration marker");
        assert_eq!(marker, "complete");
    }
}
