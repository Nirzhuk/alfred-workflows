//! Native macOS window chrome CSS cannot own.
//!
//! Wry's `trafficLightPosition.y` only sizes the title-bar container as
//! `buttonHeight + y`. It never sets the buttons' AppKit `origin.y`, so the
//! cluster stays at the default overlay inset (~28px title bar) and reads high
//! against Alfred's 44px bar. This module sizes the native chrome to that bar
//! and vertically centers the traffic lights on the title-bar text.

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::time::Duration;

use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
use tauri::{Runtime, WebviewWindow, Window};

/// Keep in sync with `--titlebar-height` in `src/App.css`.
const TITLEBAR_HEIGHT: f64 = 44.0;
/// Keep in sync with `trafficLightPosition.x` in `tauri.conf.json`.
const TRAFFIC_LIGHT_X: f64 = 16.0;

pub fn sync_main_window<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        position_traffic_lights(ns_window as *mut c_void);
    });
}

pub fn sync_event_window<R: Runtime>(window: &Window<R>) {
    if window.label() != "main" {
        return;
    }
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        position_traffic_lights(ns_window as *mut c_void);
    });
}

pub fn sync_main_window_after_layout<R: Runtime>(window: &WebviewWindow<R>) {
    sync_main_window(window);
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [16_u64, 80, 240] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            sync_main_window(&window);
        }
    });
}

unsafe fn position_traffic_lights(ns_window: *mut c_void) {
    if ns_window.is_null() {
        return;
    }

    // SAFETY: Tauri owns this NSWindow for the process lifetime. Callers hop
    // onto AppKit's main thread before entering here.
    let ns_window = &*ns_window.cast::<NSWindow>();
    let Some(close) = ns_window.standardWindowButton(NSWindowButton::CloseButton) else {
        return;
    };
    let Some(miniaturize) = ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton)
    else {
        return;
    };
    let zoom = ns_window.standardWindowButton(NSWindowButton::ZoomButton);
    let Some(titlebar_view) = close.superview() else {
        return;
    };
    let Some(container) = titlebar_view.superview() else {
        return;
    };

    let button_height = close.frame().size.height;
    let mut container_frame = NSView::frame(&container);
    container_frame.size.height = TITLEBAR_HEIGHT;
    container_frame.origin.y = ns_window.frame().size.height - TITLEBAR_HEIGHT;
    container.setFrame(container_frame);

    let mut titlebar_frame = NSView::frame(&titlebar_view);
    titlebar_frame.origin.x = 0.0;
    titlebar_frame.origin.y = 0.0;
    titlebar_frame.size.width = container_frame.size.width;
    titlebar_frame.size.height = TITLEBAR_HEIGHT;
    titlebar_view.setFrame(titlebar_frame);

    let space_between = miniaturize.frame().origin.x - close.frame().origin.x;
    let button_y = ((TITLEBAR_HEIGHT - button_height) / 2.0).max(0.0);
    let mut buttons = vec![close, miniaturize];
    if let Some(zoom) = zoom {
        buttons.push(zoom);
    }

    for (index, button) in buttons.into_iter().enumerate() {
        let mut rect = button.frame();
        rect.origin.x = TRAFFIC_LIGHT_X + (index as f64 * space_between);
        rect.origin.y = button_y;
        button.setFrameOrigin(rect.origin);
    }
}
