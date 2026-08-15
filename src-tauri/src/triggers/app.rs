//! Local connected-app event runtime.
//!
//! The loop is intentionally separate from file/webhook triggers. It schedules
//! provider adapters, persists backoff/health, and drains the durable event
//! queue only when the workflow's single-run slot is free.

use crate::agents::active;
use crate::db::{Db, Trigger};
use crate::integrations::events::{
    next_retry_at, AppEventCancellation, AppEventDeliveryMode, AppEventErrorCode,
};
use crate::integrations::IntegrationsState;
use crate::runner;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppEventRuntime {
    revision: AtomicU64,
    stopped: AtomicBool,
    in_flight: Mutex<HashMap<String, AppEventCancellation>>,
    last_pruned_day: AtomicI64,
    recovered_interrupted_runs: AtomicBool,
    polling_concurrency: Arc<tokio::sync::Semaphore>,
    socket_concurrency: Arc<tokio::sync::Semaphore>,
}

impl Default for AppEventRuntime {
    fn default() -> Self {
        Self {
            revision: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            in_flight: Mutex::new(HashMap::new()),
            last_pruned_day: AtomicI64::new(0),
            recovered_interrupted_runs: AtomicBool::new(false),
            polling_concurrency: Arc::new(tokio::sync::Semaphore::new(4)),
            // Socket adapters spend most of their time waiting for a provider
            // frame. Keep their permits separate so several installations do
            // not prevent replayable polling adapters from making progress.
            socket_concurrency: Arc::new(tokio::sync::Semaphore::new(16)),
        }
    }
}

impl AppEventRuntime {
    pub fn reload(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        if let Ok(active) = self.in_flight.lock() {
            for cancellation in active.values() {
                cancellation.cancel();
            }
        }
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn begin(&self, trigger_id: &str) -> Option<AppEventCancellation> {
        let mut active = self.in_flight.lock().ok()?;
        if active.contains_key(trigger_id) {
            return None;
        }
        let cancellation = AppEventCancellation::never();
        active.insert(trigger_id.to_owned(), cancellation.clone());
        Some(cancellation)
    }

    fn finish(&self, trigger_id: &str) {
        if let Ok(mut active) = self.in_flight.lock() {
            active.remove(trigger_id);
        }
    }
}

impl Drop for AppEventRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub async fn tick(app: &AppHandle) -> Result<(), String> {
    let Some(runtime) = app.try_state::<AppEventRuntime>() else {
        return Ok(());
    };
    if runtime.is_stopped() {
        return Ok(());
    }
    let Some(db) = app.try_state::<Db>() else {
        return Ok(());
    };

    if !runtime
        .recovered_interrupted_runs
        .swap(true, Ordering::SeqCst)
    {
        let _ = db.fail_interrupted_app_event_runs();
    }

    drain_queue(app, db.inner())?;
    let day = Utc::now().timestamp() / 86_400;
    if runtime.last_pruned_day.swap(day, Ordering::SeqCst) != day {
        let _ = db.prune_app_event_receipts();
    }

    let triggers = db
        .list_enabled_triggers(Some("app"))
        .map_err(|error| error.to_string())?;
    for trigger in triggers {
        if !is_due(db.inner(), app, &trigger) {
            continue;
        }
        let concurrency = if uses_socket_delivery(app, &trigger) {
            runtime.socket_concurrency.clone()
        } else {
            runtime.polling_concurrency.clone()
        };
        let Ok(permit) = concurrency.try_acquire_owned() else {
            continue;
        };
        let Some(cancellation) = runtime.begin(&trigger.id) else {
            continue;
        };
        let handle = app.clone();
        let trigger_id = trigger.id.clone();
        let revision = runtime.revision.load(Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let Some(runtime) = handle.try_state::<AppEventRuntime>() else {
                return;
            };
            if runtime.is_stopped() || runtime.revision.load(Ordering::SeqCst) != revision {
                runtime.finish(&trigger_id);
                return;
            }
            let Some(db) = handle.try_state::<Db>() else {
                runtime.finish(&trigger_id);
                return;
            };
            let Some(integrations) = handle.try_state::<IntegrationsState>() else {
                runtime.finish(&trigger_id);
                return;
            };
            match integrations
                .sync_app_trigger(db.inner(), &trigger, cancellation)
                .await
            {
                Ok(report) => {
                    let _ = handle.emit(
                        "app-trigger://status",
                        serde_json::json!({
                            "triggerId": trigger_id,
                            "accepted": report.accepted,
                            "duplicates": report.duplicates,
                            "rejected": report.rejected,
                            "droppedOverrun": report.dropped_overrun,
                            "backpressured": report.backpressured,
                        }),
                    );
                }
                Err(error) => {
                    if error.code == AppEventErrorCode::Cancelled {
                        runtime.finish(&trigger_id);
                        return;
                    }
                    let retry_count = db
                        .get_app_trigger_state(&trigger_id)
                        .ok()
                        .flatten()
                        .map(|state| state.retry_count)
                        .unwrap_or(0);
                    let retry_at = next_retry_at(&error, retry_count);
                    let _ = db.mark_app_trigger_error(
                        &trigger_id,
                        error.code.as_str(),
                        Some(&retry_at),
                    );
                    let _ = handle.emit(
                        "app-trigger://status",
                        serde_json::json!({
                            "triggerId": trigger_id,
                            "errorCode": error.code.as_str(),
                            "nextAttemptAt": retry_at,
                        }),
                    );
                }
            }
            runtime.finish(&trigger_id);
        });
    }
    Ok(())
}

