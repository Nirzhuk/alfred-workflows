import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";

const SENTRY_AUTH_TOKENS_URL = "https://sentry.io/settings/account/api/auth-tokens/";

export function SentryConnect({ onClose }: ConnectDialogProps) {
  const connect = useIntegrationsStore((state) => state.connectSentryPrivate);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [authToken, setAuthToken] = useState("");
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    clearError();
    setOpenError(null);
    return () => setAuthToken("");
  }, [clearError]);

  async function openSentry() {
    setOpenError(null);
    try {
      await openUrl(SENTRY_AUTH_TOKENS_URL);
    } catch {
      setOpenError(
        "Alfred could not open Sentry. Open Sentry in your browser and continue with the steps below.",
      );
    }
  }

  async function submit() {
    if (!authToken.trim()) return;
    const submittedToken = authToken;
    try {
      const result = await connect({ authToken: submittedToken });
      if (result === null) onClose();
    } finally {
      setAuthToken("");
    }
  }

  return (
    <ConnectedAppTutorialLayout
      providerId="sentry"
      providerName="Sentry"
      title="Connect Sentry with an auth token"
      titleId="sentry-auth-title"
      description={
        <p>
          Alfred reads issue alerts and updates issue status in the projects
          your token can access. It never reads stack traces or event data by
          default.
        </p>
      }
      badge="Advanced"
      steps={[
        {
          title: "Create an auth token",
          description: (
            <>
              <p>
                In Sentry, open <strong>Settings → User Auth Tokens</strong>{" "}
                and create a token with the{" "}
                <strong>org:read, project:read, event:read</strong>, and
                optionally <strong>event:write</strong> scopes.
              </p>
              <button
                type="button"
                className="ghost tutorial-wizard-step-link"
                onClick={() => void openSentry()}
              >
                Open Sentry auth tokens <ExternalLinkIcon />
              </button>
              <p className="tutorial-wizard-step-note">
                Without event:write, Alfred can read issues but not change
                their status.
              </p>
            </>
          ),
        },
        {
          title: "Keep the token private",
          description: (
            <p>
              Treat the token like a password. Alfred validates it once, then
              stores it only in your system credential store. Stack traces,
              request data, and user context are never fetched or persisted.
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
          <span>Auth token</span>
          <span className="connection-tutorial-required">required</span>
        </span>
        <input
          type="password"
          value={authToken}
          autoComplete="off"
          spellCheck={false}
          placeholder="sntryu_…"
          onChange={(event) => setAuthToken(event.currentTarget.value)}
        />
        <span className="connection-tutorial-input-hint">
          Copy the token from Sentry’s auth-token settings.
        </span>
      </label>
      <p className="connection-tutorial-form-note">
        Alfred can read issue summaries and update statuses in the Sentry
        organizations granted to this token.
      </p>
      {error ? (
        <p className="app-action-warning" role="alert">
          {error.message}
        </p>
      ) : null}
      <p className="connection-tutorial-security-copy">
        Alfred validates the token and its scopes before saving. It goes
        directly to Rust, stays in your system credential store, and is
        cleared from this form after every attempt.
      </p>
      <div className="schedule-actions">
        <button
          type="button"
          className="primary"
          disabled={loading || !authToken.trim()}
          onClick={() => void submit()}
        >
          {loading ? "Validating…" : "Connect Sentry"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
