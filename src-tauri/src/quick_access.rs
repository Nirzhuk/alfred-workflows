//! Always-available screen-edge access to workflows and schedules.
//!
//! The webview is tiny while idle, then stays anchored to the top-right while
//! the frontend asks it to expand. It is independent of the main window so
//! minimizing or closing the editor never pauses scheduled work.

use serde::Deserialize;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

pub const WINDOW_LABEL: &str = "quick-access";

const COLLAPSED_WIDTH: f64 = 24.0;
const COLLAPSED_HEIGHT: f64 = 72.0;
const COMPACT_WIDTH: f64 = 324.0;
const COMPACT_HEIGHT: f64 = 66.0;
const EXPANDED_WIDTH: f64 = 380.0;
const EXPANDED_HEIGHT: f64 = 540.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum QuickAccessLayout {
    Hover,
    Compact,
    Expanded,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct QuickAccessPosition {
    x: i32,
    y: i32,
}

fn collapsed_layout(mode: &str) -> Result<QuickAccessLayout, String> {
    match mode {
        "hover" => Ok(QuickAccessLayout::Hover),
        "compact" => Ok(QuickAccessLayout::Compact),
        _ => Err(format!("unknown quick access mode: {mode}")),
    }
}

fn physical_size(scale_factor: f64, layout: QuickAccessLayout) -> PhysicalSize<u32> {
    let (width, height) = match layout {
        QuickAccessLayout::Hover => (COLLAPSED_WIDTH, COLLAPSED_HEIGHT),
        QuickAccessLayout::Compact => (COMPACT_WIDTH, COMPACT_HEIGHT),
        QuickAccessLayout::Expanded => (EXPANDED_WIDTH, EXPANDED_HEIGHT),
    };
    PhysicalSize::new(
        (width * scale_factor).round() as u32,
        (height * scale_factor).round() as u32,
    )
}

fn monitor_for_position<R: Runtime>(
    window: &WebviewWindow<R>,
    position: Option<QuickAccessPosition>,
) -> tauri::Result<Monitor> {
    if let Some(position) = position {
        if let Some(monitor) = window.available_monitors()?.into_iter().find(|monitor| {
            let area = monitor.work_area();
            let right = area.position.x + area.size.width as i32;
            let bottom = area.position.y + area.size.height as i32;
            position.x >= area.position.x
                && position.x < right
                && position.y >= area.position.y
                && position.y < bottom
        }) {
            return Ok(monitor);
        }
    }

    window
        .current_monitor()?
        .or(window.primary_monitor()?)
        .ok_or(tauri::Error::WindowNotFound)
}

fn place_window<R: Runtime>(
    window: &WebviewWindow<R>,
    layout: QuickAccessLayout,
    position: Option<QuickAccessPosition>,
) -> tauri::Result<()> {
    let monitor = monitor_for_position(window, position)?;
    let work_area = monitor.work_area();
    let mut size = physical_size(monitor.scale_factor(), layout);
    size.width = size.width.min(work_area.size.width);
    size.height = size.height.min(work_area.size.height);

    let right_x = work_area.position.x + work_area.size.width as i32 - size.width as i32;
    let bottom_y = work_area.position.y + work_area.size.height as i32 - size.height as i32;
    let (x, y) = position
        .map(|position| {
            (
                position.x.clamp(work_area.position.x, right_x),
                position.y.clamp(work_area.position.y, bottom_y),
            )
        })
        .unwrap_or((right_x, work_area.position.y));
    window.set_size(size)?;
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_fullscreen_companion(window: &WebviewWindow, enabled: bool) -> tauri::Result<()> {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    let ns_window = window.ns_window()?;
    // SAFETY: Tauri owns this NSWindow for the lifetime of `window`, and this
    // setup code runs on the app thread immediately after window creation.
    unsafe {
        let ns_window: &NSWindow = &*ns_window.cast();
        let flags = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        let mut behavior = ns_window.collectionBehavior();
        if enabled {
            behavior |= flags;
        } else {
            behavior &= !flags;
        }
        ns_window.setAcceptsMouseMovedEvents(true);
        ns_window.setIgnoresMouseEvents(false);
        ns_window.setCollectionBehavior(behavior);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_always_on_top(window: &WebviewWindow, enabled: bool) -> tauri::Result<()> {
    use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow};

    window.set_always_on_top(enabled)?;
    let ns_window = window.ns_window()? as usize;
    // SAFETY: Tauri owns this NSWindow for the lifetime of `window`. Setting
    // the level on AppKit's main thread avoids stale levels after Spaces changes.
    window.run_on_main_thread(move || unsafe {
        let ns_window: &NSWindow = &*(ns_window as *mut std::ffi::c_void).cast();
        ns_window.setLevel(if enabled {
            NSFloatingWindowLevel
        } else {
            NSNormalWindowLevel
        });
    })
}

#[cfg(not(target_os = "macos"))]
fn configure_fullscreen_companion(_window: &WebviewWindow, _enabled: bool) -> tauri::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn configure_always_on_top(window: &WebviewWindow, enabled: bool) -> tauri::Result<()> {
    window.set_always_on_top(enabled)
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App("index.html?window=quick-access".into()),
    )
    .title("Alfred Quick Access")
    .inner_size(COLLAPSED_WIDTH, COLLAPSED_HEIGHT)
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .closable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(true)
    .accept_first_mouse(true)
    .build()?;

    place_window(&window, QuickAccessLayout::Hover, None)?;
    configure_fullscreen_companion(&window, true)?;
    configure_always_on_top(&window, true)?;
    Ok(())
}

pub(crate) fn show_expanded(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or(tauri::Error::WindowNotFound)?;
    let position = window
        .outer_position()
        .ok()
        .map(|position| QuickAccessPosition {
            x: position.x,
            y: position.y,
        });
    place_window(&window, QuickAccessLayout::Expanded, position)?;
    window.show()?;
    window.set_focus()?;
    window.emit("quick-access://open", ())?;
    Ok(())
}

#[tauri::command]
pub fn show_quick_access(app: AppHandle) -> Result<(), String> {
    show_expanded(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_expanded(
    app: AppHandle,
    expanded: bool,
    mode: String,
    position: Option<QuickAccessPosition>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "quick access window is unavailable".to_string())?;
    let layout = if expanded {
        QuickAccessLayout::Expanded
    } else {
        collapsed_layout(&mode)?
    };
    let position = if mode == "compact" { position } else { None };
    place_window(&window, layout, position).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_enabled(
    app: AppHandle,
    enabled: bool,
    mode: String,
    position: Option<QuickAccessPosition>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "quick access window is unavailable".to_string())?;

    if enabled {
        let layout = collapsed_layout(&mode)?;
        let position = if mode == "compact" { position } else { None };
        place_window(&window, layout, position).map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
    } else {
        let _ = window.emit("quick-access://reset", ());
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_quick_access_mode(
    app: AppHandle,
    mode: String,
    position: Option<QuickAccessPosition>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "quick access window is unavailable".to_string())?;
    let layout = collapsed_layout(&mode)?;
    let position = if mode == "compact" { position } else { None };
    place_window(&window, layout, position).map_err(|error| error.to_string())?;
    window
        .emit("quick-access://mode", mode)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_always_on_top(app: AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "quick access window is unavailable".to_string())?;
    configure_always_on_top(&window, enabled).map_err(|error| error.to_string())?;
    window
        .emit("quick-access://pin", enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_fullscreen(app: AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "quick access window is unavailable".to_string())?;
    window
        .set_visible_on_all_workspaces(enabled)
        .map_err(|error| error.to_string())?;
    configure_fullscreen_companion(&window, enabled).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_quick_access_target(
    app: AppHandle,
    target: String,
    workflow_id: Option<String>,
) -> Result<(), String> {
    crate::tray::show_main_window(&app);
    match target.as_str() {
        "app" => Ok(()),
        "schedules" => app
            .emit("app://open-schedules", ())
            .map_err(|error| error.to_string()),
        "workflow" => {
            let workflow_id = workflow_id
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| "workflow id is required".to_string())?;
            app.emit("app://open-workflow", workflow_id)
                .map_err(|error| error.to_string())
        }
        _ => Err(format!("unknown quick access target: {target}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_access_sizes_scale_for_retina_displays() {
        assert_eq!(
            physical_size(2.0, QuickAccessLayout::Hover),
            PhysicalSize::new(48, 144)
        );
        assert_eq!(
            physical_size(2.0, QuickAccessLayout::Compact),
            PhysicalSize::new(648, 132)
        );
        assert_eq!(
            physical_size(2.0, QuickAccessLayout::Expanded),
            PhysicalSize::new(760, 1080)
        );
    }
}
