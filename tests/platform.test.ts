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
    expect(css).toContain('html[data-platform="macos"]');
    expect(css).toContain(
      "padding: 0 var(--titlebar-safe-end) 0 var(--titlebar-safe-start);",
    );
    expect(css).not.toContain("--titlebar-pad-left");
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
    expect(nativeMaterial).toContain("NSVisualEffectMaterial::Sidebar");
    expect(nativeMaterial).toContain("NSVisualEffectState::Active");
    expect(nativeMaterial).toContain("const MATERIAL_ALPHA: f64 = 0.82;");
    expect(nativeMaterial).toContain("material.setAlphaValue(MATERIAL_ALPHA);");
    expect(nativeMaterial).toContain("let material_frame = bounds;");
    expect(nativeMaterial).toContain("NSAutoresizingMaskOptions::ViewWidthSizable");
    expect(nativeMaterial).toContain("NSAutoresizingMaskOptions::ViewHeightSizable");
    expect(nativeMaterial).not.toContain("NSVisualEffectMaterial::Titlebar");
    expect(css).toContain('html[data-platform="macos"] #root');
    expect(css).toContain("background-color: transparent;");
    expect(css).toContain(
      "background: color-mix(in srgb, var(--surface-panel) 22%, transparent);",
    );
    expect(css).toContain("background: var(--surface-panel-opaque);");
    expect(css).toMatch(
      /\.run-panel \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
    expect(css).toMatch(
      /\.canvas-toolbar \{[\s\S]*?background: var\(--surface-panel-opaque\);/,
    );
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
