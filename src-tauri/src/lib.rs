mod agents;
mod commands;
mod db;
mod notifications;
mod runner;
mod scheduler;
mod skills;
mod tray;
mod triggers;

use db::Db;
use std::time::Duration;
use tauri::{Emitter, Manager, WindowEvent};
use triggers::TriggerRuntime;

pub fn run() {
    let database = Db::open().expect("failed to open sqlite database");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(database)
        .manage(notifications::NotificationPreferences::default())
        .manage(TriggerRuntime::default());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            .plugin(tauri_plugin_window_state::Builder::default().build());
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
                    tray::refresh(&handle);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Agentflow running in the menu bar / tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
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
            notifications::set_notification_sound_cmd,
            notifications::notify_message_cmd,
            notifications::notify_run_finished_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
