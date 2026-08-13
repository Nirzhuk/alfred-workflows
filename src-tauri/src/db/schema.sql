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
  graph_json TEXT NOT NULL DEFAULT '{"nodes":[],"edges":[]}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY NOT NULL,
  provider TEXT NOT NULL CHECK (provider IN ('claude_code', 'cursor', 'codex', 'opencode')),
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

-- Cross-workflow memory links: consumer workflow → memory owned elsewhere.
CREATE TABLE IF NOT EXISTS memory_links (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  UNIQUE (workflow_id, memory_id)
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
  status TEXT NOT NULL DEFAULT 'connected' CHECK (status IN ('connected', 'expired', 'error', 'revoked')),
  expires_at TEXT,
  last_checked_at TEXT,
  last_error_code TEXT,
  credential_ref TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_app_connections_identity
  ON app_connections(provider_id, connection_mode, identity_key);
CREATE INDEX IF NOT EXISTS idx_app_connections_provider_id ON app_connections(provider_id);
