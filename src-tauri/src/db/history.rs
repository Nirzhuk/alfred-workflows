#[cfg(test)]
use super::Db;
use super::DbError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub(crate) const SEARCHABLE_JSON_MAX_BYTES: usize = 32 * 1024;

/// Flatten JSON leaves into bounded searchable text without indexing object
/// keys. The output is always valid UTF-8 and excludes non-whitespace control
/// characters.
pub(crate) fn searchable_json_text(value: &Value, max_bytes: usize) -> String {
    fn append_leaf(output: &mut String, leaf: &str, max_bytes: usize) {
        if output.len() >= max_bytes {
            return;
        }
        if !output.is_empty() && output.len() < max_bytes {
            output.push(' ');
        }
        for character in leaf.chars() {
            if character.is_control() && character != '\n' && character != '\t' {
                continue;
            }
            if output.len() + character.len_utf8() > max_bytes {
                break;
            }
            output.push(character);
        }
    }

    fn walk(value: &Value, output: &mut String, max_bytes: usize) {
        if output.len() >= max_bytes {
            return;
        }
        match value {
            Value::Null => {}
            Value::Bool(value) => append_leaf(output, &value.to_string(), max_bytes),
            Value::Number(value) => append_leaf(output, &value.to_string(), max_bytes),
            Value::String(value) => append_leaf(output, value, max_bytes),
            Value::Array(values) => {
                for value in values {
                    walk(value, output, max_bytes);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    walk(value, output, max_bytes);
                }
            }
        }
    }

    if max_bytes == 0 {
        return String::new();
    }
    let mut output = String::new();
    walk(value, &mut output, max_bytes);
    output
}

pub(crate) fn index_run_step(
    conn: &Connection,
    step_id: &str,
    run_id: &str,
    workflow_id: &str,
    node_id: &str,
    input: &Value,
    output: &Value,
    error: Option<&str>,
) -> Result<(), DbError> {
    delete_run_step_index(conn, step_id)?;
    let input_text = searchable_json_text(input, SEARCHABLE_JSON_MAX_BYTES);
    let output_text = searchable_json_text(output, SEARCHABLE_JSON_MAX_BYTES);
    let error_text = error
        .map(|value| {
            searchable_json_text(&Value::String(value.to_owned()), SEARCHABLE_JSON_MAX_BYTES)
        })
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO run_step_fts
         (step_id, run_id, workflow_id, node_id, input_text, output_text, error_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            step_id,
            run_id,
            workflow_id,
            node_id,
            input_text,
            output_text,
            error_text
        ],
    )?;
    Ok(())
}

