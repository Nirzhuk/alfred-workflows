import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { create } from "zustand";

const ENABLED_KEY = "alfred:notifications-enabled";
const SOUND_KEY = "alfred:notification-sound";

export const NOTIFICATION_SOUND_OPTIONS = [
  { value: "system", label: "System default" },
  { value: "chime", label: "Chime" },
  { value: "ping", label: "Ping" },
  { value: "pop", label: "Pop" },
  { value: "none", label: "None" },
] as const;

export type NotificationSound =
  (typeof NOTIFICATION_SOUND_OPTIONS)[number]["value"];

export type NotificationPermissionStatus = "unknown" | "granted" | "denied";

export type OpenRunOutputPayload = {
  workflowId: string;
  title: string;
  body: string;
  ok: boolean;
};

type NotificationsState = {
  enabled: boolean;
  sound: NotificationSound;
  permission: NotificationPermissionStatus;
  busy: boolean;
  refreshPermission: () => Promise<NotificationPermissionStatus>;
  setEnabled: (enabled: boolean) => Promise<void>;
  setSound: (sound: NotificationSound) => Promise<void>;
  openSystemSettings: () => Promise<void>;
  sendTest: () => Promise<boolean>;
};

export function readNotificationsEnabled(): boolean {
  try {
    const raw = localStorage.getItem(ENABLED_KEY);
    if (raw === null) return true;
    return raw === "1";
  } catch {
    return true;
  }
}

function persistNotificationsEnabled(enabled: boolean) {
  try {
    localStorage.setItem(ENABLED_KEY, enabled ? "1" : "0");
  } catch {
    /* ignore */
  }
}

export function readNotificationSound(): NotificationSound {
  try {
    const raw = localStorage.getItem(SOUND_KEY);
    return NOTIFICATION_SOUND_OPTIONS.some((option) => option.value === raw)
      ? (raw as NotificationSound)
      : "system";
  } catch {
    return "system";
  }
}

function persistNotificationSound(sound: NotificationSound) {
  try {
    localStorage.setItem(SOUND_KEY, sound);
  } catch {
    /* ignore */
  }
}

export function isMacPlatform(): boolean {
  return /Mac|Macintosh/i.test(navigator.userAgent);
}

async function readPermission(): Promise<NotificationPermissionStatus> {
  try {
    return (await isPermissionGranted()) ? "granted" : "denied";
  } catch {
    return "unknown";
  }
}

async function ensureGranted(): Promise<NotificationPermissionStatus> {
  try {
    if (await isPermissionGranted()) return "granted";
    const result = await requestPermission();
    return result === "granted" ? "granted" : "denied";
  } catch {
    return "unknown";
  }
}

async function syncNativeSound(sound: NotificationSound): Promise<void> {
  try {
    await invoke("set_notification_sound_cmd", { sound });
  } catch (err) {
    console.warn("Failed to sync notification sound", err);
  }
}

async function pushSimpleNotification(title: string, body: string) {
  try {
    await invoke("notify_message_cmd", { title, body });
  } catch (err) {
    console.warn("Failed to send notification", err);
  }
}

/**
 * True when the Alfred window is not the frontmost visible window.
 * Prefer Tauri window state — `document.hasFocus()` is unreliable in WKWebView.
 */
export async function shouldNotifyAboutRun(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    const [focused, visible] = await Promise.all([
      win.isFocused(),
      win.isVisible(),
    ]);
    return !focused || !visible;
  } catch {
    return document.hidden || !document.hasFocus();
  }
}

export const useNotificationsStore = create<NotificationsState>((set, get) => ({
  enabled: readNotificationsEnabled(),
  sound: readNotificationSound(),
  permission: "unknown",
  busy: false,

  refreshPermission: async () => {
    const permission = await readPermission();
    set({ permission });
    return permission;
  },

  setEnabled: async (enabled) => {
    set({ busy: true });
    try {
      persistNotificationsEnabled(enabled);
      set({ enabled });

      if (!enabled) return;

      const permission = await ensureGranted();
      set({ permission });
      if (permission === "granted") {
        await pushSimpleNotification(
          "Notifications on",
          "Alfred will notify you when a background run finishes.",
        );
      }
    } finally {
      set({ busy: false });
    }
  },

  setSound: async (sound) => {
    persistNotificationSound(sound);
    set({ sound });
    await syncNativeSound(sound);
  },

  openSystemSettings: async () => {
    if (!isMacPlatform()) return;
    try {
      await openUrl(
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
      );
    } catch {
      try {
        await openUrl(
          "x-apple.systempreferences:com.apple.preference.notifications",
        );
      } catch {
        /* ignore */
      }
    }
  },

  sendTest: async () => {
    const { enabled } = get();
    if (!enabled) return false;
    const permission = await ensureGranted();
    set({ permission });
    if (permission !== "granted") return false;
    await pushSimpleNotification(
      "Test notification",
      "Alfred notifications are working.",
    );
    return true;
  },
}));

/** Startup: refresh permission; only prompt if the user wants notifications. */
export async function prepareNotifications(): Promise<void> {
  const enabled = readNotificationsEnabled();
  const sound = readNotificationSound();
  useNotificationsStore.setState({ enabled, sound });
  await syncNativeSound(sound);
  if (!enabled) {
    const permission = await readPermission();
    useNotificationsStore.setState({ permission });
    return;
  }
  const permission = await ensureGranted();
  useNotificationsStore.setState({ permission });
}

/** Clickable run notification (Rust) — opens output when the user clicks it. */
export async function notifyRunFinished(input: {
  workflowId: string;
  workflowName: string;
  ok: boolean;
  title: string;
  body: string;
}): Promise<void> {
  if (!readNotificationsEnabled()) return;

  const permission = await ensureGranted();
  useNotificationsStore.setState({ permission });
  if (permission !== "granted") {
    console.warn("Skipping run notification — permission not granted");
    return;
  }

  try {
    await invoke("notify_run_finished_cmd", {
      workflowId: input.workflowId,
      workflowName: input.workflowName,
      ok: input.ok,
      title: input.title,
      body: input.body,
    });
  } catch (err) {
    console.warn("Failed to send run notification", err);
    await pushSimpleNotification(
      input.ok ? "Automation finished" : "Automation failed",
      `${input.workflowName}: ${input.title}`,
    );
  }
}
