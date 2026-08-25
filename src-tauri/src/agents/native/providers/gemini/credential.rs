//! The one place a stored account credential becomes a Gemini API auth key.
//!
//! Alfred never reads a Gemini CLI credential file, a `GEMINI_API_KEY`
//! environment variable, or an Application Default Credentials file. The key
//! arrives from the Plan 031 credential store and nowhere else.

use crate::agent_accounts::models::CredentialCustodyMode;
use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agents::native::{NativeErrorCode, NativeRuntimeError, ResolvedNativeAccount};
use std::fmt;

/// Where the API key may be carried inside the stored envelope.
const API_KEY_FIELD: &str = "api_key";

/// Google's keys are short; anything outside this band is not a key.
const MIN_KEY_BYTES: usize = 20;
const MAX_KEY_BYTES: usize = 512;

/// A validated Gemini API auth key.
///
/// Deliberately not `Serialize` and not printable: its `Debug` is redacted and
/// [`redact`] scrubs the literal key out of any provider-derived text before it
/// can reach an event, an error, or a log line.
#[derive(Clone)]
pub struct GeminiCredential {
    key: String,
}

#[cfg(test)]
pub(super) struct TestGeminiApiKey(pub String);

impl fmt::Debug for GeminiCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeminiCredential([REDACTED])")
    }
}

impl GeminiCredential {
    /// The value for the `x-goog-api-key` request header. This is the only
    /// accessor, and it is never called anywhere except the transport.
    pub fn header_value(&self) -> &str {
        &self.key
    }

    /// Removes the literal key from text that came back from the provider.
    ///
    /// The shared redactor knows `Bearer`, `sk-`, and friends; it does not know
    /// this account's key, so the provider redacts its own secret first.
    pub fn redact(&self, text: &str) -> String {
        if self.key.is_empty() || !text.contains(&self.key) {
            return text.to_owned();
        }
        text.replace(&self.key, "[REDACTED]")
    }
}

/// Extracts the API key from a resolved account, refusing every other shape.
pub fn credential_from(
    account: &ResolvedNativeAccount,
) -> Result<GeminiCredential, NativeRuntimeError> {
    #[cfg(test)]
    if let Some(key) = account.credential.downcast_ref::<TestGeminiApiKey>() {
        validate_key(&key.0)?;
        return Ok(GeminiCredential { key: key.0.clone() });
    }

    let stored = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .ok_or_else(|| {
            unavailable("gemini native mode requires an Alfred-managed account credential")
        })?;

    // A runtime-managed credential is a CLI-owned credential. Native mode never
    // borrows one, even when it happens to sit in the same store.
    if stored.custody_mode() != CredentialCustodyMode::AlfredManaged {
        return Err(unavailable(
            "gemini native mode never borrows a runtime-managed or CLI credential",
        ));
    }

    let key = stored
        .provider_field(API_KEY_FIELD)
        .or_else(|| stored.access_token())
        .ok_or_else(|| unavailable("gemini account has no stored API key"))?;

    validate_key(key)?;
    Ok(GeminiCredential { key: key.to_owned() })
}

/// Bounds the key without guessing at a prefix.
///
/// Google is migrating standard keys to service-account-bound auth keys, so a
/// hard-coded `AIza` prefix check would start rejecting valid keys. Shape,
/// length, and the absence of whitespace are what can be asserted honestly.
fn validate_key(key: &str) -> Result<(), NativeRuntimeError> {
    let valid = (MIN_KEY_BYTES..=MAX_KEY_BYTES).contains(&key.len())
        && key.trim() == key
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(unavailable(
            "stored gemini API key is malformed; reconnect the account",
        ))
    }
}

fn unavailable(message: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::AccountUnavailable, message, false)
}