pub(crate) fn index_memory(conn: &Connection, memory_id: &str) -> Result<(), DbError> {
    let memory: Option<(String, String, String)> = conn
        .query_row(
            "SELECT workflow_id, title, body FROM memories WHERE id = ?1",
            params![memory_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    delete_memory_index(conn, memory_id)?;
    if let Some((workflow_id, title, body)) = memory {
        conn.execute(
            "INSERT INTO memory_fts(memory_id, workflow_id, title, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![memory_id, workflow_id, title, body],
        )?;
    }
    Ok(())
}

pub(crate) fn delete_run_step_index(conn: &Connection, step_id: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM run_step_fts WHERE step_id = ?1",
        params![step_id],
    )?;
    Ok(())
}

pub(crate) fn delete_memory_index(conn: &Connection, memory_id: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM memory_fts WHERE memory_id = ?1",
        params![memory_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateMemoryInput, CreateWorkflowInput, UpdateMemoryInput};
    use rusqlite::params;
    use serde_json::json;

    fn create_workflow(db: &Db, name: &str) -> String {
        db.create_workflow(CreateWorkflowInput {
            name: name.to_owned(),
            description: String::new(),
            working_directory: String::new(),
            folder_id: None,
            graph: json!({ "nodes": [], "edges": [] }),
        })
        .expect("create workflow")
        .id
    }

    #[test]
    fn searchable_json_is_bounded_and_omits_keys_and_controls() {
        let value = json!({
            "secretKeyName": ["first\u{0000}", 42, true, "ééé"]
        });
        let text = searchable_json_text(&value, 18);

        assert_eq!(text, "first 42 true éé");
        assert!(!text.contains("secretKeyName"));
        assert!(text.len() <= 18);
        assert!(text.is_char_boundary(text.len()));
    }

    #[test]
    fn memory_index_tracks_create_update_and_delete() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Memory workflow");
        let memory = db
            .create_memory(CreateMemoryInput {
                workflow_id,
                title: "Original title".into(),
                body: "original body".into(),
                run_id: None,
                node_id: None,
                kind: None,
                source: Some("manual".into()),
                pinned: None,
                id: Some("memory-sync".into()),
            })
            .expect("create memory");

        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"original\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .expect("query created index");

        db.update_memory(UpdateMemoryInput {
            id: memory.id.clone(),
            title: Some("Updated title".into()),
            body: Some("replacement body".into()),
            pinned: None,
            kind: None,
        })
        .expect("update memory");
        db.with_conn(|conn| {
            let old_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"original\"*'",
                [],
                |row| row.get(0),
            )?;
            let new_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"replacement\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!((old_count, new_count), (0, 1));
            Ok(())
        })
        .expect("query updated index");

        db.delete_memory(&memory.id).expect("delete memory");
        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_id = ?1",
                params![memory.id],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .expect("query deleted index");
    }

    #[test]
    fn run_step_index_tracks_insert_and_delete() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Run workflow");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('run-sync', ?1, 'manual', 'completed', '2026-08-18T10:00:00Z')",
                params![workflow_id],
            )?;
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, input_json, output_json, created_at)
                 VALUES ('step-sync', 'run-sync', 'agent-1', 'completed', '{}', '{}',
                         '2026-08-18T10:01:00Z')",
                [],
            )?;
            index_run_step(
                &transaction,
                "step-sync",
                "run-sync",
                &workflow_id,
                "agent-1",
                &json!({ "prompt": "find the launch decision" }),
                &json!({ "answer": "ship on Friday" }),
                None,
            )?;
            transaction.commit()?;

            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE run_step_fts MATCH '\"launch\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);

            let transaction = conn.unchecked_transaction()?;
            delete_run_step_index(&transaction, "step-sync")?;
            transaction.execute("DELETE FROM run_steps WHERE id = 'step-sync'", [])?;
            transaction.commit()?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE step_id = 'step-sync'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .expect("synchronize run-step index");
    }

    #[test]
    fn legacy_rows_backfill_once_without_duplicates() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Legacy workflow");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO memories
                 (id, workflow_id, kind, source, title, body, created_at, updated_at)
                 VALUES ('legacy-memory', ?1, 'text', 'import', 'Legacy note',
                         'remember unicode café', '2026-08-18T09:00:00Z', '2026-08-18T09:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('legacy-run', ?1, 'manual', 'completed', '2026-08-18T09:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, input_json, output_json, created_at)
                 VALUES ('legacy-step', 'legacy-run', 'node-1', 'completed',
                         '{not valid json', '{\"answer\":\"legacy output\"}',
                         '2026-08-18T09:01:00Z')",
                [],
            )?;
            conn.execute("DELETE FROM memory_fts", [])?;
            conn.execute("DELETE FROM run_step_fts", [])?;
            conn.execute(
                "DELETE FROM schema_meta WHERE key = 'search_fts_backfill_v1'",
                [],
            )?;

            crate::db::migrate::rebuild_search_indexes(conn)?;
            crate::db::migrate::rebuild_search_indexes(conn)?;

            let memory_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_id = 'legacy-memory'",
                [],
                |row| row.get(0),
            )?;
            let step_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE step_id = 'legacy-step'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!((memory_count, step_count), (1, 1));
            Ok(())
        })
        .expect("backfill legacy rows");
    }

    #[test]
    fn workflow_delete_cleans_both_indexes() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Disposable workflow");
        db.create_memory(CreateMemoryInput {
            workflow_id: workflow_id.clone(),
            title: "Delete me".into(),
            body: "memory cleanup".into(),
            run_id: None,
            node_id: None,
            kind: None,
            source: None,
            pinned: None,
            id: Some("delete-memory".into()),
        })
        .expect("create memory");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('delete-run', ?1, 'manual', 'completed', '2026-08-18T11:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO run_steps(id, run_id, node_id, status, created_at)
                 VALUES ('delete-step', 'delete-run', 'node-1', 'completed',
                         '2026-08-18T11:01:00Z')",
                [],
            )?;
            index_run_step(
                conn,
                "delete-step",
                "delete-run",
                &workflow_id,
                "node-1",
                &json!({ "input": "cleanup" }),
                &json!({ "output": "cleanup" }),
                None,
            )?;
            Ok(())
        })
        .expect("create run fixture");

        db.delete_workflow(&workflow_id).expect("delete workflow");
        db.with_conn(|conn| {
            let memories: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE workflow_id = ?1",
                params![workflow_id],
                |row| row.get(0),
            )?;
            let steps: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE workflow_id = ?1",
                params![workflow_id],
                |row| row.get(0),
            )?;
            assert_eq!((memories, steps), (0, 0));
            Ok(())
        })
        .expect("query cleaned indexes");
    }
}
