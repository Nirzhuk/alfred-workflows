mod agents;
mod commands;
mod db;
// Provider-neutral seams are intentionally consumed by follow-on connector plans.
#[allow(dead_code)]
mod integrations;
mod notifications;
mod quick_access;
mod runner;
mod scheduler;
mod skills;
mod tray;
mod triggers;

use db::Db;
use integrations::IntegrationsState;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager, State, WindowEvent};
use triggers::app::AppEventRuntime;
use triggers::TriggerRuntime;

#[cfg(desktop)]
const QUICK_ACCESS_SHORTCUT: &str = "CmdOrCtrl+Shift+Space";

struct GlobalShortcutPreference(Mutex<String>);

impl Default for GlobalShortcutPreference {
    fn default() -> Self {
        Self(Mutex::new(QUICK_ACCESS_SHORTCUT.to_string()))
    }
}

#[tauri::command]
fn set_global_shortcut(
    app: tauri::AppHandle,
    preference: State<'_, GlobalShortcutPreference>,
    shortcut: String,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let shortcut = shortcut.trim();
        if shortcut.is_empty() {
            return Err("Shortcut cannot be empty".to_string());
        }

        let mut current = preference
            .0
            .lock()
            .map_err(|_| "Global shortcut preference is unavailable".to_string())?;
        if current.as_str() == shortcut {
            if app.global_shortcut().is_registered(shortcut) {
                return Ok(());
            }
            return app
                .global_shortcut()
                .register(shortcut)
                .map_err(|error| error.to_string());
        }

        // Register first so a rejected OS-level shortcut never leaves the user
        // without their previous working binding.
        app.global_shortcut()
            .register(shortcut)
            .map_err(|error| error.to_string())?;

        if let Err(error) = app.global_shortcut().unregister(current.as_str()) {
            let _ = app.global_shortcut().unregister(shortcut);
            return Err(error.to_string());
        }

        *current = shortcut.to_string();
        Ok(())
    }

    #[cfg(not(desktop))]
    {
        let _ = (app, preference, shortcut);
        Err("Global shortcuts are only available on desktop".to_string())
    }
}

pub fn run() {
    let database = Db::open().expect("failed to open sqlite database");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(database)
        .manage(IntegrationsState::default())
        .manage(notifications::NotificationPreferences::default())
        .manage(GlobalShortcutPreference::default())
        .manage(TriggerRuntime::default())
        .manage(AppEventRuntime::default());

    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::ShortcutState;

        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            if let Err(error) = quick_access::show_expanded(app) {
                                eprintln!("global shortcut could not open quick access: {error}");
                            }
                        }
                    })
                    .build(),
            )
            .plugin(
                tauri_plugin_window_state::Builder::default()
                    .with_denylist(&[quick_access::WINDOW_LABEL])
                    .build(),
            );
    }

    builder
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                notifications::prepare_notification_identity(app.handle());
                if let Some(window) = app.get_webview_window("main") {
                    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                    let _ = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::UnderWindowBackground,
                        None,
                        None,
                    );
                }
            }

            let handle = app.handle().clone();

            // Webhook listener + file watchers. Neither is fatal: manual and
            // scheduled runs still work if a trigger source fails to start.
            match triggers::http::start(handle.clone()) {
                Ok(port) => handle.state::<TriggerRuntime>().set_http_port(port),
                Err(e) => eprintln!("webhook listener not started: {e}"),
            }
            if let Err(e) = triggers::reload(&handle) {
                eprintln!("file triggers not started: {e}");
            }

            if let Err(e) = tray::install(app.handle()) {
                eprintln!("tray icon not started: {e}");
            }
            if let Err(e) = quick_access::install(app.handle()) {
                eprintln!("quick access not started: {e}");
            }

            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;

                let shortcut = app
                    .state::<GlobalShortcutPreference>()
                    .0
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_else(|_| QUICK_ACCESS_SHORTCUT.to_string());
                if let Err(error) = app.global_shortcut().register(shortcut.as_str()) {
                    eprintln!("global quick-access shortcut {shortcut} is unavailable: {error}");
                }
            }

            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(20));
                loop {
                    ticker.tick().await;
                    let Some(db) = handle.try_state::<Db>() else {
                        continue;
                    };
                    match scheduler::tick(&handle, db.inner()) {
                        Ok(n) if n > 0 => {
                            let _ = handle.emit("scheduler://fired", n);
                        }
                        Err(e) => {
                            eprintln!("scheduler tick error: {e}");
                        }
                        _ => {}
                    }
                    if let Some(integrations) = handle.try_state::<IntegrationsState>() {
                        integrations
                            .refresh
                            .scheduled_health_check(db.inner())
                            .await;
                    }
                    tray::refresh(&handle);
                }
            });

            let app_event_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(1));
                loop {
                    ticker.tick().await;
                    if app_event_handle
                        .try_state::<AppEventRuntime>()
                        .is_some_and(|runtime| runtime.is_stopped())
                    {
                        break;
                    }
                    if let Err(error) = triggers::app::tick(&app_event_handle).await {
                        eprintln!("app event runtime tick error: {error}");
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Alfred running in the menu bar / tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::integrations::list_app_providers,
            commands::integrations::list_app_action_descriptors,
            commands::integrations::list_app_action_resources,
            commands::integrations::list_app_event_descriptors,
            commands::integrations::list_app_event_resources,
            commands::integrations::list_app_connections,
            commands::integrations::get_app_connection,
            commands::integrations::get_app_connection_usage,
            commands::integrations::disconnect_app_connection,
            commands::integrations::connect_slack_private,
            commands::list_workflows,
            commands::get_workflow,
            commands::create_workflow,
            commands::update_workflow,
            commands::delete_workflow,
            commands::reorder_workflows,
            commands::list_workflow_folders,
            commands::create_workflow_folder,
            commands::rename_workflow_folder,
            commands::delete_workflow_folder,
            commands::reorder_workflow_folders,
            commands::move_workflow_to_folder,
            commands::list_agent_providers,
            commands::list_agent_models,
            commands::get_agent_usage,
            commands::list_skills,
            commands::run_workflow,
            commands::cancel_run,
            commands::list_active_runs,
            commands::list_schedules,
            commands::get_workflow_schedule,
            commands::upsert_workflow_schedule,
            commands::delete_workflow_schedule,
            commands::list_workflow_triggers,
            commands::list_app_trigger_statuses,
            commands::upsert_workflow_trigger,
            commands::delete_workflow_trigger,
            commands::test_workflow_trigger,
            commands::webhook_base_url,
            commands::list_memories,
            commands::list_linkable_memories,
            commands::link_memory,
            commands::unlink_memory,
            commands::create_memory,
            commands::update_memory,
            commands::delete_memory,
            commands::clear_memories,
            quick_access::set_quick_access_expanded,
            quick_access::set_quick_access_enabled,
            quick_access::set_quick_access_mode,
            quick_access::set_quick_access_always_on_top,
            quick_access::set_quick_access_fullscreen,
            quick_access::open_quick_access_target,
            notifications::set_notification_sound_cmd,
            notifications::notify_message_cmd,
            notifications::notify_run_finished_cmd,
            set_global_shortcut,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
