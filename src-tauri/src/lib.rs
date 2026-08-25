mod agents;
mod commands;
mod db;
// Provider-neutral seams are intentionally consumed by follow-on connector plans.
#[allow(dead_code)]
mod integrations;
mod licensing;
#[cfg(target_os = "macos")]
mod macos_titlebar;
#[cfg(target_os = "macos")]
mod native_window_material;
mod notifications;
mod quick_access;
mod runner;
mod scheduler;
mod skills;
mod tray;
mod triggers;

use db::Db;
use integrations::IntegrationsState;
use licensing::LicensingState;
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

#[tauri::command]
fn sync_macos_traffic_lights(window: tauri::Window) {
    #[cfg(target_os = "macos")]
    macos_titlebar::sync_event_window(&window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

pub fn run() {
    let database = Db::open().expect("failed to open sqlite database");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(database)
        .manage(IntegrationsState::default())
        .manage(LicensingState::default())
        .manage(notifications::NotificationPreferences::default())
        .manage(GlobalShortcutPreference::default())
        .manage(TriggerRuntime::default())
        .manage(AppEventRuntime::default());

    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::ShortcutState;
        use tauri_plugin_window_state::StateFlags;

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
                    .with_state_flags(
                        StateFlags::SIZE
                            | StateFlags::POSITION
                            | StateFlags::MAXIMIZED
                            | StateFlags::FULLSCREEN,
                    )
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
                    if let Err(error) = native_window_material::install(&window) {
                        eprintln!("native sidebar material could not be applied: {error}");
                    }
                    macos_titlebar::sync_main_window_after_layout(&window);
                }
            }

            // Blocks WhatsApp protocol logging before any client exists. The
            // crate prints raw identity and Signal key material through `log`,
            // including at WARN, so this must be installed first.
            if let Err(error) = integrations::whatsapp::log_guard::install_silent() {
                eprintln!("whatsapp log guard not installed: {error}");
            }

            let handle = app.handle().clone();

            // One WhatsApp runtime for the process lifetime, only when an
            // account is already linked. Never blocks startup.
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let (Some(db), Some(integrations)) = (
                        handle.try_state::<Db>(),
                        handle.try_state::<IntegrationsState>(),
                    ) else {
                        return;
                    };
                    integrations.whatsapp.start_stored_runtime(db.inner()).await;
                });
            }

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

            // Return the cached snapshot through `get_license_status` without
            // a startup network dependency, then refresh stale grants in the
            // background under the licensing single-flight lock.
            let license_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let Some(db) = license_handle.try_state::<Db>() else {
                    return;
                };
                let Some(licensing) = license_handle.try_state::<LicensingState>() else {
                    return;
                };
                if licensing.should_refresh(db.inner()) {
                    let _ = licensing.refresh(db.inner()).await;
                }
            });

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
            #[cfg(target_os = "macos")]
            match event {
                WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::ThemeChanged(_) => {
                    macos_titlebar::sync_event_window(window);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::activate_license,
            commands::refresh_license,
            commands::deactivate_license,
            commands::get_license_status,
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
            commands::integrations::prepare_github_connection,
            commands::integrations::poll_github_connection,
            commands::integrations::cancel_github_pairing,
            commands::integrations::prepare_gmail_connection,
            commands::integrations::complete_gmail_connection,
            commands::integrations::cancel_gmail_authorization,
            commands::integrations::prepare_microsoft_connection,
            commands::integrations::complete_microsoft_connection,
            commands::integrations::cancel_microsoft_authorization,
            commands::integrations::connect_notion_private,
            commands::integrations::connect_obsidian_vault,
            commands::integrations::connect_linear_private,
            commands::integrations::connect_sentry_private,
            commands::integrations::prepare_telegram_connection,
            commands::integrations::complete_telegram_connection,
            commands::integrations::cancel_telegram_pairing,
            commands::integrations::begin_whatsapp_pairing,
            commands::integrations::whatsapp_pairing_state,
            commands::integrations::send_whatsapp_pairing_test,
            commands::integrations::complete_whatsapp_pairing,
            commands::integrations::cancel_whatsapp_pairing,
            commands::integrations::whatsapp_runtime_status,
            commands::integrations::reconnect_whatsapp_runtime,
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
            quick_access::show_quick_access,
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
            sync_macos_traffic_lights,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS dock-icon click while the window is hidden sends Reopen,
            // not a fresh launch, so single-instance's re-show never fires.
            // Orderly exit: stop the WhatsApp client and flush protocol state.
            // Alfred sends nothing while it is not running.
            if let tauri::RunEvent::Exit = event {
                if let Some(integrations) = app_handle.try_state::<IntegrationsState>() {
                    tauri::async_runtime::block_on(integrations.whatsapp.shutdown_runtime());
                }
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
