use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
};
use objc2_foundation::MainThreadMarker;
use std::{ffi::c_void, ptr::NonNull};
use tauri::WebviewWindow;

const MATERIAL_ALPHA: f64 = 0.90;

/// Builds one native macOS wallpaper-tint layer behind the transparent
/// webview. Opaque canvas and settings surfaces mask it, leaving the shared
/// material visible through Alfred's sidebar and titlebar chrome.
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
    let material_frame = bounds;
    let material = NSVisualEffectView::initWithFrame(marker.alloc(), material_frame);
    material.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    material.setMaterial(NSVisualEffectMaterial::Sidebar);
    material.setState(NSVisualEffectState::Active);
    material.setAlphaValue(MATERIAL_ALPHA);
    material.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );

    content.addSubview_positioned_relativeTo(&material, NSWindowOrderingMode::Below, None);
}
