//! Plan 023 Step 1 feasibility spike — DISPOSABLE, NON-SHIPPING.
//!
//! Proves (or disproves) that `whatsapp-rust` 0.7.0 can:
//!   1.2 pair by QR without pair-code/phone-number setup
//!   1.3 expose a stable own JID suitable for self-send
//!   1.4 send 1:1 text to that own JID and return a stable message ID
//!   1.5 shut down, restore from durable state, reconnect, send again, log out
//!   1.6 ignore message/history-sync events without decoding or persisting them
//!   1.8 keep QR payloads, JIDs, keys and bodies out of default logs
//!
//! This crate is NOT part of the Tauri build graph and must never ship.
//! The shipping configuration is `default-features = false` (see README.md).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use log::{error, info, warn};
use tokio::sync::Notify;
use whatsapp_rust::prelude::*;

/// Session database for the spike only. Never a shipping path.
const SPIKE_DB: &str = "spike-session.db";
/// How long we wait for the socket to authenticate before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);
/// Plan 023 caps an interpolated body at 4096 Unicode chars.
const MAX_MESSAGE_CHARS: usize = 4096;

fn main() -> Result<()> {
    // `info` mirrors what a shipping build would emit by default, which is the
    // level the Step 1.8 redaction scan has to be clean at.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let command = std::env::args().nth(1).unwrap_or_else(|| "help".to_string());
    let rest: Vec<String> = std::env::args().skip(2).collect();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move {
        match command.as_str() {
            "pair" => pair().await,
            "send" => send(&rest.join(" ")).await,
            "logout" => logout().await,
            _ => {
                usage();
                Ok(())
            }
        }
    })
}

fn usage() {
    println!(
        "Plan 023 Step 1 spike\n\
         \n\
         cargo run -- pair          Link a device by QR, print the masked own JID\n\
         cargo run -- send <text>   Restore the session and self-send <text>\n\
         cargo run -- logout        Remote-logout and delete {SPIKE_DB}\n"
    );
}

/// Step 1.2 + 1.3: QR pairing, then read back the authenticated own JID.
async fn pair() -> Result<()> {
    if std::path::Path::new(SPIKE_DB).exists() {
        bail!("{SPIKE_DB} already exists — run `logout` first so pairing starts clean");
    }

    let (handle, _) = start(true).await?;
    let client = handle.client();

    let (pn, lid) = own_jids(&client)?;
    info!("paired own PN  : {}", mask(&pn));
    info!("paired own LID : {}", lid.as_deref().map_or("<none>", |l| l));
    info!("Step 1.3 GREEN — own JID is available after pairing");

    handle.shutdown().await;
    Ok(())
}

/// Step 1.4 + 1.5: restore durable state, reconnect, self-send, return message ID.
async fn send(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("message body is empty");
    }
    if text.chars().count() > MAX_MESSAGE_CHARS {
        bail!(
            "message body is {} chars, over the {MAX_MESSAGE_CHARS} cap",
            text.chars().count()
        );
    }
    if !std::path::Path::new(SPIKE_DB).exists() {
        bail!("no {SPIKE_DB} — run `pair` first");
    }

    let (handle, restored) = start(false).await?;
    if !restored {
        warn!("session was not restored from disk; a QR was requested instead");
    }
    let client = handle.client();

    let (pn, _) = own_jids(&client)?;
    // The self-chat destination is derived here and nowhere else. No caller,
    // workflow, or frontend can supply or influence it.
    let self_jid: Jid = pn.to_non_ad();

    let sent = client
        .send_message(self_jid.clone(), wa::Message::text(text))
        .await
        .map_err(|e| anyhow!("send failed: {e}"))?;

    // Only the message ID and a masked destination are ever surfaced. The body
    // is deliberately absent from this line.
    info!(
        "Step 1.4 GREEN — submitted message_id={} to {}",
        sent.message_id,
        mask(&self_jid)
    );

    handle.shutdown().await;
    Ok(())
}

