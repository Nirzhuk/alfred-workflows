//! Schedule triggers for workflows.
//!
//! Workflows are always runnable manually. Schedules enqueue the same runner
//! with `trigger_kind = schedule` when due. The ticker only runs while the app
//! is open.

use crate::db::{parse_rfc3339, Db, UpsertScheduleInput};
use crate::runner::{start_run, RunTrigger};
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use tauri::AppHandle;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("{0}")]
    Message(String),
}

/// Validate a cron expression and compute the next fire time from now.
///
/// Expects a 6-field cron: `sec min hour day month dow`
/// Example: `0 0 9 * * 1-5` = weekdays at 09:00.
pub fn next_run_from_cron(
    cron: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, SchedulerError> {
    let schedule = Schedule::from_str(cron.trim())
        .map_err(|e| SchedulerError::Message(format!("invalid cron `{cron}`: {e}")))?;

    schedule
        .after(&after)
        .next()
        .ok_or_else(|| SchedulerError::Message(format!("cron `{cron}` has no upcoming time")))
}

pub fn upsert_schedule(
    db: &Db,
    workflow_id: String,
    cron: String,
    enabled: bool,
) -> Result<crate::db::Schedule, SchedulerError> {
    let next_run_at = if enabled {
        Some(next_run_from_cron(&cron, Utc::now())?.to_rfc3339())
    } else {
        None
    };

    db.upsert_schedule(
        UpsertScheduleInput {
            workflow_id,
            cron,
            enabled,
        },
        next_run_at,
    )
    .map_err(|e| SchedulerError::Message(e.to_string()))
}

/// Check due schedules and start runs. Returns how many runs were started.
pub fn tick(app: &AppHandle, db: &Db) -> Result<usize, SchedulerError> {
    let now = Utc::now();
    let schedules = db
        .list_enabled_schedules()
        .map_err(|e| SchedulerError::Message(e.to_string()))?;

    let mut fired = 0usize;

    for schedule in schedules {
        let due = match schedule.next_run_at.as_deref().and_then(parse_rfc3339) {
            Some(next) => next <= now,
            // Missing next_run_at: compute and persist, don't fire this tick.
            None => {
                let next = next_run_from_cron(&schedule.cron, now)?;
                db.set_schedule_next_run(&schedule.id, Some(next.to_rfc3339()))
                    .map_err(|e| SchedulerError::Message(e.to_string()))?;
                false
            }
        };

        if !due {
            continue;
        }

        // start_run, not enqueue_run: enqueue only writes a pending row, so
        // scheduled runs used to sit in the table and never execute.
        start_run(
            app.clone(),
            db,
            &schedule.workflow_id,
            RunTrigger::Schedule,
            None,
        )
        .map_err(|e| SchedulerError::Message(e.to_string()))?;

        let next = next_run_from_cron(&schedule.cron, now)?;
        db.set_schedule_next_run(&schedule.id, Some(next.to_rfc3339()))
            .map_err(|e| SchedulerError::Message(e.to_string()))?;

        fired += 1;
    }

    Ok(fired)
}
