import { useEffect } from "react";
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
  onClose: () => void;
};

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsPage({ onClose }: Props) {
  const preference = useThemeStore((s) => s.preference);
  const setPreference = useThemeStore((s) => s.setPreference);

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
          <p className="settings-kicker">Alfred</p>
          <h1>Settings</h1>
        </div>
        <button type="button" className="ghost" onClick={onClose}>
          Back to workflows
        </button>
      </header>

      <div className="settings-page-body">
        <section className="settings-section">
          <h2>Appearance</h2>
          <div className="settings-card">
            <div className="settings-row settings-row-theme">
              <div>
                <p className="settings-label">Theme</p>
                <p className="settings-value">
                  Match the system, or force light or dark. Transitions when you
                  change it.
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

        <section className="settings-section">
          <h2>General</h2>
          <div className="settings-card">
            <div className="settings-row">
              <div>
                <p className="settings-label">App</p>
                <p className="settings-value">Alfred</p>
              </div>
              <span className="settings-meta">v0.1.0</span>
            </div>
            <div className="settings-row">
              <div>
                <p className="settings-label">Platform</p>
                <p className="settings-value">
                  Desktop only (macOS, Linux, Windows)
                </p>
              </div>
            </div>
          </div>
        </section>

        <section className="settings-section">
          <h2>Runs</h2>
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
                <select
                  className="settings-sound-select"
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
                </select>
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
            <div className="settings-row">
              <div>
                <p className="settings-label">Working directories</p>
                <p className="settings-value">
                  Set a folder on each workflow so agent CLIs run in that project
                  path.
                </p>
              </div>
            </div>
          </div>
        </section>

        <section className="settings-section">
          <h2>Data</h2>
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
      </div>
    </section>
  );
}
