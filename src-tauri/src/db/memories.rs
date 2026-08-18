use super::history::{delete_memory_index, index_memory};
use super::{app_data_dir, Db, DbError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Spill large bodies to disk so SQLite stays lean.
const ARTIFACT_BODY_THRESHOLD: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub workflow_id: String,
    pub run_id: Option<String>,
    pub node_id: Option<String>,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub body: String,
    pub artifact_path: Option<String>,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryInput {
    pub workflow_id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemoryInput {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Memory as shown in a workflow's library — owned or linked from elsewhere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryWithOrigin {
    #[serde(flatten)]
    pub memory: Memory,
    /// `"owned"` or `"linked"`.
    pub origin: String,
    /// Present when `origin == "linked"` — the workflow that owns the memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_workflow_name: Option<String>,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn normalize_kind(kind: Option<&str>, body_len: usize, has_artifact: bool) -> String {
    if let Some(k) = kind {
        if matches!(k, "text" | "note" | "artifact") {
            return k.to_string();
        }
    }
    if has_artifact || body_len >= ARTIFACT_BODY_THRESHOLD {
        "artifact".into()
    } else {
        "text".into()
    }
}

fn normalize_source(source: Option<&str>) -> String {
    match source {
        Some("manual") => "manual".into(),
        Some("import") => "import".into(),
        _ => "run".into(),
    }
}

fn artifacts_dir(workflow_id: &str) -> Result<PathBuf, DbError> {
    let dir = app_data_dir()?.join("artifacts").join(workflow_id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_artifact(workflow_id: &str, memory_id: &str, body: &str) -> Result<String, DbError> {
    let path = artifacts_dir(workflow_id)?.join(format!("{memory_id}.txt"));
    fs::write(&path, body)?;
    Ok(path.to_string_lossy().into_owned())
}

fn remove_artifact(path: Option<&str>) {
    if let Some(p) = path {
        let _ = fs::remove_file(p);
    }
}

fn preview_body(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_string();
    }
    let mut end = max.min(body.len());
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n… [full content stored as artifact]", &body[..end])
}

fn map_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    let pinned: i64 = row.get(9)?;
    Ok(Memory {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        run_id: row.get(2)?,
        node_id: row.get(3)?,
        kind: row.get(4)?,
        source: row.get(5)?,
        title: row.get(6)?,
        body: row.get(7)?,
        artifact_path: row.get(8)?,
        pinned: pinned != 0,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

const SELECT_COLS: &str = "id, workflow_id, run_id, node_id, kind, source, title, body,
     artifact_path, pinned, created_at, updated_at";

impl Db {
    pub fn list_memories(&self, workflow_id: &str) -> Result<Vec<Memory>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SELECT_COLS}
                 FROM memories
                 WHERE workflow_id = ?1
                 ORDER BY pinned DESC, updated_at DESC, created_at DESC"
            ))?;
            let rows = stmt
                .query_map(params![workflow_id], map_memory)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn list_pinned_memories(&self, workflow_id: &str) -> Result<Vec<Memory>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SELECT_COLS}
                 FROM memories
                 WHERE workflow_id = ?1 AND pinned = 1
                 ORDER BY updated_at DESC"
            ))?;
            let rows = stmt
                .query_map(params![workflow_id], map_memory)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<Memory>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SELECT_COLS}
                 FROM memories
                 WHERE id = ?1"
            ))?;
            let row = stmt.query_row(params![id], map_memory).optional()?;
            Ok(row)
        })
    }

    pub fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory, DbError> {
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let title = input.title.trim().to_string();
        if title.is_empty() {
            return Err(DbError::Other("memory title cannot be empty".into()));
        }
        let body = input.body;
        let created_at = now();
        let pinned = input.pinned.unwrap_or(false);
        let source = normalize_source(input.source.as_deref());

        let spill =
            body.len() >= ARTIFACT_BODY_THRESHOLD || input.kind.as_deref() == Some("artifact");
        let artifact_path = if spill {
            Some(write_artifact(&input.workflow_id, &id, &body)?)
        } else {
            None
        };
        // Keep a preview in DB for list UI even when spilled.
        let stored_body = if artifact_path.is_some() && body.len() > 2_000 {
            preview_body(&body, 2_000)
        } else {
            body.clone()
        };
        let kind = normalize_kind(input.kind.as_deref(), body.len(), artifact_path.is_some());
        // Prefer note when source is manual and kind wasn't forced.
        let kind = if source == "manual" && input.kind.is_none() && kind == "text" {
            "note".into()
        } else {
            kind
        };

        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO memories
                 (id, workflow_id, run_id, node_id, kind, source, title, body,
                  artifact_path, pinned, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    id,
                    input.workflow_id,
                    input.run_id,
                    input.node_id,
                    kind,
                    source,
                    title,
                    stored_body,
                    artifact_path,
                    if pinned { 1 } else { 0 },
                    created_at,
                ],
            )?;
            index_memory(&transaction, &id)?;
            transaction.commit()?;
            Ok(())
        })?;

        self.get_memory(&id)?
            .ok_or_else(|| DbError::Other("failed to load created memory".into()))
    }

    pub fn update_memory(&self, input: UpdateMemoryInput) -> Result<Memory, DbError> {
        let existing = self
            .get_memory(&input.id)?
            .ok_or_else(|| DbError::Other(format!("memory not found: {}", input.id)))?;

        let title = input
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or(existing.title);
        let pinned = input.pinned.unwrap_or(existing.pinned);
        let existing_kind = existing.kind.clone();
        let mut kind = input.kind.unwrap_or_else(|| existing_kind.clone());
        if !matches!(kind.as_str(), "text" | "note" | "artifact") {
            kind = existing_kind;
        }

        let (body, artifact_path) = if let Some(new_body) = input.body {
            remove_artifact(existing.artifact_path.as_deref());
            let spill = new_body.len() >= ARTIFACT_BODY_THRESHOLD || kind == "artifact";
            let path = if spill {
                Some(write_artifact(
                    &existing.workflow_id,
                    &existing.id,
                    &new_body,
                )?)
            } else {
                None
            };
            if path.is_some() {
                kind = "artifact".into();
            }
            let stored = if path.is_some() && new_body.len() > 2_000 {
                preview_body(&new_body, 2_000)
            } else {
                new_body
            };
            (stored, path)
        } else {
            (existing.body, existing.artifact_path)
        };

        let updated_at = now();
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "UPDATE memories
                 SET title = ?1, body = ?2, artifact_path = ?3, pinned = ?4,
                     kind = ?5, updated_at = ?6
                 WHERE id = ?7",
                params![
                    title,
                    body,
                    artifact_path,
                    if pinned { 1 } else { 0 },
                    kind,
                    updated_at,
                    input.id,
                ],
            )?;
            index_memory(&transaction, &input.id)?;
            transaction.commit()?;
            Ok(())
        })?;

        self.get_memory(&input.id)?
            .ok_or_else(|| DbError::Other("failed to load updated memory".into()))
    }

    pub fn delete_memory(&self, id: &str) -> Result<(), DbError> {
        let existing = self.get_memory(id)?;
        let changed = self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            delete_memory_index(&transaction, id)?;
            let changed = transaction.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
            transaction.commit()?;
            Ok(changed)
        })?;
        if changed == 0 {
            return Err(DbError::Other(format!("memory not found: {id}")));
        }
        if let Some(m) = existing {
            remove_artifact(m.artifact_path.as_deref());
        }
        Ok(())
    }

    pub fn clear_memories(&self, workflow_id: &str) -> Result<usize, DbError> {
        let paths: Vec<Option<String>> = self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT artifact_path FROM memories WHERE workflow_id = ?1")?;
            let rows = stmt
                .query_map(params![workflow_id], |row| row.get::<_, Option<String>>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let changed = self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "DELETE FROM memory_fts WHERE workflow_id = ?1",
                params![workflow_id],
            )?;
            let changed = transaction.execute(
                "DELETE FROM memories WHERE workflow_id = ?1",
                params![workflow_id],
            )?;
            transaction.commit()?;
            Ok(changed)
        })?;

        for path in paths {
            remove_artifact(path.as_deref());
        }
        let dir = app_data_dir()?.join("artifacts").join(workflow_id);
        let _ = fs::remove_dir_all(dir);

        Ok(changed)
    }

    /// Full text for prompt injection — prefers artifact file when present.
    pub fn memory_full_body(&self, memory: &Memory) -> String {
        if let Some(path) = &memory.artifact_path {
            if let Ok(contents) = fs::read_to_string(path) {
                return contents;
            }
        }
        memory.body.clone()
    }

    pub fn format_pinned_context(&self, workflow_id: &str) -> Result<String, DbError> {
        let pinned = self.list_pinned_memories(workflow_id)?;
        if pinned.is_empty() {
            return Ok(String::new());
        }

        let mut parts = Vec::with_capacity(pinned.len() + 1);
        parts.push(
            "## Pinned workflow memories\nUse these as durable context for this run.\n".to_string(),
        );
        for (i, memory) in pinned.iter().enumerate() {
            let body = self.memory_full_body(memory);
            parts.push(format!(
                "### Memory {} — {}\n{}\n",
                i + 1,
                memory.title,
                body.trim()
            ));
        }
        Ok(parts.join("\n"))
    }

    /// Owned memories plus memories linked in from other workflows.
    pub fn list_memories_with_links(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<MemoryWithOrigin>, DbError> {
        let mut owned = self
            .list_memories(workflow_id)?
            .into_iter()
            .map(|memory| MemoryWithOrigin {
                memory,
                origin: "owned".into(),
                source_workflow_name: None,
            })
            .collect::<Vec<_>>();

        let linked = self.list_linked_memories(workflow_id)?;
        owned.extend(linked);
        Ok(owned)
    }

    pub fn list_linked_memories(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<MemoryWithOrigin>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT m.id, m.workflow_id, m.run_id, m.node_id, m.kind, m.source,
                        m.title, m.body, m.artifact_path, m.pinned, m.created_at, m.updated_at,
                        w.name
                 FROM memory_links l
                 JOIN memories m ON m.id = l.memory_id
                 JOIN workflows w ON w.id = m.workflow_id
                 WHERE l.workflow_id = ?1
                 ORDER BY l.created_at DESC"
            ))?;
            let rows = stmt
                .query_map(params![workflow_id], |row| {
                    let memory = map_memory(row)?;
                    let source_workflow_name: String = row.get(12)?;
                    Ok(MemoryWithOrigin {
                        memory,
                        origin: "linked".into(),
                        source_workflow_name: Some(source_workflow_name),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Memories from other workflows that can be linked into `workflow_id`.
    pub fn list_linkable_memories(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<MemoryWithOrigin>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT m.id, m.workflow_id, m.run_id, m.node_id, m.kind, m.source,
                        m.title, m.body, m.artifact_path, m.pinned, m.created_at, m.updated_at,
                        w.name
                 FROM memories m
                 JOIN workflows w ON w.id = m.workflow_id
                 WHERE m.workflow_id != ?1
                   AND m.id NOT IN (
                     SELECT memory_id FROM memory_links WHERE workflow_id = ?1
                   )
                 ORDER BY w.name ASC, m.updated_at DESC"
            ))?;
            let rows = stmt
                .query_map(params![workflow_id], |row| {
                    let memory = map_memory(row)?;
                    let source_workflow_name: String = row.get(12)?;
                    Ok(MemoryWithOrigin {
                        memory,
                        origin: "linkable".into(),
                        source_workflow_name: Some(source_workflow_name),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn link_memory(
        &self,
        workflow_id: &str,
        memory_id: &str,
    ) -> Result<MemoryWithOrigin, DbError> {
        let memory = self
            .get_memory(memory_id)?
            .ok_or_else(|| DbError::Other(format!("memory not found: {memory_id}")))?;

        if memory.workflow_id == workflow_id {
            return Err(DbError::Other(
                "memory already belongs to this workflow".into(),
            ));
        }

        self.get_workflow(workflow_id)?
            .ok_or_else(|| DbError::Other(format!("workflow not found: {workflow_id}")))?;

        let source_name = self
            .get_workflow(&memory.workflow_id)?
            .map(|w| w.name)
            .unwrap_or_else(|| "Unknown workflow".into());

        let id = Uuid::new_v4().to_string();
        let created_at = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO memory_links (id, workflow_id, memory_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, workflow_id, memory_id, created_at],
            )?;
            Ok(())
        })?;

        Ok(MemoryWithOrigin {
            memory,
            origin: "linked".into(),
            source_workflow_name: Some(source_name),
        })
    }

    pub fn unlink_memory(&self, workflow_id: &str, memory_id: &str) -> Result<(), DbError> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM memory_links WHERE workflow_id = ?1 AND memory_id = ?2",
                params![workflow_id, memory_id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("memory link not found".into()));
        }
        Ok(())
    }

    /// Build markdown for selected memory ids (used by the Memories node).
    pub fn format_memories_context(&self, memory_ids: &[String]) -> Result<String, DbError> {
        if memory_ids.is_empty() {
            return Ok(String::new());
        }

        let mut parts = Vec::new();
        parts.push("## Linked memories\nUse these memories as context for this run.\n".to_string());

        let mut index = 0usize;
        for memory_id in memory_ids {
            let Some(memory) = self.get_memory(memory_id)? else {
                continue;
            };
            let source_name = self
                .get_workflow(&memory.workflow_id)?
                .map(|w| w.name)
                .unwrap_or_else(|| "another workflow".into());
            let body = self.memory_full_body(&memory);
            index += 1;
            parts.push(format!(
                "### Memory {index} — {} (from {})\n{}\n",
                memory.title,
                source_name,
                body.trim()
            ));
        }

        if index == 0 {
            return Ok(String::new());
        }
        Ok(parts.join("\n"))
    }
}
