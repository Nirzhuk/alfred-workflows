use rusqlite::Connection;

use super::history::{index_memory, index_run_step};
use super::DbError;

/// Apply compatibility migrations for databases created with earlier schemas.
/// Most are additive; contract changes use explicit transactional rebuilds.
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
    expand_agent_provider_check(conn)?;
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
    ensure_column(
        conn,
        "workflows",
        "memory_retrieval_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
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
    create_memory_retrieval_schema(conn)?;
    create_memory_curation_schema(conn)?;
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
    migrate_agent_account_contract(conn)?;
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
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS license_snapshot (
           id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
           product TEXT NOT NULL,
           status TEXT NOT NULL,
           masked_key TEXT,
           benefit_id TEXT,
           activation_label TEXT,
           current_device INTEGER NOT NULL DEFAULT 0,
           expires_at TEXT,
           last_success_at TEXT,
           refresh_due_at TEXT,
           offline_deadline TEXT,
           error_code TEXT,
           credential_ref TEXT UNIQUE,
           updated_at TEXT NOT NULL
         );",
    )?;

    Ok(())
}

const AGENT_ACCOUNTS_CONTRACT_TABLE: &str = "
  CREATE TABLE agent_accounts_contract (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    harness TEXT NOT NULL,
    identity_key TEXT NOT NULL,
    display_name TEXT,
    external_account_id TEXT,
    external_workspace_id TEXT,
    auth_method TEXT NOT NULL,
    custody_mode TEXT NOT NULL,
    managed_runtime_id TEXT,
    managed_runtime_version TEXT,
    runtime_profile_ref TEXT UNIQUE,
    scopes_json TEXT NOT NULL DEFAULT '[]',
    billing_source TEXT NOT NULL,
    billing_owner TEXT NOT NULL,
    entitlement_state TEXT NOT NULL DEFAULT 'unknown'
      CHECK (entitlement_state IN ('unknown', 'eligible', 'limited', 'exhausted', 'ineligible')),
    entitlement_source TEXT NOT NULL,
    entitlement_observed_at TEXT,
    status TEXT NOT NULL DEFAULT 'error'
      CHECK (status IN ('connected', 'expired', 'error', 'revoked', 'disconnect_pending')),
    expires_at TEXT,
    last_checked_at TEXT,
    last_error_code TEXT,
    credential_ref TEXT UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK ((managed_runtime_id IS NULL) = (managed_runtime_version IS NULL)),
    CHECK (runtime_profile_ref IS NULL OR managed_runtime_id IS NOT NULL),
    CHECK (runtime_profile_ref IS NOT NULL OR credential_ref IS NOT NULL)
  );";

const AGENT_ACCOUNT_CREDENTIAL_CLEANUP_TABLE: &str = "
  CREATE TABLE IF NOT EXISTS agent_account_credential_cleanup (
    account_id TEXT PRIMARY KEY NOT NULL,
    credential_ref TEXT NOT NULL UNIQUE,
    cleanup_owner TEXT NOT NULL,
    created_at TEXT NOT NULL
  );";

