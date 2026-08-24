import { describe, expect, test } from "bun:test";
import {
  applyDesktopPlatform,
  detectDesktopPlatform,
} from "../src/platform";

const root = new URL("../", import.meta.url);
const css = await Bun.file(new URL("src/App.css", root)).text();
const tauriConfig = await Bun.file(
  new URL("src-tauri/tauri.conf.json", root),
).json();
const tauriLib = await Bun.file(
  new URL("src-tauri/src/lib.rs", root),
).text();
const mainEntry = await Bun.file(new URL("src/main.tsx", root)).text();
const nativeMaterial = await Bun.file(
  new URL("src-tauri/src/native_window_material.rs", root),
).text();
const quickAccessNative = await Bun.file(
  new URL("src-tauri/src/quick_access.rs", root),
).text();

describe("desktop platform contract", () => {
  test("detects each supported desktop family from stable navigator signals", () => {
    expect(
      detectDesktopPlatform({
        platform: "MacIntel",
        userAgent: "Mozilla/5.0",
      }),
    ).toBe("macos");
    expect(
      detectDesktopPlatform({
        userAgentData: { platform: "Windows" },
        platform: "Win32",
      }),
    ).toBe("windows");
    expect(
      detectDesktopPlatform({
        platform: "Linux x86_64",
        userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      }),
    ).toBe("linux");
    expect(detectDesktopPlatform({})).toBe("unknown");
  });

  test("exposes one canonical platform attribute for CSS", () => {
    const rootElement = { dataset: {} as DOMStringMap };
    expect(
      applyDesktopPlatform(rootElement, {
        userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X)",
      }),
    ).toBe("macos");
    expect(rootElement.dataset.platform).toBe("macos");
  });

  test("reserves only the OS-owned title-bar safe area", () => {
    expect(css).toContain("--titlebar-safe-start: var(--space-3);");
    expect(css).toContain("--titlebar-macos-safe-start: 78px;");
    expect(css).toContain("--titlebar-height: 44px;");
    expect(css).toContain('html[data-platform="macos"]');
    expect(css).toContain(
      "padding: 0 var(--titlebar-safe-end) 0 var(--titlebar-safe-start);",
    );
    expect(css).not.toContain("--titlebar-pad-left");

    // Wry sizes the native title bar to buttonHeight + y and does not set the
    // buttons' vertical origin. y: 28 keeps that container at the 44px overlay
    // bar (16px controls); Rust then centers the cluster on the title-bar text.
    expect(tauriConfig.app.windows[0].trafficLightPosition).toEqual({
      x: 16,
      y: 28,
    });
  });

  test("normalizes application-owned controls and keeps fonts offline", () => {
    expect(css).toContain("select:not([multiple])");
    expect(css).toContain('input[type="checkbox"]::before');
    expect(css).toContain("appearance: none;");

    const csp = tauriConfig.app.security.csp as string;
    expect(csp).not.toContain("fonts.googleapis.com");
    expect(csp).not.toContain("fonts.gstatic.com");
    expect(csp).toContain("font-src 'self' data:");
  });

  test("uses one macOS wallpaper-tint layer beneath the sidebar and titlebar", () => {
    expect(tauriConfig.app.windows[0]?.transparent).toBe(true);
    expect(tauriLib).toContain("native_window_material::install(&window)");
    expect(nativeMaterial).toContain("window_vibrancy::{apply_vibrancy");
    expect(nativeMaterial).toContain("NSVisualEffectMaterial::HudWindow");
    expect(nativeMaterial).toContain("NSVisualEffectState::Active");
    expect(nativeMaterial).toContain("apply_vibrancy(");
    expect(nativeMaterial).not.toContain("NSVisualEffectMaterial::Titlebar");
    expect(nativeMaterial).not.toContain("NSVisualEffectMaterial::Sidebar");
    expect(css).toContain('html[data-platform="macos"] #root');
    expect(css).toContain("background-color: transparent;");
    expect(css).toContain(
      "background: color-mix(in srgb, var(--surface-panel) 60%, transparent);",
    );
    expect(css).toContain("background: var(--surface-panel-opaque);");
    expect(css).toMatch(
      /\.run-panel \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
    expect(css).toMatch(
      /\.canvas-toolbar \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
  });

  test("gives Quick Access the same rounded macOS material with opaque fallbacks", () => {
    expect(quickAccessNative).toContain(
      "crate::native_window_material::install_rounded(",
    );
    expect(quickAccessNative).toContain("QUICK_ACCESS_CORNER_RADIUS");
    expect(quickAccessNative).toContain(
      '.transparent(cfg!(target_os = "macos"))',
    );
    expect(nativeMaterial).toContain("install_rounded(window: &WebviewWindow, corner_radius: f64)");
    expect(nativeMaterial).toContain("apply_material(window, Some(corner_radius))");
    expect(css).toContain(
      'html[data-platform="macos"] .quick-access-compact,\nhtml[data-platform="macos"] .quick-access-panel',
    );
    expect(css).toMatch(
      /\.quick-access-compact \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
    expect(css).toMatch(
      /\.quick-access-panel \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
    expect(css).toContain(
      'html[data-platform="macos"][data-window="quick-access"]',
    );
    expect(css).toMatch(
      /\.quick-access-compact-run \{[\s\S]*?color: var\(--accent-ink\);/,
    );

    const reducedTransparency = css.lastIndexOf(
      "@media (prefers-reduced-transparency: reduce)",
    );
    const fallback = css.slice(reducedTransparency);
    expect(fallback).toContain(".quick-access-compact");
    expect(fallback).toContain(".quick-access-panel");
  });

  test("reveals the main window only after the first React commit", () => {
    expect(tauriConfig.app.windows[0]?.visible).toBe(false);
    expect(tauriLib).toContain("StateFlags::SIZE");
    expect(tauriLib).not.toContain("StateFlags::VISIBLE");
    expect(mainEntry).toContain("revealMainWindow");
    expect(mainEntry).toContain("getCurrentWindow().show()");
  });

  test("lets reduced transparency override native and CSS blur", () => {
    const reducedTransparency = css.lastIndexOf(
      "@media (prefers-reduced-transparency: reduce)",
    );
    expect(reducedTransparency).toBeGreaterThan(css.indexOf(".modal-backdrop,"));
    expect(reducedTransparency).toBeGreaterThan(
      css.indexOf('html[data-platform="macos"] .sidebar'),
    );
    const fallback = css.slice(reducedTransparency);
    expect(fallback).toContain('html[data-platform="macos"] .sidebar');
    expect(fallback).toContain(".memories-inspector-backdrop");
    expect(fallback).toContain("backdrop-filter: none;");
  });

  test("keeps the macOS-only reopen event out of other desktop builds", () => {
    expect(tauriLib).toMatch(
      /#\[cfg\(target_os = "macos"\)\][\s\S]*?RunEvent::Reopen/,
    );
  });
});
