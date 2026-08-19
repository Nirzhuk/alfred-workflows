//! Provider contract for the experimental WhatsApp linked device
//! (Plan 023 Step 3).
//!
//! `whatsapp` is a connected app, never an `AgentProviderId`: it does not run a
//! model or execute a workflow node. It is action-only and publishes no event
//! descriptors.
//!
//! Everything here is deliberately narrow. The full own JID, the device
//! identity, the store key, and all protocol material stay behind the
//! credential and protocol-store boundary; only a digest and a masked suffix
//! ever reach Alfred's main database or a command DTO.

use std::collections::BTreeMap;

use crate::integrations::models::canonical_identity_key;

pub const PROVIDER_ID: &str = "whatsapp";
pub const CONNECTION_MODE: &str = "linked_device_experimental";

/// Version of the on-disk protocol store layout. Backend-only metadata, so a
/// future migration can tell which schema a connection was created against.
pub const STORE_VERSION: &str = "1";

/// Backend-only `provider_metadata` keys. None of these cross the command
/// boundary — `AppConnectionDto` omits `provider_metadata` entirely.
pub mod metadata_key {
    pub const STORE_VERSION: &str = "store_version";
    pub const STORE_PATH: &str = "store_path";
    pub const MASKED_ACCOUNT: &str = "masked_account";
    pub const ACKNOWLEDGED_RISK_VERSION: &str = "acknowledged_risk_version";
    pub const ACKNOWLEDGED_AT: &str = "acknowledged_at";
}

/// Risk-acknowledgement copy version. Bumping it forces the user to accept the
/// warning again before the pairing flow will start (Plan 023 Step 4).
pub const RISK_ACKNOWLEDGEMENT_VERSION: &str = "1";

/// Whether WhatsApp may be offered on this build's operating system.
///
/// Plan 023 enables the provider per OS only after that OS has passed its own
/// packaged smoke gate. None have yet, so release builds hide it everywhere.
/// Development builds expose it so the remaining steps can be worked on; that
/// is the only reason this is not a flat `false`.
pub const fn is_available() -> bool {
    PACKAGED_GATE_PASSED || cfg!(debug_assertions)
}

/// Flip to `true` per target only after `plans/023` records a green packaged
/// smoke for that OS. A failed platform gate must never block Telegram.
#[cfg(target_os = "macos")]
const PACKAGED_GATE_PASSED: bool = false;
#[cfg(target_os = "windows")]
const PACKAGED_GATE_PASSED: bool = false;
#[cfg(target_os = "linux")]
const PACKAGED_GATE_PASSED: bool = false;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const PACKAGED_GATE_PASSED: bool = false;

/// Canonical identity for the one linked account.
///
/// Built from the provider, the mode, and the **authenticated** own JID — never
/// from anything the frontend supplied. Callers derive this once, immediately
/// after pairing, and then drop the plaintext JID.
pub fn identity_key(own_jid: &str) -> String {
    canonical_identity_key(PROVIDER_ID, CONNECTION_MODE, &[own_jid])
}

/// The only account text allowed outside the protocol store: the last two
/// characters of the user part, plus the server.
///
/// A JID shorter than the mask still yields no more than it already showed.
pub fn masked_account(own_jid: &str) -> String {
    let (user, server) = own_jid.split_once('@').unwrap_or((own_jid, ""));
    // Strip any device/agent suffix (`:17`, `.0`) before masking so the mask is
    // stable across reconnects that change the device id.
    let user = user
        .split_once(':')
        .map_or(user, |(head, _)| head)
        .split_once('.')
        .map_or_else(|| user.split_once(':').map_or(user, |(head, _)| head), |(head, _)| head);

    let tail: String = {
        let chars: Vec<char> = user.chars().collect();
        let start = chars.len().saturating_sub(2);
        chars[start..].iter().collect()
    };

    if server.is_empty() {
        format!("***{tail}")
    } else {
        format!("***{tail}@{server}")
    }
}

/// Non-sensitive label for the connection row. Intentionally constant: a
/// profile or push name would be account-identifying.
pub fn display_name() -> String {
    "WhatsApp (experimental)".to_string()
}