fn uses_socket_delivery(app: &AppHandle, trigger: &Trigger) -> bool {
    let Ok(config) = serde_json::from_value::<crate::integrations::events::AppTriggerConfig>(
        trigger.config.clone(),
    ) else {
        return false;
    };
    app.try_state::<IntegrationsState>()
        .and_then(|integrations| {
            integrations
                .events
                .descriptor(&config.provider_id, &config.event_type)
        })
        .is_some_and(|descriptor| {
            !descriptor
                .delivery_modes
                .contains(&AppEventDeliveryMode::Polling)
        })
}

fn is_due(db: &Db, app: &AppHandle, trigger: &Trigger) -> bool {
    let Ok(config) = serde_json::from_value::<crate::integrations::events::AppTriggerConfig>(
        trigger.config.clone(),
    ) else {
        return true;
    };
    let Some(integrations) = app.try_state::<IntegrationsState>() else {
        return false;
    };
    let Some(descriptor) = integrations
        .events
        .descriptor(&config.provider_id, &config.event_type)
    else {
        return true;
    };
    let state = db.get_app_trigger_state(&trigger.id).ok().flatten();
    if state
        .as_ref()
        .and_then(|item| item.next_attempt_at.as_deref())
        .is_some_and(is_future)
    {
        return false;
    }
    let interval = if descriptor
        .delivery_modes
        .contains(&AppEventDeliveryMode::Polling)
    {
        descriptor.poll_interval_seconds.max(5)
    } else {
        1
    };
    !state
        .and_then(|item| item.last_polled_at)
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|last| {
            Utc::now()
                .signed_duration_since(last.with_timezone(&Utc))
                .num_seconds()
                < interval as i64
        })
}

fn is_future(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc) > Utc::now())
        .unwrap_or(false)
}

fn drain_queue(app: &AppHandle, db: &Db) -> Result<(), String> {
    // Recover a run promoted just before a crash before promoting anything new.
    for pending in db
        .list_pending_app_event_runs()
        .map_err(|error| error.to_string())?
    {
        if !active::has_workflow(&pending.workflow_id) {
            let _ = runner::start_pending_app_event_run(app.clone(), db, &pending);
        }
    }
    for workflow_id in db
        .app_event_queue_workflow_ids()
        .map_err(|error| error.to_string())?
    {
        if active::has_workflow(&workflow_id) {
            continue;
        }
        if let Some(pending) = db
            .promote_next_app_event(&workflow_id)
            .map_err(|error| error.to_string())?
        {
            let _ = runner::start_pending_app_event_run(app.clone(), db, &pending);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, UpsertTriggerInput};
    use serde_json::json;

    #[test]
    fn retry_deadline_blocks_a_trigger_until_due() {
        assert!(is_future(
            &(Utc::now() + chrono::Duration::minutes(1)).to_rfc3339()
        ));
        assert!(!is_future(
            &(Utc::now() - chrono::Duration::minutes(1)).to_rfc3339()
        ));
    }

    #[test]
    fn pending_queue_survives_without_starting_a_second_run() {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Queue".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("workflow");
        let trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: workflow.id.clone(),
                source: "app".into(),
                label: String::new(),
                config: json!({}),
                enabled: true,
            })
            .expect("trigger");
        db.record_app_event(&trigger.id, "evt", "{}", true, 10)
            .expect("queue");
        let first = db
            .promote_next_app_event(&workflow.id)
            .expect("promote")
            .expect("pending");
        assert!(db
            .promote_next_app_event(&workflow.id)
            .expect("second promote")
            .is_none());
        assert_eq!(
            db.list_pending_app_event_runs().expect("pending"),
            vec![first]
        );
    }
}
