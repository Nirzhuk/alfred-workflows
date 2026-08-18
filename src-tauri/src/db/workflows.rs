use super::{Db, DbError};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub working_directory: String,
    pub folder_id: Option<String>,
    pub graph: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default = "empty_graph")]
    pub graph: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowInput {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub working_directory: Option<String>,
    pub graph: Option<Value>,
}

fn empty_graph() -> Value {
    serde_json::json!({ "nodes": [], "edges": [] })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn row_to_workflow(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workflow> {
    let graph_json: String = row.get(5)?;
    Ok(Workflow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        working_directory: row.get(3)?,
        folder_id: row.get(4)?,
        graph: serde_json::from_str(&graph_json).unwrap_or_else(|_| empty_graph()),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

impl Db {
    pub fn list_workflows(&self) -> Result<Vec<Workflow>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, working_directory, folder_id, graph_json, created_at, updated_at
                 FROM workflows
                 ORDER BY sort_order ASC, updated_at DESC",
            )?;

            let rows = stmt
                .query_map([], row_to_workflow)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rows)
        })
    }

    pub fn get_workflow(&self, id: &str) -> Result<Option<Workflow>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, working_directory, folder_id, graph_json, created_at, updated_at
                 FROM workflows
                 WHERE id = ?1",
            )?;

            let mut rows = stmt.query_map(params![id], row_to_workflow)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn create_workflow(&self, input: CreateWorkflowInput) -> Result<Workflow, DbError> {
        let id = Uuid::new_v4().to_string();
        let created_at = now();
        let graph_json =
            serde_json::to_string(&input.graph).map_err(|e| DbError::Other(e.to_string()))?;

        self.with_conn(|conn| {
            if let Some(folder_id) = input.folder_id.as_deref() {
                let exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM workflow_folders WHERE id = ?1",
                    params![folder_id],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    return Err(DbError::Other(format!("folder not found: {folder_id}")));
                }
            }
            let next_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MIN(sort_order), 0) - 1 FROM workflows",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(-1);
            conn.execute(
                "INSERT INTO workflows (id, name, description, working_directory, folder_id, sort_order, graph_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    id,
                    input.name,
                    input.description,
                    input.working_directory,
                    input.folder_id,
                    next_order,
                    graph_json,
                    created_at
                ],
            )?;
            Ok(())
        })?;

        self.get_workflow(&id)?
            .ok_or_else(|| DbError::Other("failed to load created workflow".into()))
    }

    /// Persist sidebar order. `ordered_ids` is top → bottom.
    pub fn reorder_workflows(&self, ordered_ids: &[String]) -> Result<(), DbError> {
        if ordered_ids.is_empty() {
            return Ok(());
        }

        self.with_conn(|conn| {
            // Held under the Db mutex — treat as one logical write.
            for (index, id) in ordered_ids.iter().enumerate() {
                conn.execute(
                    "UPDATE workflows SET sort_order = ?1 WHERE id = ?2",
                    params![index as i64, id],
                )?;
            }
            Ok(())
        })
    }

    pub fn update_workflow(&self, input: UpdateWorkflowInput) -> Result<Workflow, DbError> {
        let existing = self
            .get_workflow(&input.id)?
            .ok_or_else(|| DbError::Other(format!("workflow not found: {}", input.id)))?;

        let name = input.name.unwrap_or(existing.name);
        let description = input.description.unwrap_or(existing.description);
        let working_directory = input
            .working_directory
            .unwrap_or(existing.working_directory);
        let graph = input.graph.unwrap_or(existing.graph);
        let graph_json =
            serde_json::to_string(&graph).map_err(|e| DbError::Other(e.to_string()))?;
        let updated_at = now();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE workflows
                 SET name = ?1, description = ?2, working_directory = ?3, graph_json = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![
                    name,
                    description,
                    working_directory,
                    graph_json,
                    updated_at,
                    input.id
                ],
            )?;
            Ok(())
        })?;

        self.get_workflow(&input.id)?
            .ok_or_else(|| DbError::Other("failed to load updated workflow".into()))
    }

    pub fn delete_workflow(&self, id: &str) -> Result<(), DbError> {
        let changed = self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            // Explicit cleanup so delete still works if CASCADE isn't active
            // on an older database connection.
            transaction.execute("DELETE FROM memory_fts WHERE workflow_id = ?1", params![id])?;
            transaction.execute("DELETE FROM run_step_fts WHERE workflow_id = ?1", params![id])?;
            transaction.execute("DELETE FROM memories WHERE workflow_id = ?1", params![id])?;
            transaction.execute("DELETE FROM schedules WHERE workflow_id = ?1", params![id])?;
            transaction.execute(
                "DELETE FROM run_steps WHERE run_id IN (SELECT id FROM runs WHERE workflow_id = ?1)",
                params![id],
            )?;
            transaction.execute("DELETE FROM runs WHERE workflow_id = ?1", params![id])?;
            let changed = transaction.execute("DELETE FROM workflows WHERE id = ?1", params![id])?;
            transaction.commit()?;
            Ok(changed)
        })?;

        if changed == 0 {
            return Err(DbError::Other(format!("workflow not found: {id}")));
        }

        let artifacts = super::app_data_dir()
            .map(|d| d.join("artifacts").join(id))
            .ok();
        if let Some(dir) = artifacts {
            let _ = std::fs::remove_dir_all(dir);
        }

        Ok(())
    }
}
