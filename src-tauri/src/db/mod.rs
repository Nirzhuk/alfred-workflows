mod app_connections;
mod agent_accounts;
mod app_events;
mod history;
mod license;
mod memories;
mod memory_curation;
mod memory_retrieval;
mod migrate;
mod schedules;
mod triggers;
mod workflow_folders;
mod workflows;

pub use app_events::*;
pub use history::*;
pub use license::*;
pub use memories::*;
pub use memory_curation::*;
pub use memory_retrieval::*;
pub use schedules::*;
pub use triggers::*;
pub use workflow_folders::*;
pub use workflows::*;

use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not resolve app data directory")]
    NoAppDir,
    #[error("{0}")]
    Other(String),
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open() -> Result<Self, DbError> {
        let path = db_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(include_str!("schema.sql"))?;
        migrate::apply_migrations(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| DbError::Other("database lock poisoned".into()))?;
        f(&conn)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(include_str!("schema.sql"))?;
        migrate::apply_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

pub fn app_data_dir() -> Result<PathBuf, DbError> {
    let dirs = ProjectDirs::from("com", "nirzhuk", "alfred").ok_or(DbError::NoAppDir)?;
    Ok(dirs.data_dir().to_path_buf())
}

fn db_path() -> Result<PathBuf, DbError> {
    Ok(app_data_dir()?.join("app.db"))
}
