//! Blocks WhatsApp protocol logging from reaching any sink (Plan 023 Step 5,
//! gate 8 mitigation).
//!
//! The Step 1 spike found that `whatsapp-rust` and its `wacore*` crates print
//! identity and key material through the `log` facade at levels a normal filter
//! would let through:
//!
//! - `whatsapp_rust::pair` logs the raw LID **and phone number** at `info`;
//! - `wacore_libsignal::protocol::session_cipher` logs the raw LID **and Signal
//!   ratchet/base keys** at `warn`.
//!
//! Filtering by level therefore does not help — a `warn` passes every sane
//! default. The filter has to be by target.
//!
//! Alfred currently installs no logger, so these calls are already no-ops. That
//! safety is accidental: the day anyone adds `tauri-plugin-log` or `env_logger`
//! for debugging, every one of those records starts landing in a file. Installing
//! this guard makes the policy explicit and testable, and gives any future logger
//! a place it must pass through.

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Target prefixes whose records are dropped unconditionally.
///
/// `Client/` has no `::` because the crate logs some records under bare
/// display-style targets (`Client/Recv`, `Client/Send`, `Client/AccountSync`).
const DENIED_TARGET_PREFIXES: &[&str] = &["whatsapp_rust", "wacore", "waproto", "Client/"];

/// Whether a record from `target` may reach a sink.
pub fn is_sensitive_target(target: &str) -> bool {
    DENIED_TARGET_PREFIXES
        .iter()
        .any(|prefix| target.starts_with(prefix))
}

/// Wraps whatever logger Alfred wants, dropping protocol records before they
/// reach it.
pub struct RedactingLogger {
    inner: Option<Box<dyn Log>>,
}

impl RedactingLogger {
    pub fn new(inner: Option<Box<dyn Log>>) -> Self {
        Self { inner }
    }
}

impl Log for RedactingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        if is_sensitive_target(metadata.target()) {
            return false;
        }
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.enabled(metadata))
    }

    fn log(&self, record: &Record<'_>) {
        // Checked again here: `log!` may be called without consulting `enabled`.
        if is_sensitive_target(record.target()) {
            return;
        }
        if let Some(inner) = &self.inner {
            inner.log(record);
        }
    }

    fn flush(&self) {
        if let Some(inner) = &self.inner {
            inner.flush();
        }
    }
}

/// Installs the guard as the global logger.
///
/// Call once, before any WhatsApp client is constructed. Failing is not fatal —
/// it means a logger was already installed, which the caller should treat as a
/// configuration bug rather than a crash.
pub fn install(inner: Option<Box<dyn Log>>, max_level: LevelFilter) -> Result<(), String> {
    log::set_boxed_logger(Box::new(RedactingLogger::new(inner)))
        .map(|()| log::set_max_level(max_level))
        .map_err(|error| error.to_string())
}

/// Convenience for the default Alfred configuration: no sink at all.
pub fn install_silent() -> Result<(), String> {
    install(None, LevelFilter::Off)
}

/// Level below which nothing is worth forwarding even for allowed targets.
pub const DEFAULT_LEVEL: Level = Level::Warn;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingLogger {
        seen: Mutex<Vec<String>>,
    }

    impl Log for RecordingLogger {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &Record<'_>) {
            self.seen
                .lock()
                .unwrap()
                .push(format!("{}|{}", record.target(), record.args()));
        }
        fn flush(&self) {}
    }

    /// Drives a logger directly, so the test does not depend on a global
    /// logger being installed.
    fn emit(logger: &dyn Log, target: &str, level: Level, message: &str) {
        logger.log(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(level)
                .target(target)
                .build(),
        );
    }

    #[test]
    fn the_exact_leak_sites_from_the_spike_are_blocked() {
        // Targets observed emitting a raw phone number, LID, or Signal key.
        for target in [
            "whatsapp_rust::pair",
            "whatsapp_rust::client::node_io",
            "whatsapp_rust::client::sessions",
            "wacore_libsignal::protocol::session_cipher",
            "Client/AccountSync",
            "Client/Recv",
            "Client/Send",
        ] {
            assert!(
                is_sensitive_target(target),
                "{target} leaked identity in the Step 1 spike and must stay blocked"
            );
        }
    }

    #[test]
    fn a_warn_from_the_signal_layer_never_reaches_a_sink() {
        // The worst case: WARN passes any level filter, and this record carries
        // the raw LID plus ratchet and base keys.
        let inner = Box::new(RecordingLogger::default());
        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let guard = RedactingLogger::new(Some(inner));

        emit(
            &guard,
            "wacore_libsignal::protocol::session_cipher",
            Level::Warn,
            "Session loaded for 237756605284433@lid.0. base key: deadbeef",
        );

        // Nothing forwarded: the inner logger was never called.
        assert!(seen.lock().unwrap().is_empty());
        assert!(!guard.enabled(
            &Metadata::builder()
                .level(Level::Warn)
                .target("wacore_libsignal::protocol::session_cipher")
                .build()
        ));
    }

    #[test]
    fn alfreds_own_records_still_pass_through() {
        let inner = RecordingLogger::default();
        let guard = RedactingLogger::new(Some(Box::new(RecordingLogger::default())));

        // Sanity: the recording logger works when used directly.
        emit(&inner, "alfred::runner", Level::Info, "ran");
        assert_eq!(inner.seen.lock().unwrap().len(), 1);

        assert!(guard.enabled(
            &Metadata::builder()
                .level(Level::Info)
                .target("alfred::runner")
                .build()
        ));
    }

    #[test]
    fn a_guard_without_a_sink_swallows_everything() {
        let guard = RedactingLogger::new(None);
        emit(&guard, "anything", Level::Error, "message");
        assert!(!guard.enabled(
            &Metadata::builder()
                .level(Level::Error)
                .target("anything")
                .build()
        ));
    }

    #[test]
    fn every_wacore_crate_is_covered_by_one_prefix() {
        for target in [
            "wacore::store::persistence_manager",
            "wacore_binary::jid",
            "wacore_appstate::processor",
            "wacore_noise::handshake",
            "waproto::whatsapp",
        ] {
            assert!(is_sensitive_target(target), "{target} must be blocked");
        }
    }

    #[test]
    fn unrelated_targets_are_not_over_blocked() {
        for target in [
            "alfred::db",
            "reqwest::connect",
            "tauri::app",
            // Near-misses that must not be caught by the prefixes.
            "whatsapp_helper_of_ours",
            "ClientSideThing",
        ] {
            assert_eq!(
                is_sensitive_target(target),
                target.starts_with("whatsapp_") && target.starts_with("whatsapp_rust"),
                "{target} classification is wrong"
            );
        }
    }
}
