//! Transient OpenCode Go key intake and managed-profile custody.

use super::launch::OpenCodeServerProvider;
use super::protocol::parse_go_models;
use crate::agent_accounts::models::{AgentProductId, CredentialCustodyMode, ManagedRuntimeId};
use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agent_accounts::runtime_profile::RuntimeProfileRef;
use crate::agents::native::{
    NativeCancellation, NativeErrorCode, NativeRuntimeError, ResolvedNativeAccount,
};
use crate::agents::{AgentProvider, OpaqueAgentAccountRef};
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

pub const OPENCODE_GO_PROVIDER_ID: &str = "opencode-go";
pub const OPENCODE_GO_USAGE_URL: &str = "https://opencode.ai/auth";
const MAX_GO_KEY_BYTES: usize = 4 * 1024;

/// Secret-entry value that is consumed by the runtime auth endpoint and then
/// zeroized. It is intentionally neither `Clone`, `Serialize`, nor printable.
pub struct OpenCodeGoKey(Zeroizing<String>);

impl OpenCodeGoKey {
    pub fn parse(mut value: String) -> Result<Self, NativeRuntimeError> {
        let valid = value.len() >= 16
            && value.len() <= MAX_GO_KEY_BYTES
            && value.trim() == value
            && value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'\0');
        if valid {
            Ok(Self(Zeroizing::new(value)))
        } else {
            value.zeroize();
            Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "OpenCode Go key is invalid",
                false,
            ))
        }
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for OpenCodeGoKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenCodeGoKey([REDACTED])")
    }
}

/// Backend-only account operation boundary. Command DTOs must never retain the
/// key or the returned opaque profile reference.
pub struct OpenCodeAccountManager {
    servers: Arc<dyn OpenCodeServerProvider>,
}

impl OpenCodeAccountManager {
    pub fn new(servers: Arc<dyn OpenCodeServerProvider>) -> Self {
        Self { servers }
    }

    pub fn connect(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        repository: &Path,
        key: OpenCodeGoKey,
        cancellation: &NativeCancellation,
    ) -> Result<RuntimeProfileRef, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let (profile_ref, server) =
            self.servers
                .create_and_launch(account_ref, repository, cancellation)?;
        let result = (|| {
            server.api().set_go_key(&key)?;
            // `/provider` is the only catalog authority. Successful intake is
            // not allowed to infer or select Zen/another provider.
            let catalog = server.api().list_providers(path_text(repository)?)?;
            if parse_go_models(&catalog)?.is_empty() {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "OpenCode Go key did not expose any OpenCode Go models",
                    false,
                ));
            }
            Ok(())
        })();
        drop(key);
        let stop = server.stop();
        let completed = result.and(stop);
        if let Err(error) = completed {
            let _ = self.servers.purge_profile(account_ref, &profile_ref);
            return Err(error);
        }
        Ok(profile_ref)
    }

    pub fn disconnect(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        profile_ref: &RuntimeProfileRef,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        cancellation.checkpoint()?;
        let launched =
            self.servers
                .launch_existing(account_ref, profile_ref, repository, cancellation);
        let endpoint_result = match launched {
            Ok(server) => {
                let result = server.api().delete_go_key();
                let stop = server.stop();
                result.and(stop)
            }
            Err(error) => Err(error),
        };
        // Purging the isolated profile is the authoritative credential
        // deletion. It is attempted even when the runtime is already broken.
        let purge_result = self.servers.purge_profile(account_ref, profile_ref);
        match (endpoint_result, purge_result) {
            (_, Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

pub(crate) fn profile_ref_for_account(
    account: &ResolvedNativeAccount,
) -> Result<RuntimeProfileRef, NativeRuntimeError> {
    if account.provider != AgentProvider::Opencode || account.product != AgentProductId::OpencodeGo
    {
        return Err(account_mismatch());
    }

    #[cfg(test)]
    if let Some(credential) = account
        .credential
        .downcast_ref::<TestOpenCodeProfileCredential>()
    {
        return Ok(credential.0.clone());
    }

    let credential = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .ok_or_else(account_mismatch)?;
    if credential.custody_mode() != CredentialCustodyMode::RuntimeManaged
        || credential.managed_runtime_id() != Some(ManagedRuntimeId::OpencodeServer)
        || credential.managed_runtime_version() != Some(super::package::OPENCODE_RUNTIME_VERSION)
        || credential.access_token().is_some()
        || credential.refresh_token().is_some()
        || credential.runtime_credential_ref().is_some()
        || credential.expires_at().is_some()
    {
        return Err(account_mismatch());
    }
    let value = credential.runtime_profile_ref().ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "OpenCode Go managed profile is missing",
            false,
        )
    })?;
    RuntimeProfileRef::parse(value).map_err(|_| {
        NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "OpenCode Go managed profile is invalid",
            false,
        )
    })
}

#[cfg(test)]
pub(crate) struct TestOpenCodeProfileCredential(pub RuntimeProfileRef);

fn path_text(path: &Path) -> Result<&str, NativeRuntimeError> {
    path.to_str().ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "OpenCode repository path is not valid UTF-8",
            false,
        )
    })
}

fn account_mismatch() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::AccountMismatch,
        "OpenCode Go runtime received an incompatible account",
        false,
    )
}
