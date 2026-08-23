use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::{ffi::c_void, ptr::NonNull};
use tauri::WebviewWindow;

const SIDEBAR_WIDTH: f64 = 280.0;
const TITLEBAR_HEIGHT: f64 = 44.0;

/// Builds Alfred's native macOS material as two explicit regions behind the
/// transparent webview. Opaque canvas and settings surfaces mask the native
/// views, so wallpaper tinting is visible only in application chrome.
pub fn install(window: &WebviewWindow) -> Result<(), String> {
    let view = NonNull::new(window.ns_view().map_err(|error| error.to_string())?)
        .ok_or_else(|| "main window has no native content view".to_string())?;
    let marker = MainThreadMarker::new()
        .ok_or_else(|| "native window material must be installed on the main thread".to_string())?;

    unsafe { install_regions(view, marker) };
    Ok(())
}

unsafe fn install_regions(view: NonNull<c_void>, marker: MainThreadMarker) {
    let content: &NSView = unsafe { view.cast().as_ref() };
    let bounds = content.bounds();
    let sidebar_width = SIDEBAR_WIDTH.min(bounds.size.width);
    let titlebar_height = TITLEBAR_HEIGHT.min(bounds.size.height);

    let sidebar_frame = NSRect::new(
        NSPoint::new(bounds.origin.x, bounds.origin.y),
        NSSize::new(sidebar_width, bounds.size.height),
    );
    let sidebar = NSVisualEffectView::initWithFrame(marker.alloc(), sidebar_frame);
    sidebar.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    sidebar.setMaterial(NSVisualEffectMaterial::Sidebar);
    sidebar.setState(NSVisualEffectState::Active);
    sidebar.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);

    let titlebar_frame = NSRect::new(
        NSPoint::new(
            bounds.origin.x + sidebar_width,
            bounds.origin.y + bounds.size.height - titlebar_height,
        ),
        NSSize::new(bounds.size.width - sidebar_width, titlebar_height),
    );
    let titlebar = NSVisualEffectView::initWithFrame(marker.alloc(), titlebar_frame);
    titlebar.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    titlebar.setMaterial(NSVisualEffectMaterial::Titlebar);
    titlebar.setState(NSVisualEffectState::Active);
    titlebar.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );

    content.addSubview_positioned_relativeTo(&sidebar, NSWindowOrderingMode::Below, None);
    content.addSubview_positioned_relativeTo(&titlebar, NSWindowOrderingMode::Below, None);
}
