use rusqlite::Connection;

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
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_triggers_workflow_id ON triggers(workflow_id);
         CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled);",
    )?;

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