/// Backend-only metadata for a ready connection.
pub fn connection_metadata(
    own_jid: &str,
    store_path: &str,
    acknowledged_at: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (metadata_key::STORE_VERSION.into(), STORE_VERSION.into()),
        (metadata_key::STORE_PATH.into(), store_path.into()),
        (
            metadata_key::MASKED_ACCOUNT.into(),
            masked_account(own_jid),
        ),
        (
            metadata_key::ACKNOWLEDGED_RISK_VERSION.into(),
            RISK_ACKNOWLEDGEMENT_VERSION.into(),
        ),
        (metadata_key::ACKNOWLEDGED_AT.into(), acknowledged_at.into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::catalog::ProviderCatalog;

    const OWN_JID: &str = "34600123456@s.whatsapp.net";

    #[test]
    fn the_catalog_lists_whatsapp_as_experimental_and_single_account() {
        let catalog = ProviderCatalog::default();
        let provider = catalog.get(PROVIDER_ID).expect("whatsapp is registered");

        assert!(provider.experimental, "the risk badge depends on this flag");
        assert!(provider.single_connection);
        assert_eq!(provider.connection_modes, vec![CONNECTION_MODE]);
        assert!(catalog.is_single_connection(PROVIDER_ID));
    }

    #[test]
    fn the_capability_copy_stays_action_only_and_names_the_running_requirement() {
        let catalog = ProviderCatalog::default();
        let summary = &catalog.get(PROVIDER_ID).unwrap().capability_summary;

        assert!(summary.contains("your own WhatsApp chat"));
        assert!(summary.contains("while Alfred is running"));
        // No recipient, contact, or group capability may be advertised.
        for forbidden in ["contact", "group", "recipient", "receive", "inbox"] {
            assert!(
                !summary.to_lowercase().contains(forbidden),
                "capability copy must not advertise {forbidden}"
            );
        }
    }

    #[test]
    fn other_providers_are_not_marked_experimental() {
        let catalog = ProviderCatalog::default();
        for id in ["slack", "telegram", "github", "notion"] {
            let provider = catalog.get(id).expect("provider is registered");
            assert!(!provider.experimental, "{id} must not carry the badge");
            assert!(!provider.single_connection, "{id} allows several accounts");
        }
    }

    #[test]
    fn release_builds_hide_whatsapp_until_a_packaged_gate_passes() {
        // The gate is what keeps an unvalidated OS from offering the provider.
        assert_eq!(is_available(), PACKAGED_GATE_PASSED || cfg!(debug_assertions));
        if !cfg!(debug_assertions) {
            assert!(!is_available(), "no OS has passed its packaged smoke yet");
        }
    }

    #[test]
    fn identity_is_derived_from_the_authenticated_jid_and_is_stable() {
        let first = identity_key(OWN_JID);
        assert_eq!(first, identity_key(OWN_JID));
        assert_ne!(first, identity_key("34600999999@s.whatsapp.net"));
        // Never the raw JID, and separated from every other provider/mode pair.
        assert!(!first.contains("34600123456"));
        assert_ne!(
            first,
            canonical_identity_key("telegram", CONNECTION_MODE, &[OWN_JID])
        );
        assert_ne!(
            first,
            canonical_identity_key(PROVIDER_ID, "private_bot", &[OWN_JID])
        );
    }

    #[test]
    fn masking_keeps_only_the_last_two_characters() {
        assert_eq!(masked_account(OWN_JID), "***56@s.whatsapp.net");
        assert!(!masked_account(OWN_JID).contains("34600123456"));
        assert!(!masked_account(OWN_JID).contains("3460012345"));
    }

    #[test]
    fn masking_is_stable_across_device_suffixes() {
        // The device id changes on relink; the mask must not.
        let expected = "***56@s.whatsapp.net";
        assert_eq!(masked_account("34600123456:17@s.whatsapp.net"), expected);
        assert_eq!(masked_account("34600123456.0:17@s.whatsapp.net"), expected);
        assert_eq!(masked_account("34600123456@s.whatsapp.net"), expected);
    }

    #[test]
    fn masking_handles_short_and_malformed_input() {
        assert_eq!(masked_account("7@s.whatsapp.net"), "***7@s.whatsapp.net");
        assert_eq!(masked_account(""), "***");
        assert_eq!(masked_account("nolocalpart"), "***rt");
    }

    #[test]
    fn backend_metadata_carries_no_raw_identity() {
        let metadata = connection_metadata(OWN_JID, "/tmp/whatsapp/protocol.db", "2026-08-19");

        assert_eq!(
            metadata.get(metadata_key::MASKED_ACCOUNT).unwrap(),
            "***56@s.whatsapp.net"
        );
        assert_eq!(metadata.get(metadata_key::STORE_VERSION).unwrap(), "1");
        assert_eq!(
            metadata
                .get(metadata_key::ACKNOWLEDGED_RISK_VERSION)
                .unwrap(),
            RISK_ACKNOWLEDGEMENT_VERSION
        );

        let serialized = serde_json::to_string(&metadata).unwrap();
        assert!(!serialized.contains("34600123456"));
    }

    #[test]
    fn the_display_label_is_not_account_identifying() {
        let label = display_name();
        assert!(label.to_lowercase().contains("experimental"));
        assert!(!label.contains("34600123456"));
    }
}
