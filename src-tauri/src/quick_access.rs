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
#[cfg(target_os = "macos")]
const QUICK_ACCESS_CORNER_RADIUS: f64 = 16.0;

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

fn initial_size(layout: QuickAccessLayout) -> (f64, f64) {
    match layout {
        QuickAccessLayout::Hover => (COLLAPSED_WIDTH, COLLAPSED_HEIGHT),
        QuickAccessLayout::Compact => (COMPACT_WIDTH, COMPACT_HEIGHT),
        QuickAccessLayout::Expanded => (EXPANDED_WIDTH, EXPANDED_HEIGHT),
    }
}

/// The popover boots from its URL, so a window built already expanded does not
/// need a `quick-access://open` event it could not yet be listening for.
fn initial_url(layout: QuickAccessLayout) -> &'static str {
    match layout {
        QuickAccessLayout::Expanded => "index.html?window=quick-access&expanded=1",
        _ => "index.html?window=quick-access",
    }
}

/// Build the popover webview at `layout`.
///
/// This window is a second WebContent process, so it is built on demand — when
/// the preference is switched on, or when the shortcut/tray asks for it — never
/// at startup. Idempotent: an existing window always wins.
fn install_layout(app: &AppHandle, layout: QuickAccessLayout) -> tauri::Result<()> {
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return Ok(());
    }

    let (width, height) = initial_size(layout);
    let window = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App(initial_url(layout).into()),
    )
    .title("Alfred Quick Access")
    .inner_size(width, height)
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .closable(false)
    .decorations(false)
    // Built hidden. `place_window` below runs AFTER `build()`, so a visible
    // builder paints the window at its default position with no rounded
    // material yet, then jumps to the screen edge — invisible when this ran at
    // startup, a flicker now that the window is built on first use. The callers
    // (`show_expanded`, `set_quick_access_enabled`) show it once it is placed.
    .visible(false)
    .transparent(cfg!(target_os = "macos"))
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(true)
    .accept_first_mouse(true)
    .build()?;

    #[cfg(target_os = "macos")]
    if let Err(error) =
        crate::native_window_material::install_rounded(&window, QUICK_ACCESS_CORNER_RADIUS)
    {
        eprintln!("native Quick Access material could not be applied: {error}");
    }

    place_window(&window, layout, None)?;
    configure_fullscreen_companion(&window, true)?;
    configure_always_on_top(&window, true)?;
    Ok(())
}

fn ensure_window(app: &AppHandle, layout: QuickAccessLayout) -> tauri::Result<WebviewWindow> {
    install_layout(app, layout)?;
    app.get_webview_window(WINDOW_LABEL)
        .ok_or(tauri::Error::WindowNotFound)
}

pub(crate) fn show_expanded(app: &AppHandle) -> tauri::Result<()> {
    // Safety net for the global shortcut and the tray item: the window is built
    // lazily now, so it can legitimately be missing — the preference may be off,
    // or the main window may never have booted to sync it.
    let existing = app.get_webview_window(WINDOW_LABEL);
    let was_running = existing.is_some();
    let window = match existing {
        Some(window) => window,
        None => ensure_window(app, QuickAccessLayout::Expanded)?,
    };
    // A window that just came up is already at the anchored default; only a
    // window the user has moved has a position worth honoring.
    let position = was_running
        .then(|| window.outer_position().ok())
        .flatten()
        .map(|position| QuickAccessPosition {
            x: position.x,
            y: position.y,
        });
    place_window(&window, QuickAccessLayout::Expanded, position)?;
    window.show()?;
    window.set_focus()?;
    if was_running {
        // A freshly built webview has no listeners yet, so this event would be
        // dropped; it boots expanded from `?expanded=1` instead.
        window.emit("quick-access://open", ())?;
    }
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
    // Only the popover itself calls this, so a missing window means it was torn
    // down mid-gesture. There is nothing left to resize and nothing to report.
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
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
    if !enabled {
        // Hiding left the webview's WebContent process resident for the whole
        // session, so a user who never wanted Quick Access still paid for it.
        // Destroying hands that memory back; switching it on rebuilds instead.
        // `destroy` rather than `close`, because the app-wide CloseRequested
        // handler prevents closes and merely hides the window.
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            return Ok(());
        };
        let _ = window.emit("quick-access://reset", ());
        return window.destroy().map_err(|error| error.to_string());
    }

    // Reject an unknown mode before building anything, so a bad call cannot
    // leave a half-configured window behind.
    let layout = collapsed_layout(&mode)?;
    let position = if mode == "compact" { position } else { None };
    let window = ensure_window(&app, layout).map_err(|error| error.to_string())?;
    place_window(&window, layout, position).map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_mode(
    app: AppHandle,
    mode: String,
    position: Option<QuickAccessPosition>,
) -> Result<(), String> {
    let layout = collapsed_layout(&mode)?;
    // The preference lives in the frontend and is pushed on boot before the
    // window may exist. Staying quiet is correct: `set_quick_access_enabled`
    // places the window for this mode when it builds it, and the popover reads
    // the mode from storage on mount, so nothing is lost by skipping the emit.
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    let position = if mode == "compact" { position } else { None };
    place_window(&window, layout, position).map_err(|error| error.to_string())?;
    window
        .emit("quick-access://mode", mode)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_always_on_top(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Quiet no-op while Quick Access is off. The frontend re-applies this
    // preference right after the window is built, so nothing drifts.
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    configure_always_on_top(&window, enabled).map_err(|error| error.to_string())?;
    window
        .emit("quick-access://pin", enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_quick_access_fullscreen(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Quiet no-op while Quick Access is off, for the same reason as the pin
    // above: there is no window to configure and the user asked for none.
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
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
    // Window-independent by construction: this only raises the main window and
    // routes an app event, so lazy creation cannot reach it. Left as-is.
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
    fn expanded_boot_carries_its_state_in_the_url() {
        // The shortcut can build this window from cold. A `quick-access://open`
        // emit would land before the webview has listeners, so the expanded
        // state has to travel in the URL the webview loads.
        assert_eq!(
            initial_url(QuickAccessLayout::Expanded),
            "index.html?window=quick-access&expanded=1"
        );
        for layout in [QuickAccessLayout::Hover, QuickAccessLayout::Compact] {
            assert_eq!(initial_url(layout), "index.html?window=quick-access");
        }
    }

    #[test]
    fn a_lazily_built_window_opens_at_its_final_size() {
        // Building at the collapsed size and resizing afterwards would flash a
        // 24x72 sliver before the popover appeared.
        assert_eq!(
            initial_size(QuickAccessLayout::Expanded),
            (EXPANDED_WIDTH, EXPANDED_HEIGHT)
        );
        assert_eq!(
            initial_size(QuickAccessLayout::Compact),
            (COMPACT_WIDTH, COMPACT_HEIGHT)
        );
        assert_eq!(
            initial_size(QuickAccessLayout::Hover),
            (COLLAPSED_WIDTH, COLLAPSED_HEIGHT)
        );
    }

    #[test]
    fn an_unknown_mode_is_rejected_before_a_window_is_built() {
        assert!(collapsed_layout("nonsense").is_err());
        assert_eq!(collapsed_layout("hover"), Ok(QuickAccessLayout::Hover));
        assert_eq!(collapsed_layout("compact"), Ok(QuickAccessLayout::Compact));
    }

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
