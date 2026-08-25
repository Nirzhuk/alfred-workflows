use super::{Db, DbError};
use crate::agent_accounts::models::{
    AgentAccount, AgentAccountStatus, AuthorizedAgentAccount,
};
use crate::agents::{AgentHarness, AgentProvider};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::str::FromStr;
use uuid::Uuid;

const COLUMNS: &str = "id, provider_id, harness, identity_key, display_name, \
external_account_id, external_workspace_id, auth_method, custody_mode, scopes_json, \
status, expires_at, last_checked_at, last_error_code, credential_ref, created_at, updated_at";

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn invalid_column(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message)),
    )
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentAccount> {
    let provider_id: String = row.get(1)?;
    let provider = AgentProvider::from_str(&provider_id)
        .ok_or_else(|| invalid_column(1, "unknown agent provider".into()))?;
    let harness_value: String = row.get(2)?;
    let harness = match harness_value.as_str() {
        "alfred" => AgentHarness::Alfred,
        "cli" => AgentHarness::Cli,
        _ => return Err(invalid_column(2, "unknown agent harness".into())),
    };
    let auth_method_value: String = row.get(7)?;
    let auth_method = FromStr::from_str(&auth_method_value).map_err(|error| invalid_column(7, error))?;
    let custody_value: String = row.get(8)?;
    let custody_mode = FromStr::from_str(&custody_value).map_err(|error| invalid_column(8, error))?;
    let scopes_json: String = row.get(9)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let status_value: String = row.get(10)?;
    let status = FromStr::from_str(&status_value).map_err(|error| invalid_column(10, error))?;

    Ok(AgentAccount {
        id: row.get(0)?,
        provider,
        harness,
        identity_key: row.get(3)?,
        display_name: row.get(4)?,
        external_account_id: row.get(5)?,
        external_workspace_id: row.get(6)?,
        auth_method,
        custody_mode,
        scopes,
        status,
        expires_at: row.get(11)?,
        last_checked_at: row.get(12)?,
        last_error_code: row.get(13)?,
        credential_ref: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

impl Db {
    pub fn list_agent_accounts(&self) -> Result<Vec<AgentAccount>, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM agent_accounts ORDER BY provider_id, created_at, id"
            ))?;
            let accounts = statement
                .query_map([], map_account)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(accounts)
        })
    }

    pub fn get_agent_account(&self, id: &str) -> Result<Option<AgentAccount>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM agent_accounts WHERE id = ?1"),
                    params![id],
                    map_account,
                )
                .optional()?)
        })
    }

    /// Creates or reconnects metadata in a non-connected state. The service
    /// must persist the credential before promoting this row to connected.
    pub fn prepare_agent_account(
        &self,
        mut input: AuthorizedAgentAccount,
    ) -> Result<AgentAccount, DbError> {
        if input.harness != AgentHarness::Alfred {
            return Err(DbError::Other(
                "native accounts require the alfred harness".into(),
            ));
        }
        if input.external_account_id.trim().is_empty() {
            return Err(DbError::Other("validated account identity is required".into()));
        }
        input.scopes.sort();
        input.scopes.dedup();
        let identity_key = input.identity_key();
        let scopes_json = serde_json::to_string(&input.scopes)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();

        let id = self.with_conn(|conn| {
            let existing: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, credential_ref FROM agent_accounts
                     WHERE provider_id = ?1 AND harness = ?2 AND identity_key = ?3",
                    params![input.provider.as_str(), input.harness.as_str(), identity_key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            if let Some((id, _credential_ref)) = existing {
                conn.execute(
                    "UPDATE agent_accounts SET
                       display_name = ?1, external_account_id = ?2,
                       external_workspace_id = ?3, auth_method = ?4,
                       custody_mode = ?5, scopes_json = ?6, status = 'error',
                       expires_at = ?7, last_error_code = 'credential_pending',
                       updated_at = ?8
                     WHERE id = ?9",
                    params![
                        input.display_name,
                        input.external_account_id,
                        input.external_workspace_id,
                        input.auth_method.as_str(),
                        input.custody_mode.as_str(),
                        scopes_json,
                        input.expires_at,
                        updated_at,
                        id,
                    ],
                )?;
                return Ok(id);
            }

            let id = format!("account_{}", Uuid::new_v4().simple());
            let credential_ref = format!("agent-account:{id}");
            conn.execute(
                "INSERT INTO agent_accounts (
                   id, provider_id, harness, identity_key, display_name,
                   external_account_id, external_workspace_id, auth_method,
                   custody_mode, scopes_json, status, expires_at,
                   last_error_code, credential_ref, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                           'error', ?11, 'credential_pending', ?12, ?13, ?13)",
                params![
                    id,
                    input.provider.as_str(),
                    input.harness.as_str(),
                    identity_key,
                    input.display_name,
                    input.external_account_id,
                    input.external_workspace_id,
                    input.auth_method.as_str(),
                    input.custody_mode.as_str(),
                    scopes_json,
                    input.expires_at,
                    credential_ref,
                    updated_at,
                ],
            )?;
            Ok(id)
        })?;

        self.get_agent_account(&id)?
            .ok_or_else(|| DbError::Other("saved agent account could not be loaded".into()))
    }

    pub fn set_agent_account_state(
        &self,
        id: &str,
        status: AgentAccountStatus,
        expires_at: Option<&str>,
        last_error_code: Option<&str>,
    ) -> Result<(), DbError> {
        let checked_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE agent_accounts
                 SET status = ?1, expires_at = ?2, last_checked_at = ?3,
                     last_error_code = ?4, updated_at = ?3
                 WHERE id = ?5",
                params![status.as_str(), expires_at, checked_at, last_error_code, id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("agent account not found".into()));
        }
        Ok(())
    }

    pub fn delete_agent_account_metadata(&self, id: &str) -> Result<(), DbError> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM agent_accounts WHERE id = ?1", params![id])?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("agent account not found".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::{AgentAuthMethod, CredentialCustodyMode};

    fn fixture(account: &str, workspace: &str) -> AuthorizedAgentAccount {
        AuthorizedAgentAccount {
            provider: AgentProvider::Codex,
            harness: AgentHarness::Alfred,
            display_name: Some(account.into()),
            external_account_id: account.into(),
            external_workspace_id: Some(workspace.into()),
            auth_method: AgentAuthMethod::OAuthPkce,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: vec!["models:read".into(), "models:read".into()],
            expires_at: None,
        }
    }

    #[test]
    fn distinct_identities_are_deterministic_and_reconnect_reuses_account() {
        let db = Db::open_in_memory().expect("database");
        let first = db.prepare_agent_account(fixture("one", "org")).expect("first");
        let reconnect = db
            .prepare_agent_account(fixture("one", "org"))
            .expect("reconnect");
        let second = db.prepare_agent_account(fixture("two", "org")).expect("second");

        assert_eq!(first.id, reconnect.id);
        assert_eq!(first.credential_ref, reconnect.credential_ref);
        assert_ne!(first.id, second.id);
        assert_eq!(db.list_agent_accounts().expect("list").len(), 2);
        assert_eq!(reconnect.scopes, vec!["models:read"]);
        assert_eq!(reconnect.status, AgentAccountStatus::Error);
        assert_eq!(reconnect.last_error_code.as_deref(), Some("credential_pending"));
    }
}
