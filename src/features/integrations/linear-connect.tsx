import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";

const LINEAR_API_KEYS_URL = "https://linear.app/settings/api";

export function LinearConnect({ onClose }: ConnectDialogProps) {
  const connect = useIntegrationsStore((state) => state.connectLinearPrivate);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [apiKey, setApiKey] = useState("");
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    clearError();
    setOpenError(null);
    return () => setApiKey("");
  }, [clearError]);

  async function openLinear() {
    setOpenError(null);
    try {
      await openUrl(LINEAR_API_KEYS_URL);
    } catch {
      setOpenError(
        "Alfred could not open Linear. Open Linear in your browser and continue with the steps below.",
      );
    }
  }

  async function submit() {
    if (!apiKey.trim()) return;
    const submittedKey = apiKey;
    try {
      const result = await connect({ apiKey: submittedKey });
      if (result === null) onClose();
    } finally {
      setApiKey("");
    }
  }

  return (
    <ConnectedAppTutorialLayout
      providerId="linear"
      providerName="Linear"
      title="Connect Linear with a personal API key"
      titleId="linear-private-title"
      description={
        <p>
          Alfred creates, comments on, and updates issues in the connected
          workspace only. This advanced mode uses your personal API key until
          one-workspace OAuth ships.
        </p>
      }
      badge="Advanced"
      steps={[
        {
          title: "Create a personal API key",
          description: (
            <>
              <p>
                In Linear, open <strong>Settings → API</strong> and create a
                personal API key for the workspace Alfred should use.
              </p>
              <button
                type="button"
                className="ghost tutorial-wizard-step-link"
                onClick={() => void openLinear()}
              >
                Open Linear API settings <ExternalLinkIcon />
              </button>
              <p className="tutorial-wizard-step-note">
                The key acts as your Linear user in that workspace. Revoke it
                in Linear to stop Alfred immediately.
              </p>
            </>
          ),
        },
        {
          title: "Keep the key private",
          description: (
            <p>
              Treat the key like a password. Alfred validates it once, then
              stores it only in your system credential store. It is never
              written into workflows, logs, or the React interface.
            </p>
          ),
        },
      ]}
      onClose={onClose}
    >
      {openError ? (
        <p className="connection-tutorial-inline-error" role="alert">
          {openError}
        </p>
      ) : null}
      <label className="field">
        <span className="connection-tutorial-field-label">
          <span>Personal API key</span>
          <span className="connection-tutorial-required">required</span>
        </span>
        <input
          type="password"
          value={apiKey}
          autoComplete="off"
          spellCheck={false}
          placeholder="lin_…"
          onChange={(event) => setApiKey(event.currentTarget.value)}
        />
        <span className="connection-tutorial-input-hint">
          Copy the key from Linear’s API settings.
        </span>
      </label>
      <p className="connection-tutorial-form-note">
        Alfred can create and update issues, add comments, and watch issue
        activity in the connected workspace.
      </p>
      {error ? (
        <p className="app-action-warning" role="alert">
          {error.message}
        </p>
      ) : null}
      <p className="connection-tutorial-security-copy">
        Alfred validates the key and workspace before saving. It goes directly
        to Rust, stays in your system credential store, and is cleared from
        this form after every attempt.
      </p>
      <div className="schedule-actions">
        <button
          type="button"
          className="primary"
          disabled={loading || !apiKey.trim()}
          onClick={() => void submit()}
        >
          {loading ? "Validating…" : "Connect Linear"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
