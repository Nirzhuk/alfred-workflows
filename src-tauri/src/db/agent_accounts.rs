use super::{Db, DbError};
use crate::agent_accounts::models::{
    validate_agent_account, validate_authorized_agent_account, AgentAccount, AgentAccountStatus,
    AgentAuthMethod, AgentProductId, AuthorizedAgentAccount, CredentialCustodyMode,
    ManagedRuntimeId,
};
use crate::agents::{AgentHarness, AgentProvider};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::str::FromStr;
use uuid::Uuid;

const COLUMNS: &str = "id, provider_id, product_id, harness, identity_key, display_name, \
external_account_id, external_workspace_id, auth_method, custody_mode, managed_runtime_id, \
managed_runtime_version, runtime_profile_ref, scopes_json, billing_source, billing_owner, \
entitlement_state, entitlement_source, entitlement_observed_at, status, expires_at, \
last_checked_at, last_error_code, credential_ref, created_at, updated_at";

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn invalid_column(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccountReadDiagnostic {
    pub account_id: Option<String>,
    pub error_code: &'static str,
}

#[derive(Debug)]
pub struct AgentAccountList {
    pub accounts: Vec<AgentAccount>,
    pub diagnostics: Vec<AgentAccountReadDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccountCredentialCleanup {
    pub credential_ref: String,
    pub cleanup_owner: String,
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentAccount> {
    let provider_id: String = row.get(1)?;
    let provider = AgentProvider::from_str(&provider_id)
        .ok_or_else(|| invalid_column(1, "unknown agent provider".into()))?;
    let product_value: String = row.get(2)?;
    let product = FromStr::from_str(&product_value).map_err(|error| invalid_column(2, error))?;
    let harness_value: String = row.get(3)?;
    let harness = match harness_value.as_str() {
        "alfred" => AgentHarness::Alfred,
        "cli" => AgentHarness::Cli,
        _ => return Err(invalid_column(3, "unknown agent harness".into())),
    };
    let auth_method_value: String = row.get(8)?;
    let auth_method =
        FromStr::from_str(&auth_method_value).map_err(|error| invalid_column(8, error))?;
    let custody_value: String = row.get(9)?;
    let custody_mode =
        FromStr::from_str(&custody_value).map_err(|error| invalid_column(9, error))?;
    let managed_runtime_value: Option<String> = row.get(10)?;
    let managed_runtime_id = managed_runtime_value
        .map(|value| FromStr::from_str(&value).map_err(|error| invalid_column(10, error)))
        .transpose()?;
    let scopes_json: String = row.get(13)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let entitlement_value: String = row.get(16)?;
    let entitlement_state =
        FromStr::from_str(&entitlement_value).map_err(|error| invalid_column(16, error))?;
    let status_value: String = row.get(19)?;
    let status = FromStr::from_str(&status_value).map_err(|error| invalid_column(19, error))?;

    let account = AgentAccount {
        id: row.get(0)?,
        provider,
        product,
        harness,
        identity_key: row.get(4)?,
        display_name: row.get(5)?,
        external_account_id: row.get(6)?,
        external_workspace_id: row.get(7)?,
        auth_method,
        custody_mode,
        managed_runtime_id,
        managed_runtime_version: row.get(11)?,
        runtime_profile_ref: row.get(12)?,
        scopes,
        billing_source: row.get(14)?,
        billing_owner: row.get(15)?,
        entitlement_state,
        entitlement_source: row.get(17)?,
        entitlement_observed_at: row.get(18)?,
        status,
        expires_at: row.get(20)?,
        last_checked_at: row.get(21)?,
        last_error_code: row.get(22)?,
        credential_ref: row.get(23)?,
        created_at: row.get(24)?,
        updated_at: row.get(25)?,
    };
    validate_agent_account(&account).map_err(|error| invalid_column(2, error))?;
    Ok(account)
}

impl Db {
    pub fn list_agent_accounts(&self) -> Result<Vec<AgentAccount>, DbError> {
        let result = self.list_agent_accounts_with_diagnostics()?;
        if !result.diagnostics.is_empty() {
            eprintln!(
                "native-agent account list skipped {} corrupt row(s); support diagnostics retain recovery identifiers",
                result.diagnostics.len()
            );
        }
        Ok(result.accounts)
    }

    pub fn list_agent_accounts_with_diagnostics(&self) -> Result<AgentAccountList, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM agent_accounts ORDER BY provider_id, product_id, created_at, id"
            ))?;
            let mut rows = statement.query([])?;
            let mut accounts = Vec::new();
            let mut diagnostics = Vec::new();
            while let Some(row) = rows.next()? {
                let account_id = row.get::<_, String>(0).ok();
                match map_account(row) {
                    Ok(account) => accounts.push(account),
                    Err(_) => diagnostics.push(AgentAccountReadDiagnostic {
                        account_id,
                        error_code: "agent_account_row_corrupt",
                    }),
                }
            }
            Ok(AgentAccountList {
                accounts,
                diagnostics,
            })
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

    pub fn get_agent_account_by_identity(
        &self,
        provider: AgentProvider,
        product: AgentProductId,
        harness: AgentHarness,
        identity_key: &str,
    ) -> Result<Option<AgentAccount>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM agent_accounts
                         WHERE provider_id = ?1 AND product_id = ?2
                           AND harness = ?3 AND identity_key = ?4"
                    ),
                    params![
                        provider.as_str(),
                        product.as_str(),
                        harness.as_str(),
                        identity_key
                    ],
                    map_account,
                )
                .optional()?)
        })
    }

    pub fn prepare_agent_account(
        &self,
        mut input: AuthorizedAgentAccount,
    ) -> Result<AgentAccount, DbError> {
        validate_authorized_agent_account(&input).map_err(DbError::Other)?;
        if input.external_account_id.trim().is_empty() {
            return Err(DbError::Other(
                "validated account identity is required".into(),
            ));
        }
        input.scopes.sort();
        input.scopes.dedup();
        let identity_key = input.identity_key();
        let scopes_json = serde_json::to_string(&input.scopes)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();

        let id = self.with_conn(|conn| {
            let existing: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT id, credential_ref FROM agent_accounts
                     WHERE provider_id = ?1 AND product_id = ?2
                       AND harness = ?3 AND identity_key = ?4",
                    params![
                        input.provider.as_str(),
                        input.product.as_str(),
                        input.harness.as_str(),
                        identity_key
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            if let Some((id, credential_ref)) = existing {
                if input.product.requires_credential() != credential_ref.is_some() {
                    return Err(DbError::Other(
                        "agent account access contract mismatch".into(),
                    ));
                }
                conn.execute(
                    "UPDATE agent_accounts SET
                       display_name = ?1, external_account_id = ?2,
                       external_workspace_id = ?3, auth_method = ?4,
                       custody_mode = ?5, managed_runtime_id = ?6,
                       managed_runtime_version = ?7, runtime_profile_ref = ?8,
                       scopes_json = ?9, billing_source = ?10, billing_owner = ?11,
                       entitlement_state = ?12, entitlement_source = ?13,
                       entitlement_observed_at = ?14, status = 'error', expires_at = ?15,
                       last_error_code = 'account_access_pending', updated_at = ?16
                     WHERE id = ?17",
                    params![
                        input.display_name,
                        input.external_account_id,
                        input.external_workspace_id,
                        input.auth_method.as_str(),
                        input.custody_mode.as_str(),
                        input.managed_runtime_id.map(ManagedRuntimeId::as_str),
                        input.managed_runtime_version,
                        input.runtime_profile_ref,
                        scopes_json,
                        input.billing_source,
                        input.billing_owner,
                        input.entitlement_state.as_str(),
                        input.entitlement_source,
                        input.entitlement_observed_at,
                        input.expires_at,
                        updated_at,
                        id,
                    ],
                )?;
                return Ok(id);
            }

            let id = format!("account_{}", Uuid::new_v4().simple());
            let credential_ref = input
                .product
                .requires_credential()
                .then(|| format!("agent-account:{id}"));
            conn.execute(
                "INSERT INTO agent_accounts (
                   id, provider_id, product_id, harness, identity_key, display_name,
                   external_account_id, external_workspace_id, auth_method, custody_mode,
                   managed_runtime_id, managed_runtime_version, runtime_profile_ref,
                   scopes_json, billing_source, billing_owner, entitlement_state,
                   entitlement_source, entitlement_observed_at, status, expires_at,
                   last_error_code, credential_ref, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                           ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                           'error', ?20, 'account_access_pending', ?21, ?22, ?22)",
                params![
                    id,
                    input.provider.as_str(),
                    input.product.as_str(),
                    input.harness.as_str(),
                    identity_key,
                    input.display_name,
                    input.external_account_id,
                    input.external_workspace_id,
                    input.auth_method.as_str(),
                    input.custody_mode.as_str(),
                    input.managed_runtime_id.map(ManagedRuntimeId::as_str),
                    input.managed_runtime_version,
                    input.runtime_profile_ref,
                    scopes_json,
                    input.billing_source,
                    input.billing_owner,
                    input.entitlement_state.as_str(),
                    input.entitlement_source,
                    input.entitlement_observed_at,
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

    pub fn upsert_runtime_managed_account(
        &self,
        id: &str,
        mut input: AuthorizedAgentAccount,
    ) -> Result<AgentAccount, DbError> {
        validate_authorized_agent_account(&input).map_err(DbError::Other)?;
        if input.product.requires_credential() {
            return Err(DbError::Other(
                "runtime-managed accounts must not use a secret credential reference".into(),
            ));
        }
        if input.external_account_id.trim().is_empty() {
            return Err(DbError::Other(
                "validated account identity is required".into(),
            ));
        }
        input.scopes.sort();
        input.scopes.dedup();
        let identity_key = input.identity_key();
        let scopes_json = serde_json::to_string(&input.scopes)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();

        self.with_conn(|conn| {
            let existing_identity: Option<String> = conn
                .query_row(
                    "SELECT id FROM agent_accounts
                     WHERE provider_id = ?1 AND product_id = ?2
                       AND harness = ?3 AND identity_key = ?4",
                    params![
                        input.provider.as_str(),
                        input.product.as_str(),
                        input.harness.as_str(),
                        identity_key
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            if existing_identity
                .as_ref()
                .is_some_and(|existing| existing != id)
            {
                return Err(DbError::Other(
                    "agent account identity is already bound to a different account".into(),
                ));
            }

            let existing_row: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT id, credential_ref FROM agent_accounts WHERE id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            if let Some((_, credential_ref)) = existing_row {
                if credential_ref.is_some() {
                    return Err(DbError::Other(
                        "managed subscription accounts must not use a secret credential reference"
                            .into(),
                    ));
                }
                conn.execute(
                    "UPDATE agent_accounts SET
                       display_name = ?1, external_account_id = ?2,
                       external_workspace_id = ?3, auth_method = ?4,
                       custody_mode = ?5, managed_runtime_id = ?6,
                       managed_runtime_version = ?7, runtime_profile_ref = ?8,
                       scopes_json = ?9, billing_source = ?10, billing_owner = ?11,
                       entitlement_state = ?12, entitlement_source = ?13,
                       entitlement_observed_at = ?14, status = 'error', expires_at = ?15,
                       last_error_code = 'account_access_pending', updated_at = ?16
                     WHERE id = ?17",
                    params![
                        input.display_name,
                        input.external_account_id,
                        input.external_workspace_id,
                        input.auth_method.as_str(),
                        input.custody_mode.as_str(),
                        input.managed_runtime_id.map(ManagedRuntimeId::as_str),
                        input.managed_runtime_version,
                        input.runtime_profile_ref,
                        scopes_json,
                        input.billing_source,
                        input.billing_owner,
                        input.entitlement_state.as_str(),
                        input.entitlement_source,
                        input.entitlement_observed_at,
                        input.expires_at,
                        updated_at,
                        id,
                    ],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO agent_accounts (
                       id, provider_id, product_id, harness, identity_key, display_name,
                       external_account_id, external_workspace_id, auth_method, custody_mode,
                       managed_runtime_id, managed_runtime_version, runtime_profile_ref,
                       scopes_json, billing_source, billing_owner, entitlement_state,
                       entitlement_source, entitlement_observed_at, status, expires_at,
                       last_error_code, credential_ref, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                               ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                               'error', ?20, 'account_access_pending', NULL, ?21, ?21)",
                    params![
                        id,
                        input.provider.as_str(),
                        input.product.as_str(),
                        input.harness.as_str(),
                        identity_key,
                        input.display_name,
                        input.external_account_id,
                        input.external_workspace_id,
                        input.auth_method.as_str(),
                        input.custody_mode.as_str(),
                        input.managed_runtime_id.map(ManagedRuntimeId::as_str),
                        input.managed_runtime_version,
                        input.runtime_profile_ref,
                        scopes_json,
                        input.billing_source,
                        input.billing_owner,
                        input.entitlement_state.as_str(),
                        input.entitlement_source,
                        input.entitlement_observed_at,
                        input.expires_at,
                        updated_at,
                    ],
                )?;
            }
            Ok(())
        })?;

        self.get_agent_account(id)?
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

    pub fn finalize_api_key_agent_account(
        &self,
        id: &str,
        input: &AuthorizedAgentAccount,
    ) -> Result<(), DbError> {
        validate_authorized_agent_account(input).map_err(DbError::Other)?;
        if input.auth_method != AgentAuthMethod::ApiKey
            || input.custody_mode != CredentialCustodyMode::AlfredManaged
            || !input.product.requires_credential()
            || input.external_account_id.trim().is_empty()
        {
            return Err(DbError::Other("invalid API-key account metadata".into()));
        }
        let identity_key = input.identity_key();
        let checked_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE agent_accounts SET
                   identity_key = ?1, display_name = ?2, external_account_id = ?3,
                   external_workspace_id = NULL, auth_method = 'api_key',
                   custody_mode = 'alfred_managed', scopes_json = '[]',
                   billing_source = ?4, billing_owner = ?5,
                   entitlement_state = ?6, entitlement_source = ?7,
                   entitlement_observed_at = ?8, status = 'connected', expires_at = NULL,
                   last_checked_at = ?9, last_error_code = NULL, updated_at = ?9
                 WHERE id = ?10 AND provider_id = ?11 AND product_id = ?12
                   AND harness = 'alfred' AND credential_ref IS NOT NULL",
                params![
                    identity_key,
                    input.display_name,
                    input.external_account_id,
                    input.billing_source,
                    input.billing_owner,
                    input.entitlement_state.as_str(),
                    input.entitlement_source,
                    input.entitlement_observed_at,
                    checked_at,
                    id,
                    input.provider.as_str(),
                    input.product.as_str(),
                ],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("agent account not found".into()));
        }
        Ok(())
    }

    pub fn get_agent_account_credential_cleanup(
        &self,
        account_id: &str,
    ) -> Result<Option<AgentAccountCredentialCleanup>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT credential_ref, cleanup_owner
                     FROM agent_account_credential_cleanup
                     WHERE account_id = ?1",
                    params![account_id],
                    |row| {
                        Ok(AgentAccountCredentialCleanup {
                            credential_ref: row.get(0)?,
                            cleanup_owner: row.get(1)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    pub fn delete_agent_account_credential_cleanup(&self, account_id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM agent_account_credential_cleanup WHERE account_id = ?1",
                params![account_id],
            )?;
            Ok(())
        })
    }

    pub fn delete_agent_account_metadata(&self, id: &str) -> Result<(), DbError> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM agent_accounts
                 WHERE id = ?1
                   AND NOT EXISTS (
                     SELECT 1 FROM agent_account_credential_cleanup
                     WHERE account_id = ?1
                   )",
                params![id],
            )?)
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
    use crate::agent_accounts::models::AgentEntitlementState;

    fn fixture(account: &str, workspace: &str) -> AuthorizedAgentAccount {
        AuthorizedAgentAccount {
            provider: AgentProvider::Codex,
            product: AgentProductId::OpenaiApi,
            harness: AgentHarness::Alfred,
            display_name: Some(account.into()),
            external_account_id: account.into(),
            external_workspace_id: Some(workspace.into()),
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            managed_runtime_id: None,
            managed_runtime_version: None,
            runtime_profile_ref: None,
            scopes: vec!["models:read".into(), "models:read".into()],
            billing_source: "provider_api".into(),
            billing_owner: "credential_owner".into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "not_observed".into(),
            entitlement_observed_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn distinct_products_and_identities_are_deterministic() {
        let db = Db::open_in_memory().expect("database");
        let first = db
            .prepare_agent_account(fixture("one", "org"))
            .expect("first");
        let reconnect = db
            .prepare_agent_account(fixture("one", "org"))
            .expect("reconnect");
        let second = db
            .prepare_agent_account(fixture("two", "org"))
            .expect("second");

        assert_eq!(first.id, reconnect.id);
        assert_eq!(first.credential_ref, reconnect.credential_ref);
        assert_ne!(first.id, second.id);
        assert_eq!(db.list_agent_accounts().expect("list").len(), 2);
        assert_eq!(reconnect.scopes, vec!["models:read"]);
        assert_eq!(reconnect.status, AgentAccountStatus::Error);
        assert_eq!(
            reconnect.last_error_code.as_deref(),
            Some("account_access_pending")
        );
    }

    #[test]
    fn managed_subscription_uses_only_a_runtime_profile_reference() {
        let db = Db::open_in_memory().expect("database");
        let account = db
            .prepare_agent_account(AuthorizedAgentAccount {
                provider: AgentProvider::Codex,
                product: AgentProductId::ChatgptCodex,
                harness: AgentHarness::Alfred,
                display_name: Some("ChatGPT".into()),
                external_account_id: "chatgpt-user".into(),
                external_workspace_id: None,
                auth_method: AgentAuthMethod::DeviceCode,
                custody_mode: CredentialCustodyMode::RuntimeManaged,
                managed_runtime_id: Some(ManagedRuntimeId::CodexPythonSdk),
                managed_runtime_version: Some("0.147.0".into()),
                runtime_profile_ref: Some("profile-opaque".into()),
                scopes: Vec::new(),
                billing_source: "provider_subscription".into(),
                billing_owner: "subscription_account".into(),
                entitlement_state: AgentEntitlementState::Eligible,
                entitlement_source: "runtime_account".into(),
                entitlement_observed_at: Some("2026-08-26T12:00:00Z".into()),
                expires_at: None,
            })
            .expect("managed account");
        assert_eq!(
            account.runtime_profile_ref.as_deref(),
            Some("profile-opaque")
        );
        assert!(account.credential_ref.is_none());
    }

    #[test]
    fn one_corrupt_row_does_not_hide_valid_accounts_or_destroy_recovery_data() {
        let db = Db::open_in_memory().expect("database");
        let valid = db
            .prepare_agent_account(fixture("valid", "org"))
            .expect("valid account");
        let corrupt = db
            .prepare_agent_account(fixture("corrupt", "org"))
            .expect("account to corrupt");
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE agent_accounts SET billing_owner = 'wrong_owner' WHERE id = ?1",
                params![corrupt.id],
            )?;
            Ok(())
        })
        .expect("corrupt one row");

        let listed = db
            .list_agent_accounts_with_diagnostics()
            .expect("resilient list");
        assert_eq!(listed.accounts.len(), 1);
        assert_eq!(listed.accounts[0].id, valid.id);
        assert_eq!(
            listed.diagnostics,
            vec![AgentAccountReadDiagnostic {
                account_id: Some(corrupt.id.clone()),
                error_code: "agent_account_row_corrupt",
            }]
        );
        assert_eq!(db.list_agent_accounts().expect("public list").len(), 1);
        let retained: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM agent_accounts WHERE id = ?1",
                    params![corrupt.id],
                    |row| row.get(0),
                )?)
            })
            .expect("corrupt row remains recoverable");
        assert_eq!(retained, 1);
    }
}