fn migrate_agent_account_contract(conn: &Connection) -> Result<(), DbError> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute_batch("DROP TABLE IF EXISTS agent_accounts_contract;")?;
    transaction.execute_batch(AGENT_ACCOUNT_CREDENTIAL_CLEANUP_TABLE)?;

    if !table_has_column(&transaction, "agent_accounts", "id")? {
        transaction.execute_batch(AGENT_ACCOUNTS_CONTRACT_TABLE)?;
        transaction
            .execute_batch("ALTER TABLE agent_accounts_contract RENAME TO agent_accounts;")?;
        create_agent_account_indexes(&transaction)?;
        transaction.commit()?;
        return Ok(());
    }
    if table_has_column(&transaction, "agent_accounts", "product_id")? {
        create_agent_account_indexes(&transaction)?;
        transaction.commit()?;
        return Ok(());
    }

    let unsupported: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM agent_accounts
         WHERE provider_id NOT IN (
           'claude_code', 'cursor', 'codex', 'opencode',
           'github_copilot', 'gemini', 'grok'
         )",
        [],
        |row| row.get(0),
    )?;
    if unsupported != 0 {
        return Err(DbError::Other(
            "legacy native account has no registered product route".into(),
        ));
    }

    transaction.execute_batch("DROP INDEX IF EXISTS idx_agent_accounts_identity;")?;
    transaction.execute_batch(AGENT_ACCOUNTS_CONTRACT_TABLE)?;
    transaction.execute_batch(
        "INSERT OR IGNORE INTO agent_account_credential_cleanup (
           account_id, credential_ref, cleanup_owner, created_at
         )
         SELECT id, credential_ref, 'legacy_agent_account_migration', updated_at
         FROM agent_accounts
         WHERE (provider_id = 'claude_code' AND auth_method <> 'api_key')
            OR (provider_id = 'codex' AND auth_method <> 'api_key')
            OR (provider_id = 'opencode' AND custody_mode = 'runtime_managed');
         INSERT INTO agent_accounts_contract (
           id, provider_id, product_id, harness, identity_key, display_name,
           external_account_id, external_workspace_id, auth_method, custody_mode,
           managed_runtime_id, managed_runtime_version, runtime_profile_ref,
           scopes_json, billing_source, billing_owner, entitlement_state,
           entitlement_source, entitlement_observed_at, status, expires_at,
           last_checked_at, last_error_code, credential_ref, created_at, updated_at
         )
         SELECT
           id,
           provider_id,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method = 'api_key' THEN 'claude_api'
             WHEN provider_id = 'claude_code' THEN 'claude_code_subscription'
             WHEN provider_id = 'codex' AND auth_method = 'api_key' THEN 'openai_api'
             WHEN provider_id = 'codex' THEN 'chatgpt_codex'
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN 'opencode_go'
             WHEN provider_id = 'opencode' THEN 'opencode_zen'
             WHEN provider_id = 'cursor' THEN 'cursor_cloud'
             WHEN provider_id = 'github_copilot' THEN 'github_copilot_subscription'
             WHEN provider_id = 'gemini' THEN 'gemini_api'
             WHEN provider_id = 'grok' THEN 'grok_api'
           END,
           harness,
           identity_key,
           display_name,
           external_account_id,
           external_workspace_id,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'runtime'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'device_code'
             WHEN provider_id IN ('opencode', 'cursor', 'gemini', 'grok') THEN 'api_key'
             WHEN provider_id = 'github_copilot' THEN 'device_code'
             ELSE auth_method
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'runtime_managed'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'runtime_managed'
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN 'runtime_managed'
             ELSE 'alfred_managed'
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'claude_code_managed'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'codex_python_sdk'
             WHEN provider_id = 'opencode' THEN 'opencode_server'
             ELSE NULL
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN '2.1.246'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN '0.147.0'
             WHEN provider_id = 'opencode' THEN '1.18.23'
             ELSE NULL
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'migration-profile:' || id
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'migration-profile:' || id
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN 'migration-profile:' || id
             WHEN provider_id = 'opencode' THEN 'migration-profile:' || id
             ELSE NULL
           END,
           scopes_json,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'provider_subscription'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'provider_subscription'
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN 'provider_subscription'
             WHEN provider_id = 'github_copilot' THEN 'provider_subscription'
             WHEN provider_id = 'opencode' THEN 'provider_payg'
             ELSE 'provider_api'
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'subscription_account'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'subscription_account'
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN 'subscription_account'
             WHEN provider_id = 'github_copilot' THEN 'subscription_account'
             ELSE 'credential_owner'
           END,
           'unknown',
           'migration',
           NULL,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'error'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'error'
             WHEN provider_id = 'opencode' THEN 'error'
             ELSE status
           END,
           expires_at,
           last_checked_at,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN 'managed_runtime_reconnect_required'
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN 'managed_runtime_reconnect_required'
             WHEN provider_id = 'opencode' THEN 'managed_runtime_reconnect_required'
             ELSE last_error_code
           END,
           CASE
             WHEN provider_id = 'claude_code' AND auth_method <> 'api_key' THEN NULL
             WHEN provider_id = 'codex' AND auth_method <> 'api_key' THEN NULL
             WHEN provider_id = 'opencode' AND custody_mode = 'runtime_managed' THEN NULL
             ELSE credential_ref
           END,
           created_at,
           updated_at
         FROM agent_accounts;
         DROP TABLE agent_accounts;
         ALTER TABLE agent_accounts_contract RENAME TO agent_accounts;",
    )?;
    create_agent_account_indexes(&transaction)?;
    transaction.commit()?;
    Ok(())
}

