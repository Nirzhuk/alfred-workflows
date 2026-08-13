import { useEffect, useState, type KeyboardEvent } from "react";
import {
  DEFAULT_SHORTCUTS,
  SHORTCUT_DEFINITIONS,
  findReservedShortcutConflict,
  findShortcutConflict,
  formatShortcut,
  shortcutFromKeyboardEvent,
  type ShortcutId,
  useShortcutPreferences,
} from "../../shortcuts";

export function ShortcutSettings() {
  const shortcuts = useShortcutPreferences((state) => state.shortcuts);
  const busy = useShortcutPreferences((state) => state.busy);
  const setShortcut = useShortcutPreferences((state) => state.setShortcut);
  const resetShortcuts = useShortcutPreferences(
    (state) => state.resetShortcuts,
  );
  const [recording, setRecording] = useState<ShortcutId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const hasCustomShortcuts = SHORTCUT_DEFINITIONS.some(
    ({ id }) => shortcuts[id] !== DEFAULT_SHORTCUTS[id],
  );

  useEffect(() => {
    if (!recording) return;

    const stopRecordingOutside = (event: PointerEvent) => {
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest(`[data-shortcut-recorder="${recording}"]`)
      ) {
        return;
      }

      setRecording(null);
      setError(null);
      if (document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
    };

    document.addEventListener("pointerdown", stopRecordingOutside, true);
    return () =>
      document.removeEventListener("pointerdown", stopRecordingOutside, true);
  }, [recording]);

  const recordShortcut = async (
    id: ShortcutId,
    event: KeyboardEvent<HTMLButtonElement>,
  ) => {
    if (recording !== id || event.repeat) return;
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") {
      setRecording(null);
      setError(null);
      return;
    }

    const accelerator = shortcutFromKeyboardEvent(event.nativeEvent);
    if (!accelerator) {
      if (!["Alt", "Control", "Meta", "Shift"].includes(event.key)) {
        setError("Use Command, Control, or Alt with a key (or use an F-key).");
      }
      return;
    }

    const conflictId = findShortcutConflict(shortcuts, accelerator, id);
    if (conflictId) {
      const conflict = SHORTCUT_DEFINITIONS.find(
        (definition) => definition.id === conflictId,
      );
      setError(
        `${formatShortcut(accelerator)} is already used by ${conflict?.label ?? "another command"}.`,
      );
      return;
    }

    const reservedLabel = findReservedShortcutConflict(accelerator);
    if (reservedLabel) {
      setError(
        `${formatShortcut(accelerator)} is reserved for ${reservedLabel}.`,
      );
      return;
    }

    try {
      await setShortcut(id, accelerator);
      setRecording(null);
      setError(null);
    } catch (cause) {
      setError(
        id === "quickAccess"
          ? "That system-wide shortcut is unavailable. Try another combination."
          : cause instanceof Error
            ? cause.message
            : "Alfred could not save that shortcut.",
      );
    }
  };

  const reset = async () => {
    setRecording(null);
    setError(null);
    try {
      await resetShortcuts();
    } catch {
      setError(
        "The default system-wide shortcut is unavailable. Close the app using it and try again.",
      );
    }
  };

  return (
    <section
      className="settings-section"
      aria-labelledby="shortcut-settings-heading"
    >
      <div className="settings-section-heading shortcut-settings-heading">
        <div>
          <h2 id="shortcut-settings-heading">Keyboard shortcuts</h2>
          <p className="settings-section-copy">
            Click a shortcut, then press the key combination you want to use.
          </p>
        </div>
        <button
          type="button"
          className="ghost shortcut-reset-button"
          disabled={busy || !hasCustomShortcuts}
          onClick={() => void reset()}
        >
          Reset defaults
        </button>
      </div>

      <div className="settings-card shortcut-settings-card">
        {SHORTCUT_DEFINITIONS.map((definition) => {
          const isRecording = recording === definition.id;
          return (
            <div
              className="settings-row settings-row-control shortcut-settings-row"
              key={definition.id}
            >
              <div>
                <div className="shortcut-settings-label-line">
                  <p className="settings-label">{definition.label}</p>
                  <span className="shortcut-scope">{definition.scope}</span>
                </div>
                <p className="settings-value">{definition.description}</p>
              </div>
              <button
                type="button"
                data-shortcut-recorder={definition.id}
                className={[
                  "shortcut-recorder",
                  isRecording ? "is-recording" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                aria-label={
                  isRecording
                    ? `Recording shortcut for ${definition.label}`
                    : `Change shortcut for ${definition.label}. Current shortcut: ${formatShortcut(shortcuts[definition.id])}`
                }
                aria-pressed={isRecording}
                disabled={busy}
                onClick={() => {
                  setRecording(definition.id);
                  setError(null);
                }}
                onBlur={() => {
                  if (recording === definition.id && !busy) {
                    setRecording(null);
                  }
                }}
                onKeyDown={(event) =>
                  void recordShortcut(definition.id, event)
                }
              >
                {isRecording ? (
                  <span className="shortcut-recording-label">Press keys…</span>
                ) : (
                  <kbd>{formatShortcut(shortcuts[definition.id])}</kbd>
                )}
              </button>
            </div>
          );
        })}
      </div>

      <p
        className={
          error
            ? "shortcut-settings-note is-error"
            : "shortcut-settings-note"
        }
        role={error ? "alert" : undefined}
      >
        {error ?? "Press Escape while recording to keep the current shortcut."}
      </p>
    </section>
  );
}
