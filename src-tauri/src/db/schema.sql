PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS workflow_folders (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  working_directory TEXT NOT NULL DEFAULT '',
  folder_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  memory_retrieval_enabled INTEGER NOT NULL DEFAULT 0,
  graph_json TEXT NOT NULL DEFAULT '{"nodes":[],"edges":[]}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY NOT NULL,
  provider TEXT NOT NULL CHECK (provider IN ('claude_code', 'cursor', 'codex', 'opencode', 'github_copilot', 'gemini', 'grok', 'pi', 'omp')),
  name TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- ponytail: no CHECK on trigger_kind — every new trigger source would need a
-- table rebuild (SQLite can't ALTER a CHECK). Values come from RunTrigger.
CREATE TABLE IF NOT EXISTS runs (
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

CREATE TABLE IF NOT EXISTS run_steps (
  id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  agent_provider TEXT,
  skill_name TEXT,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed', 'skipped')),
  input_json TEXT NOT NULL DEFAULT '{}',
  output_json TEXT NOT NULL DEFAULT '{}',
  error TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schedules (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  cron TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  next_run_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Event triggers: one workflow can have many (unlike schedules, which are 1:1).
-- `source` is free-form so new sources need no migration; `config_json` holds
-- the source-specific shape (file: {path, pattern, debounceMs}).
CREATE TABLE IF NOT EXISTS triggers (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  config_json TEXT NOT NULL DEFAULT '{}',
  secret TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_fired_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memories (
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
  supersedes_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  last_confirmed_at TEXT,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (scope_type = 'workflow' AND workflow_id IS NOT NULL AND scope_key = workflow_id)
    OR (scope_type = 'workspace' AND length(trim(scope_key)) > 0)
    OR (scope_type = 'user' AND scope_key = 'local-user')
  )
);

-- Cross-workflow memory links: consumer workflow → memory owned elsewhere.
CREATE TABLE IF NOT EXISTS memory_links (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  UNIQUE (workflow_id, memory_id)
);

CREATE TABLE IF NOT EXISTS run_memory_uses (
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

CREATE TABLE IF NOT EXISTS schema_meta (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);

-- Disposable local search indexes. Canonical text remains in memories and
-- run_steps; these tables can be cleared and rebuilt at any time.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
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
);

-- Connected-app metadata only. OAuth credentials live in the OS credential
-- store and are addressed by the opaque `credential_ref`.
CREATE TABLE IF NOT EXISTS app_connections (
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

-- Provider-neutral app event delivery. Normalized payloads are bounded and
-- minimized before entering this queue; provider bodies and headers never do.
CREATE TABLE IF NOT EXISTS app_trigger_state (
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

-- Post-run memory review: explicit opt-in settings (singleton), one review job
-- per run, and model-proposed memory candidates that never touch canonical
-- memories until a user approves them.
CREATE TABLE IF NOT EXISTS memory_review_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  enabled INTEGER NOT NULL DEFAULT 0,
  provider TEXT,
  model TEXT,
  max_candidates INTEGER NOT NULL DEFAULT 5 CHECK (max_candidates BETWEEN 1 AND 5),
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO memory_review_settings (id, enabled, provider, model, max_candidates, updated_at)
VALUES (1, 0, NULL, NULL, 5, '1970-01-01T00:00:00Z');

CREATE TABLE IF NOT EXISTS workflow_memory_review (
  workflow_id TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

-- Review job metadata only. Raw provider errors, prompts, responses, and run
-- transcripts never enter this table; failures carry stable codes alone.
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
CREATE INDEX IF NOT EXISTS idx_memory_candidates_created ON memory_candidates(created_at);
CREATE INDEX IF NOT EXISTS idx_memory_reviews_status ON memory_reviews(status);
CREATE INDEX IF NOT EXISTS idx_memory_reviews_created ON memory_reviews(created_at);

-- Safe licensing snapshot only. The full key and Polar activation ID live
-- together in the OS credential store under `credential_ref`.
CREATE TABLE IF NOT EXISTS license_snapshot (
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
);

CREATE INDEX IF NOT EXISTS idx_runs_workflow_id ON runs(workflow_id);
CREATE INDEX IF NOT EXISTS idx_run_steps_run_id ON run_steps(run_id);
CREATE INDEX IF NOT EXISTS idx_schedules_workflow_id ON schedules(workflow_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_workflow_unique ON schedules(workflow_id);
CREATE INDEX IF NOT EXISTS idx_triggers_workflow_id ON triggers(workflow_id);
CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled);
CREATE INDEX IF NOT EXISTS idx_memories_workflow_id ON memories(workflow_id);
CREATE INDEX IF NOT EXISTS idx_memories_workflow_pinned ON memories(workflow_id, pinned);
CREATE INDEX IF NOT EXISTS idx_memory_links_workflow_id ON memory_links(workflow_id);
CREATE INDEX IF NOT EXISTS idx_memory_links_memory_id ON memory_links(memory_id);
CREATE INDEX IF NOT EXISTS idx_run_memory_uses_run_node_rank
  ON run_memory_uses(run_id, node_id, rank);
CREATE INDEX IF NOT EXISTS idx_run_memory_uses_memory_id ON run_memory_uses(memory_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_app_connections_identity
  ON app_connections(provider_id, connection_mode, identity_key);
CREATE INDEX IF NOT EXISTS idx_app_connections_provider_id ON app_connections(provider_id);
CREATE INDEX IF NOT EXISTS idx_app_event_queue_trigger ON app_event_queue(trigger_id, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_app_event_receipts_received ON app_event_receipts(received_at);
