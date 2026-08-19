//! Experimental WhatsApp linked-device integration (Plan 023).
//!
//! Outbound-only, one account, self-chat destination only. Nothing in this
//! module may expose inbound content, history, contacts, or the raw protocol
//! client. See `plans/023-whatsapp-linked-device-self-notifications.md`.

pub mod crypto;
pub mod keyring;
pub mod provider;
pub mod store;