/// Step 1.5: remote logout, then remove every local trace of the session.
async fn logout() -> Result<()> {
    if std::path::Path::new(SPIKE_DB).exists() {
        let (handle, restored) = start(false).await?;
        if restored {
            handle.client().logout().await;
            info!("remote logout requested");
        }
        handle.shutdown().await;
    }

    // Local deletion happens regardless of whether the remote logout landed —
    // the same rule Plan 023 Step 7 puts on the shipping disconnect path.
    let mut removed = 0;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let path = format!("{SPIKE_DB}{suffix}");
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    info!("Step 1.5 GREEN — removed {removed} local session file(s)");
    Ok(())
}

/// Builds and starts one client. Returns the handle and whether a stored
/// session was restored (`false` means the backend asked to pair again).
async fn start(expect_qr: bool) -> Result<(BotHandle, bool)> {
    let store = SqliteStore::new(SPIKE_DB)
        .await
        .map_err(|e| anyhow!("open spike store: {e}"))?;

    let connected = Arc::new(Notify::new());
    let logged_out = Arc::new(Notify::new());
    let saw_qr = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let on_connected = connected.clone();
    let on_logged_out = logged_out.clone();
    let qr_flag = saw_qr.clone();

    let bot = Bot::builder()
        .with_backend(store)
        // Step 1.6: refuse the history-sync stream outright rather than
        // receiving blobs and discarding them after the fact.
        .skip_history_sync()
        // Plan 023 Step 6 caps sends at 5/min. Proving the knob exists here
        // means the shipping limiter can lean on it instead of reimplementing.
        .with_resend_rate_limit(5, 5)
        .on_qr_code(move |code, timeout| {
            let qr_flag = qr_flag.clone();
            async move {
                qr_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                // The payload is rendered straight to the terminal and never
                // logged, stored, or echoed as text (Step 1.8).
                if qr2term::print_qr(&code).is_err() {
                    error!("could not render the QR code in this terminal");
                }
                info!(
                    "scan within {}s: WhatsApp > Linked Devices > Link a device",
                    timeout.as_secs()
                );
            }
        })
        .on_connected(move |_client| {
            let on_connected = on_connected.clone();
            async move {
                on_connected.notify_waiters();
            }
        })
        .on_logged_out(move |_info| {
            let on_logged_out = on_logged_out.clone();
            async move {
                on_logged_out.notify_waiters();
            }
        })
        .build()
        .await
        .map_err(|e| anyhow!("build client: {e}"))?;

    // No `on_message` handler is registered anywhere in this spike. Inbound
    // content is never observed, decoded, or persisted (Step 1.6).
    let handle = bot.spawn();

    let wait_connected = connected.notified();
    let wait_logged_out = logged_out.notified();
    tokio::select! {
        _ = wait_connected => {}
        _ = wait_logged_out => bail!("session is logged out — relink required"),
        _ = tokio::time::sleep(CONNECT_TIMEOUT) => {
            bail!("timed out after {}s waiting for an authenticated connection", CONNECT_TIMEOUT.as_secs())
        }
    }

    let restored = !saw_qr.load(std::sync::atomic::Ordering::SeqCst);
    if expect_qr && restored {
        warn!("connected without showing a QR — an existing session was reused");
    }
    Ok((handle, restored))
}

/// Reads the authenticated own JID pair off the live client.
fn own_jids(client: &Arc<Client>) -> Result<(Jid, Option<String>)> {
    let pn = client
        .pn()
        .ok_or_else(|| anyhow!("client has no own PN JID after connecting"))?;
    let lid = client.lid().map(|l| mask(&l));
    Ok((pn, lid))
}

/// Masks a JID down to what Plan 023 allows in a DTO: the last two digits of
/// the user part plus the server. Never the full identifier.
fn mask(jid: &Jid) -> String {
    let user = &jid.user;
    let tail: String = user.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
    format!("***{tail}@{}", jid.server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_keeps_only_the_last_two_digits() {
        let jid: Jid = "34600123456@s.whatsapp.net".parse().unwrap();
        let masked = mask(&jid);
        assert_eq!(masked, "***56@s.whatsapp.net");
        assert!(!masked.contains("34600123456"));
        assert!(!masked.contains("3460012345"));
    }

    #[test]
    fn mask_survives_a_short_user_part() {
        let jid: Jid = "7@s.whatsapp.net".parse().unwrap();
        assert_eq!(mask(&jid), "***7@s.whatsapp.net");
    }
}
