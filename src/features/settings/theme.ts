import { getCurrentWindow } from "@tauri-apps/api/window";
import { create } from "zustand";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const THEME_KEY = "agentflow:theme";

type ThemeState = {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference: (preference: ThemePreference) => void;
};

export function getSystemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  return preference === "system" ? getSystemTheme() : preference;
}

export function readThemePreference(): ThemePreference {
  try {
    const raw = localStorage.getItem(THEME_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    /* ignore */
  }
  return "system";
}

function persistThemePreference(preference: ThemePreference) {
  try {
    localStorage.setItem(THEME_KEY, preference);
  } catch {
    /* ignore */
  }
}

export function applyResolvedTheme(resolved: ResolvedTheme) {
  document.documentElement.dataset.theme = resolved;
  document.documentElement.style.colorScheme = resolved;
}

async function syncNativeTheme(preference: ThemePreference) {
  try {
    // `null` follows the OS; light/dark force the window chrome.
    await getCurrentWindow().setTheme(
      preference === "system" ? null : preference,
    );
  } catch {
    /* ignore — webview-only or capability missing */
  }
}

export function enableThemeTransitions() {
  requestAnimationFrame(() => {
    document.documentElement.dataset.themeTransition = "";
  });
}

function applyPreference(preference: ThemePreference) {
  const resolved = resolveTheme(preference);
  applyResolvedTheme(resolved);
  void syncNativeTheme(preference);
  return resolved;
}

export const useThemeStore = create<ThemeState>((set) => ({
  preference: readThemePreference(),
  resolved: resolveTheme(readThemePreference()),
  setPreference: (preference) => {
    persistThemePreference(preference);
    const resolved = applyPreference(preference);
    set({ preference, resolved });
  },
}));

/** Apply stored preference before first paint / after store hydrate. */
export function bootstrapTheme() {
  const preference = readThemePreference();
  const resolved = applyPreference(preference);
  useThemeStore.setState({ preference, resolved });
}

/** Keep `system` in sync with OS appearance changes. */
export function installThemeListeners() {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const onSystemChange = () => {
    const { preference } = useThemeStore.getState();
    if (preference !== "system") return;
    const resolved = applyPreference("system");
    useThemeStore.setState({ resolved });
  };

  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onSystemChange);
  } else {
    media.addListener(onSystemChange);
  }

  let unlistenNative: (() => void) | undefined;
  void getCurrentWindow()
    .onThemeChanged(({ payload }) => {
      const { preference } = useThemeStore.getState();
      if (preference !== "system") return;
      applyResolvedTheme(payload);
      useThemeStore.setState({ resolved: payload });
    })
    .then((unlisten) => {
      unlistenNative = unlisten;
    })
    .catch(() => {
      /* ignore */
    });

  return () => {
    if (typeof media.removeEventListener === "function") {
      media.removeEventListener("change", onSystemChange);
    } else {
      media.removeListener(onSystemChange);
    }
    unlistenNative?.();
  };
}
