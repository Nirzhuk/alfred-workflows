import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

const QUICK_ACCESS_KEY = "alfred:quick-access-enabled";
const QUICK_ACCESS_FULLSCREEN_KEY = "alfred:quick-access-fullscreen";
const QUICK_ACCESS_MODE_KEY = "alfred:quick-access-mode";
const QUICK_ACCESS_ALWAYS_ON_TOP_KEY = "alfred:quick-access-always-on-top";
const QUICK_ACCESS_POSITION_KEY = "alfred:quick-access-position";

export type QuickAccessMode = "hover" | "compact";
export type QuickAccessPosition = { x: number; y: number };

type QuickAccessPreferences = {
  enabled: boolean;
  showInFullscreen: boolean;
  mode: QuickAccessMode;
  alwaysOnTop: boolean;
  busy: boolean;
  setEnabled: (enabled: boolean) => Promise<void>;
  setShowInFullscreen: (enabled: boolean) => Promise<void>;
  setMode: (mode: QuickAccessMode) => Promise<void>;
  setAlwaysOnTop: (enabled: boolean) => Promise<void>;
  resetPosition: () => Promise<void>;
};

export function readQuickAccessEnabled(): boolean {
  try {
    const value = localStorage.getItem(QUICK_ACCESS_KEY);
    return value === null ? true : value === "1";
  } catch {
    return true;
  }
}

export function readQuickAccessFullscreen(): boolean {
  try {
    const value = localStorage.getItem(QUICK_ACCESS_FULLSCREEN_KEY);
    return value === null ? true : value === "1";
  } catch {
    return true;
  }
}

export function readQuickAccessMode(): QuickAccessMode {
  try {
    return localStorage.getItem(QUICK_ACCESS_MODE_KEY) === "compact"
      ? "compact"
      : "hover";
  } catch {
    return "hover";
  }
}

export function readQuickAccessAlwaysOnTop(): boolean {
  try {
    const value = localStorage.getItem(QUICK_ACCESS_ALWAYS_ON_TOP_KEY);
    return value === null ? true : value === "1";
  } catch {
    return true;
  }
}

export function readQuickAccessPosition(): QuickAccessPosition | null {
  try {
    const value = localStorage.getItem(QUICK_ACCESS_POSITION_KEY);
    if (!value) return null;
    const parsed = JSON.parse(value) as Partial<QuickAccessPosition>;
    if (
      typeof parsed.x !== "number" ||
      !Number.isFinite(parsed.x) ||
      typeof parsed.y !== "number" ||
      !Number.isFinite(parsed.y)
    ) {
      return null;
    }
    return { x: Math.round(parsed.x), y: Math.round(parsed.y) };
  } catch {
    return null;
  }
}

function persistQuickAccessEnabled(enabled: boolean) {
  try {
    localStorage.setItem(QUICK_ACCESS_KEY, enabled ? "1" : "0");
  } catch {
    /* ignore */
  }
}

function persistQuickAccessFullscreen(enabled: boolean) {
  try {
    localStorage.setItem(QUICK_ACCESS_FULLSCREEN_KEY, enabled ? "1" : "0");
  } catch {
    /* ignore */
  }
}

function persistQuickAccessMode(mode: QuickAccessMode) {
  try {
    localStorage.setItem(QUICK_ACCESS_MODE_KEY, mode);
  } catch {
    /* ignore */
  }
}

function persistQuickAccessAlwaysOnTop(enabled: boolean) {
  try {
    localStorage.setItem(QUICK_ACCESS_ALWAYS_ON_TOP_KEY, enabled ? "1" : "0");
  } catch {
    /* ignore */
  }
}

export function saveQuickAccessPosition(position: QuickAccessPosition | null) {
  try {
    if (position) {
      localStorage.setItem(QUICK_ACCESS_POSITION_KEY, JSON.stringify(position));
    } else {
      localStorage.removeItem(QUICK_ACCESS_POSITION_KEY);
    }
  } catch {
    /* ignore */
  }
}

export async function syncQuickAccessPreference(): Promise<void> {
  const enabled = readQuickAccessEnabled();
  const showInFullscreen = readQuickAccessFullscreen();
  const mode = readQuickAccessMode();
  const alwaysOnTop = readQuickAccessAlwaysOnTop();
  const position = readQuickAccessPosition();
  useQuickAccessPreferences.setState({
    enabled,
    showInFullscreen,
    mode,
    alwaysOnTop,
  });
  try {
    await invoke("set_quick_access_mode", { mode, position });
    await Promise.all([
      invoke("set_quick_access_enabled", { enabled, mode, position }),
      invoke("set_quick_access_fullscreen", { enabled: showInFullscreen }),
      invoke("set_quick_access_always_on_top", { enabled: alwaysOnTop }),
    ]);
  } catch (error) {
    console.warn("Failed to sync screen-edge quick access", error);
  }
}

export const useQuickAccessPreferences = create<QuickAccessPreferences>(
  (set) => ({
    enabled: readQuickAccessEnabled(),
    showInFullscreen: readQuickAccessFullscreen(),
    mode: readQuickAccessMode(),
    alwaysOnTop: readQuickAccessAlwaysOnTop(),
    busy: false,
    setEnabled: async (enabled) => {
      set({ enabled, busy: true });
      persistQuickAccessEnabled(enabled);
      try {
        await invoke("set_quick_access_enabled", {
          enabled,
          mode: useQuickAccessPreferences.getState().mode,
          position: readQuickAccessPosition(),
        });
      } catch (error) {
        console.warn("Failed to update screen-edge quick access", error);
      } finally {
        set({ busy: false });
      }
    },
    setShowInFullscreen: async (enabled) => {
      set({ showInFullscreen: enabled, busy: true });
      persistQuickAccessFullscreen(enabled);
      try {
        await invoke("set_quick_access_fullscreen", { enabled });
      } catch (error) {
        console.warn("Failed to update full-screen quick access", error);
      } finally {
        set({ busy: false });
      }
    },
    setMode: async (mode) => {
      set({ mode, busy: true });
      persistQuickAccessMode(mode);
      try {
        await invoke("set_quick_access_mode", {
          mode,
          position: readQuickAccessPosition(),
        });
      } catch (error) {
        console.warn("Failed to update quick access mode", error);
      } finally {
        set({ busy: false });
      }
    },
    setAlwaysOnTop: async (enabled) => {
      set({ alwaysOnTop: enabled, busy: true });
      persistQuickAccessAlwaysOnTop(enabled);
      try {
        await invoke("set_quick_access_always_on_top", { enabled });
      } catch (error) {
        console.warn("Failed to update quick access pin", error);
      } finally {
        set({ busy: false });
      }
    },
    resetPosition: async () => {
      set({ busy: true });
      saveQuickAccessPosition(null);
      try {
        await invoke("set_quick_access_mode", {
          mode: useQuickAccessPreferences.getState().mode,
          position: null,
        });
      } catch (error) {
        console.warn("Failed to reset quick access position", error);
      } finally {
        set({ busy: false });
      }
    },
  }),
);
