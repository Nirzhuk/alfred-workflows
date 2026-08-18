import { useEffect } from "react";
import { SelectControl } from "../../../../components/select-control";
import { ConnectedAppsSettings } from "../../../integrations/connected-apps-settings";
import {
  showQuickAccess,
  useQuickAccessPreferences,
} from "../../../quick-access/preferences";
import { ShortcutSettings } from "../shortcut-settings";
import {
  SETTINGS_SECTION_LABELS,
  type SettingsSectionId,
} from "../settings-sidebar";
import {
  isMacPlatform,
  NOTIFICATION_SOUND_OPTIONS,
  type NotificationSound,
  useNotificationsStore,
} from "../../notifications";
import {
  useThemeStore,
  type ThemePreference,
} from "../../theme";

type Props = {
  activeSection: SettingsSectionId;
};

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsPage({ activeSection }: Props) {
  const preference = useThemeStore((s) => s.preference);
  const setPreference = useThemeStore((s) => s.setPreference);
  const quickAccessEnabled = useQuickAccessPreferences((s) => s.enabled);
  const quickAccessFullscreen = useQuickAccessPreferences(
    (s) => s.showInFullscreen,
  );
  const quickAccessMode = useQuickAccessPreferences((s) => s.mode);
  const quickAccessAlwaysOnTop = useQuickAccessPreferences(
    (s) => s.alwaysOnTop,
  );
  const quickAccessBusy = useQuickAccessPreferences((s) => s.busy);
  const setQuickAccessEnabled = useQuickAccessPreferences((s) => s.setEnabled);
  const setQuickAccessMode = useQuickAccessPreferences((s) => s.setMode);
  const setQuickAccessFullscreen = useQuickAccessPreferences(
    (s) => s.setShowInFullscreen,
  );
  const setQuickAccessAlwaysOnTop = useQuickAccessPreferences(
    (s) => s.setAlwaysOnTop,
  );
  const resetQuickAccessPosition = useQuickAccessPreferences(
    (s) => s.resetPosition,
  );

  const notificationsEnabled = useNotificationsStore((s) => s.enabled);
  const notificationSound = useNotificationsStore((s) => s.sound);
  const permission = useNotificationsStore((s) => s.permission);
  const busy = useNotificationsStore((s) => s.busy);
  const setNotificationsEnabled = useNotificationsStore((s) => s.setEnabled);
  const setNotificationSound = useNotificationsStore((s) => s.setSound);
  const refreshPermission = useNotificationsStore((s) => s.refreshPermission);
  const openSystemSettings = useNotificationsStore((s) => s.openSystemSettings);
  const sendTest = useNotificationsStore((s) => s.sendTest);

  useEffect(() => {
    void refreshPermission();
    const onFocus = () => {
      void refreshPermission();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshPermission]);

  const permissionLabel =
    permission === "granted"
      ? "Allowed"
      : permission === "denied"
        ? "Not allowed"
        : "Checking…";

  return (
    <section className="settings-page" aria-label="Settings">
      <header className="settings-page-header">
        <div>
          <p className="settings-kicker">Settings</p>
          <h1>{SETTINGS_SECTION_LABELS[activeSection]}</h1>
        </div>
      </header>

      <div className="settings-page-body settings-page-panel">
        {activeSection === "general" ? (
          <section
            className="settings-section"
            aria-labelledby="theme-settings-heading"
          >
            <h2 id="theme-settings-heading">Theme</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-theme">
                <div>
                  <p className="settings-label">Theme</p>
                  <p className="settings-value">
                    Match the system, or force light or dark. Transitions when
                    you change it.
                  </p>
                </div>
                <div
                  className="theme-switch"
                  role="radiogroup"
                  aria-label="Theme"
                >
                  {THEME_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      role="radio"
                      aria-checked={preference === option.value}
                      className={
                        preference === option.value
                          ? "theme-switch-option is-active"
                          : "theme-switch-option"
                      }
                      onClick={() => setPreference(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "connected-apps" ? <ConnectedAppsSettings /> : null}

        {activeSection === "shortcuts" ? <ShortcutSettings /> : null}

        {activeSection === "quick-access" ? (
          <section
            className="settings-section"
            aria-labelledby="quick-access-settings-heading"
          >
            <h2 id="quick-access-settings-heading">Floating window</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Always-ready quick access</p>
                  <p className="settings-value">
                    Keep workflows and schedules within reach while the main
                    Alfred window is hidden.
                  </p>
                </div>
                <div className="settings-controls settings-quick-access-actions">
                  <button
                    type="button"
                    className="ghost settings-test-btn"
                    disabled={quickAccessBusy}
                    onClick={() => void showQuickAccess()}
                  >
                    Open now
                  </button>
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessEnabled ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessEnabled}
                    aria-label="Enable always-ready quick access"
                    disabled={quickAccessBusy}
                    onClick={() =>
                      void setQuickAccessEnabled(!quickAccessEnabled)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Quick access style</p>
                  <p className="settings-value">
                    Use an invisible hover corner or a movable compact workflow
                    window.
                  </p>
                </div>
                <div
                  className="theme-switch"
                  role="radiogroup"
                  aria-label="Quick access style"
                >
                  <button
                    type="button"
                    role="radio"
                    aria-checked={quickAccessMode === "hover"}
                    className={
                      quickAccessMode === "hover"
                        ? "theme-switch-option is-active"
                        : "theme-switch-option"
                    }
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() => void setQuickAccessMode("hover")}
                  >
                    Hover corner
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={quickAccessMode === "compact"}
                    className={
                      quickAccessMode === "compact"
                        ? "theme-switch-option is-active"
                        : "theme-switch-option"
                    }
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() => void setQuickAccessMode("compact")}
                  >
                    Compact window
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Always above other windows</p>
                  <p className="settings-value">
                    Keep the compact launcher above your other apps.
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessAlwaysOnTop ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessAlwaysOnTop}
                    aria-label="Keep compact quick access above other windows"
                    disabled={
                      quickAccessBusy ||
                      !quickAccessEnabled ||
                      quickAccessMode !== "compact"
                    }
                    onClick={() =>
                      void setQuickAccessAlwaysOnTop(!quickAccessAlwaysOnTop)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Show on full-screen desktops</p>
                  <p className="settings-value">
                    Keep your chosen quick-access style available over
                    full-screen apps and across virtual desktops.
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessFullscreen ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessFullscreen}
                    aria-label="Show quick access on full-screen desktops"
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() =>
                      void setQuickAccessFullscreen(!quickAccessFullscreen)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Compact window position</p>
                  <p className="settings-value">
                    Drag the grip beside Alfred to move it. Its position is
                    restored when Alfred starts again.
                  </p>
                </div>
                <button
                  type="button"
                  className="ghost settings-test-btn"
                  disabled={
                    quickAccessBusy ||
                    !quickAccessEnabled ||
                    quickAccessMode !== "compact"
                  }
                  onClick={() => void resetQuickAccessPosition()}
                >
                  Restore default
                </button>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "general" ? (
          <section
            className="settings-section"
            aria-labelledby="general-settings-heading"
          >
            <h2 id="general-settings-heading">Application behavior</h2>
            <div className="settings-card">
              <div className="settings-row">
                <div>
                  <p className="settings-label">Background operation</p>
                  <p className="settings-value">
                    Closing the window hides Alfred. Schedules keep running
                    until you choose Quit from the menu bar or system tray.
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">App</p>
                  <p className="settings-value">Alfred</p>
                </div>
                <span className="settings-meta">v0.5.0</span>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Platform</p>
                  <p className="settings-value">
                    Desktop only (macOS, Linux, Windows)
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Working directories</p>
                  <p className="settings-value">
                    Set a folder on each workflow so agent CLIs run in that
                    project path.
                  </p>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "notifications" ? (
          <section
            className="settings-section"
            aria-labelledby="notification-settings-heading"
          >
            <h2 id="notification-settings-heading">Run notifications</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Notifications</p>
                  <p className="settings-value">
                    Notify when a run finishes while the window is in the
                    background.
                  </p>
                  <p className="settings-hint">
                    macOS permission: {permissionLabel}
                    {permission === "denied" && isMacPlatform() ? (
                      <>
                        {" · "}
                        <button
                          type="button"
                          className="settings-link"
                          onClick={() => void openSystemSettings()}
                        >
                          Open System Settings
                        </button>
                      </>
                    ) : null}
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      notificationsEnabled ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={notificationsEnabled}
                    aria-label="Enable notifications"
                    disabled={busy}
                    onClick={() =>
                      void setNotificationsEnabled(!notificationsEnabled)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Notification sound</p>
                  <p className="settings-value">
                    Used for finished runs, failures, and desktop Notify nodes.
                  </p>
                </div>
                <div className="settings-sound-controls">
                  <SelectControl
                    containerClassName="settings-sound-select"
                    aria-label="Notification sound"
                    value={notificationSound}
                    disabled={!notificationsEnabled || busy}
                    onChange={(event) =>
                      void setNotificationSound(
                        event.currentTarget.value as NotificationSound,
                      )
                    }
                  >
                    {NOTIFICATION_SOUND_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </SelectControl>
                  <button
                    type="button"
                    className="ghost settings-test-btn"
                    disabled={
                      !notificationsEnabled || permission !== "granted" || busy
                    }
                    onClick={() => void sendTest()}
                  >
                    Preview
                  </button>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "data" ? (
          <section
            className="settings-section"
            aria-labelledby="data-settings-heading"
          >
            <h2 id="data-settings-heading">Local data</h2>
            <div className="settings-card">
              <div className="settings-row">
                <div>
                  <p className="settings-label">Storage</p>
                  <p className="settings-value">
                    Workflows, memories, schedules, and run history are stored
                    locally in SQLite on this machine.
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Linked memories</p>
                  <p className="settings-value">
                    Memories linked from other workflows stay owned by their
                    source workflow. Unlinking only removes the reference.
                  </p>
                </div>
              </div>
            </div>
          </section>
        ) : null}
      </div>
    </section>
  );
}
