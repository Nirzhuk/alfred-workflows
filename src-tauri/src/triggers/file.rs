//! File-change triggers.
//!
//! Config shape (`triggers.config_json`):
//! ```json
//! { "path": "/abs/path", "pattern": "*.rs,*.ts", "debounceMs": 2000 }
//! ```
//! `pattern` empty means "any file". Editors and build tools touch files in
//! bursts, so a per-trigger cooldown collapses a burst into one run.

use super::TriggerRuntime;
use crate::db::Db;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEFAULT_DEBOUNCE_MS: u64 = 2_000;

/// Directories whose churn is noise, not intent. Without this a single `git
/// commit` or `cargo build` fires dozens of runs.
const IGNORED_DIRS: [&str; 7] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
];

#[derive(Clone)]
struct FileConfig {
    path: PathBuf,
    patterns: Vec<String>,
    debounce: Duration,
}

fn parse_config(config: &Value) -> Result<FileConfig, String> {
    let path = config
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("file trigger needs a `path`")?;

    let patterns = config
        .get("pattern")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    let debounce_ms = config
        .get("debounceMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_DEBOUNCE_MS);

    Ok(FileConfig {
        path: PathBuf::from(path),
        patterns,
        debounce: Duration::from_millis(debounce_ms.max(100)),
    })
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        let part = c.as_os_str().to_string_lossy();
        IGNORED_DIRS.contains(&part.as_ref())
    })
}

/// `*.rs` matches by extension; anything else is a substring match on the path.
fn matches_pattern(path: &Path, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let full = path.to_string_lossy();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    patterns
        .iter()
        .any(|pattern| match pattern.strip_prefix('*') {
            Some(suffix) => name.ends_with(suffix),
            None => full.contains(pattern.as_str()),
        })
}

fn watch_trigger(
    app: &AppHandle,
    trigger_id: &str,
    config: &Value,
) -> Result<RecommendedWatcher, String> {
    let config = parse_config(config)?;
    if !config.path.exists() {
        return Err(format!("path does not exist: {}", config.path.display()));
    }

    let app = app.clone();
    let trigger_id = trigger_id.to_string();
    let matcher = config.clone();
    let last_fired: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            return;
        }

        let Some(changed) = event
            .paths
            .iter()
            .find(|p| !is_ignored(p) && matches_pattern(p, &matcher.patterns))
        else {
            return;
        };

        // Cooldown: one run per burst.
        {
            let Ok(mut last) = last_fired.lock() else {
                return;
            };
            let now = Instant::now();
            if last.is_some_and(|prev| now.duration_since(prev) < matcher.debounce) {
                return;
            }
            *last = Some(now);
        }

        let Some(db) = app.try_state::<Db>() else {
            return;
        };
        let Ok(Some(trigger)) = db.get_trigger(&trigger_id) else {
            return;
        };
        if !trigger.enabled {
            return;
        }

        let payload = serde_json::json!({
            "source": "file",
            "triggerId": trigger.id,
            "path": changed.to_string_lossy(),
            "event": format!("{:?}", event.kind),
            "watchedPath": matcher.path.to_string_lossy(),
        });

        if let Err(e) = super::fire(&app, db.inner(), &trigger, payload) {
            eprintln!("file trigger {trigger_id} failed to start run: {e}");
        }
    })
    .map_err(|e| e.to_string())?;

    watcher
        .watch(&config.path, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    Ok(watcher)
}

/// Rebuild every file watcher from the DB. Cheap enough to call on any change.
pub fn reload_watchers(app: &AppHandle) -> Result<usize, String> {
    let db = app
        .try_state::<Db>()
        .ok_or_else(|| "database unavailable".to_string())?;
    let triggers = db
        .list_enabled_triggers(Some("file"))
        .map_err(|e| e.to_string())?;

    let mut watchers = Vec::new();
    for trigger in triggers {
        match watch_trigger(app, &trigger.id, &trigger.config) {
            Ok(watcher) => watchers.push(watcher),
            // A bad path shouldn't take down the other watchers.
            Err(e) => eprintln!("file trigger {} skipped: {e}", trigger.id),
        }
    }

    let count = watchers.len();
    app.state::<TriggerRuntime>().replace_watchers(watchers);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_and_ignore_rules() {
        let rs = Path::new("/repo/src/main.rs");
        let ts = Path::new("/repo/src/app.ts");
        let git = Path::new("/repo/.git/index");

        assert!(matches_pattern(rs, &[]));
        assert!(matches_pattern(rs, &["*.rs".into()]));
        assert!(!matches_pattern(ts, &["*.rs".into()]));
        assert!(matches_pattern(ts, &["*.rs".into(), "*.ts".into()]));
        assert!(matches_pattern(rs, &["src/".into()]));

        assert!(is_ignored(git));
        assert!(!is_ignored(rs));
        assert!(is_ignored(Path::new("/repo/node_modules/x/index.js")));
    }

    #[test]
    fn config_defaults_and_errors() {
        let cfg = parse_config(&serde_json::json!({ "path": "/tmp" })).unwrap();
        assert_eq!(cfg.debounce, Duration::from_millis(DEFAULT_DEBOUNCE_MS));
        assert!(cfg.patterns.is_empty());

        assert!(parse_config(&serde_json::json!({})).is_err());
        assert!(parse_config(&serde_json::json!({ "path": "  " })).is_err());
    }
}