fn create_agent_account_indexes(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_agent_accounts_identity;
         CREATE UNIQUE INDEX idx_agent_accounts_identity
           ON agent_accounts(provider_id, product_id, harness, identity_key);
         CREATE INDEX IF NOT EXISTS idx_agent_accounts_provider_id
           ON agent_accounts(provider_id);
         CREATE INDEX IF NOT EXISTS idx_agent_accounts_product_id
           ON agent_accounts(product_id);",
    )?;
    Ok(())
}

fn create_memory_retrieval_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS run_memory_uses (
           id TEXT PRIMARY KEY NOT NULL,
           run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
           node_id TEXT NOT NULL,
           memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
           rank INTEGER NOT NULL,
           score REAL NOT NULL,
           reason TEXT NOT NULL CHECK (reason IN ('lexical', 'recent', 'pinned')),
           rendered_bytes INTEGER NOT NULL,
           created_at TEXT NOT NULL,
           UNIQUE (run_id, node_id, memory_id)
         );
         CREATE INDEX IF NOT EXISTS idx_run_memory_uses_run_node_rank
           ON run_memory_uses(run_id, node_id, rank);
         CREATE INDEX IF NOT EXISTS idx_run_memory_uses_memory_id
           ON run_memory_uses(memory_id);",
    )?;
    Ok(())
}

