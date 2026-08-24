use tauri::WebviewWindow;
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

/// Native frost behind the transparent webview. Opaque canvas and settings
/// surfaces mask it, leaving HudWindow vibrancy visible through Alfred's
/// sidebar and titlebar chrome.
pub fn install(window: &WebviewWindow) -> Result<(), String> {
    apply_material(window, None)
}

/// Same material for a borderless utility window, clipped to the window shape.
pub fn install_rounded(window: &WebviewWindow, corner_radius: f64) -> Result<(), String> {
    apply_material(window, Some(corner_radius))
}

fn apply_material(window: &WebviewWindow, corner_radius: Option<f64>) -> Result<(), String> {
    apply_vibrancy(
        window,
        NSVisualEffectMaterial::HudWindow,
        Some(NSVisualEffectState::Active),
        corner_radius,
    )
    .map_err(|error| error.to_string())
}