fn create_memory_curation_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_review_settings (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           enabled INTEGER NOT NULL DEFAULT 0,
           provider TEXT,
           model TEXT,
           max_candidates INTEGER NOT NULL DEFAULT 5 CHECK (max_candidates BETWEEN 1 AND 5),
           updated_at TEXT NOT NULL
         );
         INSERT OR IGNORE INTO memory_review_settings
           (id, enabled, provider, model, max_candidates, updated_at)
         VALUES (1, 0, NULL, NULL, 5, '1970-01-01T00:00:00Z');
         CREATE TABLE IF NOT EXISTS workflow_memory_review (
           workflow_id TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
           enabled INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS memory_reviews (
           run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
           workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
           status TEXT NOT NULL CHECK (
             status IN ('pending','running','completed','failed','skipped')
           ),
           provider TEXT NOT NULL,
           model TEXT,
           error_code TEXT,
           candidate_count INTEGER NOT NULL DEFAULT 0,
           started_at TEXT,
           finished_at TEXT,
           created_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS memory_candidates (
           id TEXT PRIMARY KEY NOT NULL,
           review_run_id TEXT NOT NULL REFERENCES memory_reviews(run_id) ON DELETE CASCADE,
           workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
           source_node_id TEXT,
           operation TEXT NOT NULL CHECK (operation IN ('create','supersede','retract')),
           target_memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
           scope_type TEXT NOT NULL CHECK (scope_type IN ('user','workspace','workflow')),
           scope_key TEXT NOT NULL,
           memory_type TEXT NOT NULL,
           title TEXT NOT NULL,
           body TEXT NOT NULL,
           confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
           rationale TEXT NOT NULL,
           content_hash TEXT NOT NULL,
           status TEXT NOT NULL CHECK (
             status IN ('pending','approved','rejected','blocked')
           ),
           blocked_code TEXT,
           created_at TEXT NOT NULL,
           decided_at TEXT,
           UNIQUE (review_run_id, content_hash)
         );
         CREATE INDEX IF NOT EXISTS idx_memory_candidates_status_workflow
           ON memory_candidates(status, workflow_id);
         CREATE INDEX IF NOT EXISTS idx_memory_candidates_created
           ON memory_candidates(created_at);
         CREATE INDEX IF NOT EXISTS idx_memory_reviews_status ON memory_reviews(status);
         CREATE INDEX IF NOT EXISTS idx_memory_reviews_created ON memory_reviews(created_at);",
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

/// Add newly supported local agent CLIs to the legacy `agents` table check.
/// SQLite cannot ALTER a CHECK constraint, so rebuild the small table once.
fn expand_agent_provider_check(conn: &Connection) -> Result<(), DbError> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'agents'",
            [],
            |row| row.get(0),
        )
        .ok();

    let Some(sql) = sql else { return Ok(()) };
    if !sql.contains("CHECK (provider")
        || (sql.contains("github_copilot")
            && sql.contains("gemini")
            && sql.contains("grok")
            && sql.contains("'pi'")
            && sql.contains("'omp'"))
    {
        return Ok(());
    }

    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         BEGIN;
         CREATE TABLE agents_migrated (
           id TEXT PRIMARY KEY NOT NULL,
           provider TEXT NOT NULL CHECK (provider IN ('claude_code', 'cursor', 'codex', 'opencode', 'github_copilot', 'gemini', 'grok', 'pi', 'omp')),
           name TEXT NOT NULL,
           config_json TEXT NOT NULL DEFAULT '{}',
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         INSERT INTO agents_migrated (id, provider, name, config_json, created_at, updated_at)
           SELECT id, provider, name, config_json, created_at, updated_at FROM agents;
         DROP TABLE agents;
         ALTER TABLE agents_migrated RENAME TO agents;
         COMMIT;
         PRAGMA foreign_keys = ON;",
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

    const LEGACY_AGENT_ACCOUNT_SCHEMA: &str = "
      CREATE TABLE agent_accounts (
        id TEXT PRIMARY KEY NOT NULL,
        provider_id TEXT NOT NULL,
        harness TEXT NOT NULL,
        identity_key TEXT NOT NULL,
        display_name TEXT,
        external_account_id TEXT,
        external_workspace_id TEXT,
        auth_method TEXT NOT NULL,
        custody_mode TEXT NOT NULL,
        scopes_json TEXT NOT NULL DEFAULT '[]',
        status TEXT NOT NULL DEFAULT 'error',
        expires_at TEXT,
        last_checked_at TEXT,
        last_error_code TEXT,
        credential_ref TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE UNIQUE INDEX idx_agent_accounts_identity
        ON agent_accounts(provider_id, harness, identity_key);
      CREATE INDEX idx_agent_accounts_provider_id
        ON agent_accounts(provider_id);";

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
    fn initializes_secret_free_agent_accounts_on_empty_and_existing_databases() {
        for drop_new_table in [false, true] {
            let conn = Connection::open_in_memory().expect("open database");
            conn.execute_batch(include_str!("schema.sql"))
                .expect("initialize schema");
            if drop_new_table {
                conn.execute_batch("DROP TABLE agent_accounts;")
                    .expect("create legacy fixture");
            }
            apply_migrations(&conn).expect("apply migrations");

            let names = columns(&conn, "agent_accounts");
            assert!(names.contains(&"identity_key".to_owned()));
            assert!(names.contains(&"product_id".to_owned()));
            assert!(names.contains(&"credential_ref".to_owned()));
            assert!(names.contains(&"custody_mode".to_owned()));
            assert!(names.contains(&"managed_runtime_id".to_owned()));
            assert!(names.contains(&"runtime_profile_ref".to_owned()));
            assert!(names.contains(&"billing_owner".to_owned()));
            assert!(names.contains(&"entitlement_state".to_owned()));
            assert!(!names.iter().any(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("token")
                    || name.contains("secret")
                    || name.contains("authorization_code")
                    || name.contains("verifier")
                    || name.contains("nonce")
            }));
        }
    }

    #[test]
    fn production_order_upgrades_legacy_agent_accounts_before_product_indexes() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(LEGACY_AGENT_ACCOUNT_SCHEMA)
            .expect("initialize legacy agent account schema");
        conn.execute(
            "INSERT INTO agent_accounts (
               id, provider_id, harness, identity_key, auth_method, custody_mode,
               status, credential_ref, created_at, updated_at
             ) VALUES (
               'legacy-api', 'gemini', 'alfred', 'identity', 'api_key',
               'alfred_managed', 'connected', 'legacy-secret', 'now', 'now'
             )",
            [],
        )
        .expect("insert legacy account");

        conn.execute_batch(include_str!("schema.sql"))
            .expect("run current schema before migrations");
        assert!(!columns(&conn, "agent_accounts").contains(&"product_id".to_owned()));

        apply_migrations(&conn).expect("upgrade production-order schema");
        let migrated: (String, String) = conn
            .query_row(
                "SELECT product_id, credential_ref
                 FROM agent_accounts WHERE id = 'legacy-api'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load migrated account");
        assert_eq!(migrated, ("gemini_api".into(), "legacy-secret".into()));
        let indexes = {
            let mut statement = conn
                .prepare("SELECT name FROM pragma_index_list('agent_accounts')")
                .expect("prepare index list");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query indexes")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect indexes")
        };
        assert!(indexes.contains(&"idx_agent_accounts_identity".to_owned()));
        assert!(indexes.contains(&"idx_agent_accounts_product_id".to_owned()));
    }

    #[test]
    fn agent_account_rebuild_discards_stale_scratch_and_retries_transactionally() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(LEGACY_AGENT_ACCOUNT_SCHEMA)
            .expect("initialize legacy agent account schema");
        conn.execute_batch(
            "INSERT INTO agent_accounts (
               id, provider_id, harness, identity_key, auth_method, custody_mode,
               status, credential_ref, created_at, updated_at
             ) VALUES (
               'legacy-api', 'grok', 'alfred', 'identity', 'api_key',
               'alfred_managed', 'connected', 'legacy-secret', 'now', 'now'
             );
             CREATE TABLE agent_accounts_contract (stale TEXT NOT NULL);",
        )
        .expect("create interrupted rebuild fixture");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("run current schema before migrations");

        apply_migrations(&conn).expect("retry interrupted rebuild");
        assert_eq!(
            conn.query_row(
                "SELECT product_id FROM agent_accounts WHERE id = 'legacy-api'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("load retried row"),
            "grok_api"
        );
        assert!(!table_has_column(&conn, "agent_accounts_contract", "stale")
            .expect("inspect scratch table"));
    }

    #[test]
    fn migrates_legacy_api_and_subscription_rows_to_distinct_access_routes() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        conn.execute_batch(
            "DROP TABLE agent_accounts;
             CREATE TABLE agent_accounts (
               id TEXT PRIMARY KEY NOT NULL,
               provider_id TEXT NOT NULL,
               harness TEXT NOT NULL,
               identity_key TEXT NOT NULL,
               display_name TEXT,
               external_account_id TEXT,
               external_workspace_id TEXT,
               auth_method TEXT NOT NULL,
               custody_mode TEXT NOT NULL,
               scopes_json TEXT NOT NULL DEFAULT '[]',
               status TEXT NOT NULL,
               expires_at TEXT,
               last_checked_at TEXT,
               last_error_code TEXT,
               credential_ref TEXT NOT NULL UNIQUE,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             INSERT INTO agent_accounts VALUES
               ('api', 'codex', 'alfred', 'identity-api', NULL, 'api-user', NULL,
                'api_key', 'alfred_managed', '[]', 'connected', NULL, NULL, NULL,
                'secret-api', 'now', 'now'),
               ('subscription', 'codex', 'alfred', 'identity-sub', NULL, 'chatgpt-user', NULL,
                'oauth_pkce', 'runtime_managed', '[]', 'connected', NULL, NULL, NULL,
                'profile-sub', 'now', 'now');",
        )
        .expect("legacy account fixture");

        apply_migrations(&conn).expect("migrate accounts");
        let api: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT product_id, runtime_profile_ref, credential_ref
                 FROM agent_accounts WHERE id = 'api'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("API route");
        assert_eq!(api, ("openai_api".into(), None, Some("secret-api".into())));

        let subscription: (String, String, String, Option<String>, String) = conn
            .query_row(
                "SELECT product_id, managed_runtime_id, runtime_profile_ref,
                        credential_ref, status
                 FROM agent_accounts WHERE id = 'subscription'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("subscription route");
        assert_eq!(
            subscription,
            (
                "chatgpt_codex".into(),
                "codex_python_sdk".into(),
                "migration-profile:subscription".into(),
                None,
                "error".into(),
            )
        );
        let cleanup: (String, String) = conn
            .query_row(
                "SELECT credential_ref, cleanup_owner
                 FROM agent_account_credential_cleanup
                 WHERE account_id = 'subscription'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("legacy credential cleanup route");
        assert_eq!(
            cleanup,
            (
                "profile-sub".into(),
                "legacy_agent_account_migration".into()
            )
        );
    }

    #[test]
    fn upgrades_an_existing_schema_without_connection_metadata() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize legacy fixture");
        conn.execute_batch(
            "DROP TABLE app_connections;
             ALTER TABLE workflows DROP COLUMN memory_retrieval_enabled;",
        )
        .expect("remove new table from fixture");

        conn.execute(
            "INSERT INTO workflows
               (id, name, description, working_directory, sort_order, graph_json, created_at, updated_at)
             VALUES ('legacy-workflow', 'Legacy', '', '', 0, '{}', 'now', 'now')",
            [],
        )
        .expect("insert existing workflow before recall migration");

        apply_migrations(&conn).expect("upgrade fixture");

        assert!(columns(&conn, "app_connections").contains(&"identity_key".to_owned()));
        assert!(columns(&conn, "app_connections").contains(&"provider_metadata_json".to_owned()));
        let enabled: i64 = conn
            .query_row(
                "SELECT memory_retrieval_enabled FROM workflows WHERE id = 'legacy-workflow'",
                [],
                |row| row.get(0),
            )
            .expect("load migrated recall default");
        assert_eq!(enabled, 0, "existing workflows must remain recall-off");
    }

    #[test]
    fn initializes_memory_retrieval_rollout_and_audit_cascades() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");

        assert!(columns(&conn, "workflows").contains(&"memory_retrieval_enabled".to_owned()));
        for expected in [
            "run_id",
            "node_id",
            "memory_id",
            "rank",
            "score",
            "reason",
            "rendered_bytes",
            "created_at",
        ] {
            assert!(
                columns(&conn, "run_memory_uses").contains(&expected.to_owned()),
                "missing {expected}"
            );
        }
        let indexes = {
            let mut statement = conn
                .prepare("SELECT name FROM pragma_index_list('run_memory_uses')")
                .expect("prepare audit indexes");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query audit indexes")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect audit indexes")
        };
        assert!(indexes.contains(&"idx_run_memory_uses_run_node_rank".to_owned()));
        assert!(indexes.contains(&"idx_run_memory_uses_memory_id".to_owned()));

        conn.execute_batch(
            "INSERT INTO workflows
               (id, name, description, graph_json, created_at, updated_at)
             VALUES ('workflow-1', 'One', '', '{}', 'now', 'now');
             INSERT INTO runs (id, workflow_id, created_at)
             VALUES ('run-1', 'workflow-1', 'now'), ('run-2', 'workflow-1', 'now');
             INSERT INTO memories
               (id, workflow_id, scope_type, scope_key, title, body, created_at, updated_at)
             VALUES ('memory-1', 'workflow-1', 'workflow', 'workflow-1', 'One', 'Body', 'now', 'now'),
                    ('memory-2', 'workflow-1', 'workflow', 'workflow-1', 'Two', 'Body', 'now', 'now');
             INSERT INTO run_memory_uses
               (id, run_id, node_id, memory_id, rank, score, reason, rendered_bytes, created_at)
             VALUES ('use-run', 'run-1', 'agent', 'memory-1', 1, 100.0, 'lexical', 100, 'now'),
                    ('use-memory', 'run-2', 'agent', 'memory-2', 1, 20.0, 'recent', 80, 'now');",
        )
        .expect("insert audit fixtures");
        assert!(conn
            .execute(
                "INSERT INTO run_memory_uses
                   (id, run_id, node_id, memory_id, rank, score, reason, rendered_bytes, created_at)
                 VALUES ('bad-reason', 'run-1', 'other', 'memory-1', 1, 0, 'semantic', 0, 'now')",
                [],
            )
            .is_err());

        conn.execute("DELETE FROM runs WHERE id = 'run-1'", [])
            .expect("delete run");
        conn.execute("DELETE FROM memories WHERE id = 'memory-2'", [])
            .expect("delete memory");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM run_memory_uses", [], |row| row.get(0))
            .expect("count audit rows");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn expands_legacy_agent_provider_check() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize legacy fixture");
        conn.execute_batch(
            "DROP TABLE agents;
             CREATE TABLE agents (
               id TEXT PRIMARY KEY NOT NULL,
               provider TEXT NOT NULL CHECK (provider IN ('claude_code', 'cursor', 'codex', 'opencode')),
               name TEXT NOT NULL,
               config_json TEXT NOT NULL DEFAULT '{}',
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             INSERT INTO agents (id, provider, name, created_at, updated_at)
             VALUES ('legacy', 'opencode', 'Legacy agent', 'now', 'now');",
        )
        .expect("create legacy agents table");

        apply_migrations(&conn).expect("upgrade agents table");

        conn.execute(
            "INSERT INTO agents (id, provider, name, created_at, updated_at)
             VALUES ('gemini', 'gemini', 'Gemini', 'now', 'now')",
            [],
        )
        .expect("accept new provider");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agents", [], |row| row.get(0))
            .expect("count agents");
        assert_eq!(count, 2);
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
    fn additive_license_migration_preserves_existing_data() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        conn.execute(
            "INSERT INTO workflows (id, name, description, graph_json, created_at, updated_at)
             VALUES ('existing', 'Existing workflow', '', '{}', 'now', 'now')",
            [],
        )
        .expect("insert existing row");
        conn.execute_batch("DROP TABLE license_snapshot;")
            .expect("remove new table from legacy fixture");

        apply_migrations(&conn).expect("upgrade fixture");

        let name: String = conn
            .query_row(
                "SELECT name FROM workflows WHERE id = 'existing'",
                [],
                |row| row.get(0),
            )
            .expect("existing row");
        assert_eq!(name, "Existing workflow");
        let names = columns(&conn, "license_snapshot");
        assert!(names.contains(&"masked_key".to_owned()));
        assert!(names.contains(&"credential_ref".to_owned()));
        assert!(!names.contains(&"license_key".to_owned()));
        assert!(!names.contains(&"activation_id".to_owned()));
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

    #[test]
    fn initializes_memory_curation_tables_singleton_and_indexes() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");

        for expected in [
            "enabled",
            "provider",
            "model",
            "max_candidates",
            "updated_at",
        ] {
            assert!(
                columns(&conn, "memory_review_settings").contains(&expected.to_owned()),
                "missing settings column {expected}"
            );
        }
        for expected in [
            "run_id",
            "workflow_id",
            "status",
            "provider",
            "model",
            "error_code",
            "candidate_count",
        ] {
            assert!(
                columns(&conn, "memory_reviews").contains(&expected.to_owned()),
                "missing review column {expected}"
            );
        }

        // Singleton defaults: disabled with null provider/model.
        let (enabled, provider, model, max_candidates): (i64, Option<String>, Option<String>, i64) =
            conn.query_row(
                "SELECT enabled, provider, model, max_candidates FROM memory_review_settings
                 WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load singleton settings");
        assert_eq!(
            (
                enabled,
                provider.as_deref(),
                model.as_deref(),
                max_candidates
            ),
            (0, None, None, 5)
        );
        assert!(conn
            .execute(
                "INSERT INTO memory_review_settings (id, enabled, max_candidates, updated_at)
                 VALUES (2, 0, 5, 'now')",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "UPDATE memory_review_settings SET max_candidates = 9 WHERE id = 1",
                [],
            )
            .is_err());

        // Review status enum rejects unknown values.
        conn.execute_batch(
            "INSERT INTO workflows (id, name, description, graph_json, created_at, updated_at)
             VALUES ('workflow-1', 'One', '', '{}', 'now', 'now');
             INSERT INTO runs (id, workflow_id, created_at)
             VALUES ('run-1', 'workflow-1', 'now');",
        )
        .expect("insert workflow and run");
        assert!(conn
            .execute(
                "INSERT INTO memory_reviews
                   (run_id, workflow_id, status, provider, created_at)
                 VALUES ('run-1', 'workflow-1', 'queued', 'claude_code', 'now')",
                [],
            )
            .is_err());

        let indexes: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT name FROM pragma_index_list('memory_candidates')")
                .expect("prepare candidate index list");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query candidate index list")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect candidate indexes")
        };
        assert!(indexes.contains(&"sqlite_autoindex_memory_candidates_1".to_owned()));
    }

    #[test]
    fn memory_curation_cascades_and_uniqueness_hold_on_fresh_schema() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize schema");
        apply_migrations(&conn).expect("apply migrations");
        conn.execute_batch(
            "INSERT INTO workflows
               (id, name, description, working_directory, sort_order, graph_json, created_at, updated_at)
             VALUES ('workflow-1', 'One', '', '/tmp/one', 0, '{}', 'now', 'now');
             INSERT INTO runs (id, workflow_id, status, created_at)
             VALUES ('run-1', 'workflow-1', 'completed', 'now');
             INSERT INTO memories
               (id, workflow_id, scope_type, scope_key, title, body, created_at, updated_at)
             VALUES ('target-1', 'workflow-1', 'workflow', 'workflow-1',
                     'Target', 'Body', 'now', 'now');
             INSERT INTO memory_reviews
               (run_id, workflow_id, status, provider, created_at)
             VALUES ('run-1', 'workflow-1', 'completed', 'claude_code', 'now');
             INSERT INTO memory_candidates
               (id, review_run_id, workflow_id, operation, target_memory_id,
                scope_type, scope_key, memory_type, title, body, confidence,
                rationale, content_hash, status, created_at)
             VALUES ('candidate-1', 'run-1', 'workflow-1', 'supersede', 'target-1',
                     'workflow', 'workflow-1', 'fact', 'Title', 'Body', 0.8,
                     'Rationale', 'hash-1', 'pending', 'now');",
        )
        .expect("insert curation fixtures");

        // One candidate hash per review run.
        assert!(conn
            .execute(
                "INSERT INTO memory_candidates
                   (id, review_run_id, workflow_id, operation, scope_type, scope_key,
                    memory_type, title, body, confidence, rationale, content_hash, status, created_at)
                 VALUES ('candidate-2', 'run-1', 'workflow-1', 'create', 'workflow',
                         'workflow-1', 'fact', 'Other', 'Other', 0.5, 'Why', 'hash-1', 'pending', 'now')",
                [],
            )
            .is_err());

        // Deleting the run cascades through reviews into candidates.
        conn.execute("DELETE FROM runs WHERE id = 'run-1'", [])
            .expect("delete reviewed run");
        let candidates: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_candidates", [], |row| {
                row.get(0)
            })
            .expect("count candidates");
        assert_eq!(candidates, 0);

        // Deleting the workflow removes per-workflow toggles and job rows.
        // The target memory goes first: its workflow_id is ON DELETE SET NULL,
        // which would violate the scoped-memory CHECK if nulled implicitly.
        conn.execute_batch(
            "DELETE FROM memories WHERE id = 'target-1';
             DELETE FROM workflows WHERE id = 'workflow-1';",
        )
        .expect("delete memory and workflow");
        let leftovers: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM workflow_memory_review)
                       + (SELECT COUNT(*) FROM memory_reviews)",
                [],
                |row| row.get(0),
            )
            .expect("count leftovers");
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn upgrades_legacy_database_with_curation_tables() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("initialize legacy fixture");
        conn.execute_batch(
            "DROP TABLE memory_review_settings;
             DROP TABLE workflow_memory_review;
             DROP TABLE memory_candidates;
             DROP TABLE memory_reviews;",
        )
        .expect("remove curation tables from fixture");
        apply_migrations(&conn).expect("upgrade legacy fixture");

        for table in [
            "memory_review_settings",
            "workflow_memory_review",
            "memory_reviews",
            "memory_candidates",
        ] {
            assert!(
                !columns(&conn, table).is_empty(),
                "missing migrated table {table}"
            );
        }
        let enabled: i64 = conn
            .query_row(
                "SELECT enabled FROM memory_review_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("load migrated singleton default");
        assert_eq!(enabled, 0, "review must stay off after upgrade");
    }
}
