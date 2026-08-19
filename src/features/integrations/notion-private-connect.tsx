import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";

const NOTION_CONNECTIONS_URL = "https://www.notion.so/profile/integrations/internal";

export function NotionPrivateConnect({ onClose }: ConnectDialogProps) {
  const connect = useIntegrationsStore((state) => state.connectNotionPrivate);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [integrationToken, setIntegrationToken] = useState("");
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    clearError();
    setOpenError(null);
    return () => setIntegrationToken("");
  }, [clearError]);

  async function openNotion() {
    setOpenError(null);
    try {
      await openUrl(NOTION_CONNECTIONS_URL);
    } catch {
      setOpenError(
        "Alfred could not open Notion. Open Notion in your browser and continue with the steps below.",
      );
    }
  }

  async function submit() {
    if (!integrationToken.trim()) return;
    const submittedToken = integrationToken;
    try {
      const result = await connect({ integrationToken: submittedToken });
      if (result === null) onClose();
    } finally {
      setIntegrationToken("");
    }
  }

  return (
    <ConnectedAppTutorialLayout
      providerId="notion"
      providerName="Notion"
      title="Connect a Notion integration"
      titleId="notion-private-title"
      description={
        <p>
          Alfred reads only the Notion pages and data sources you explicitly
          share. It does not index your workspace.
        </p>
      }
      badge="Read only"
      steps={[
        {
          title: "Create an internal connection",
          description: (
            <>
              <p>
                In Notion, open <strong>Settings → Connections</strong>, choose
                <strong> Develop your own connections</strong>, then create a
                connection for the workspace Alfred should read.
              </p>
              <button
                type="button"
                className="ghost tutorial-wizard-step-link"
                onClick={() => void openNotion()}
              >
                Open Notion <ExternalLinkIcon />
              </button>
              <p className="tutorial-wizard-step-note">
                You must be a workspace owner to create an internal connection.
              </p>
            </>
          ),
        },
        {
          title: "Keep the connection read-only",
          description: (
            <p>
              Enable the <strong>read-content capability only</strong>; Notion
              labels this <strong>Read content only</strong>. Leave write,
              insert, and user-information access turned off.
            </p>
          ),
        },
        {
          title: "Share only the content Alfred should use",
          description: (
            <p>
              On each page or database, open{" "}
              <strong>Share → Add connections</strong> and select your new
              connection. Sharing a parent page also gives it access to that
              page’s children.
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
              <span>Internal integration token</span>
              <span className="connection-tutorial-required">required</span>
            </span>
            <input
              type="password"
              value={integrationToken}
              autoComplete="off"
              spellCheck={false}
              placeholder="ntn_… or secret_…"
              onChange={(event) =>
                setIntegrationToken(event.currentTarget.value)
              }
            />
            <span className="connection-tutorial-input-hint">
              Copy installation access token from Notion’s connection menu.
            </span>
          </label>
          <p className="connection-tutorial-form-note">
            Alfred reads only the Notion pages and data sources you explicitly
            share. It does not index your workspace.
          </p>
          {error ? (
            <p className="app-action-warning" role="alert">
              {error.message}
            </p>
          ) : null}
          <p className="connection-tutorial-security-copy">
            Alfred validates the token and workspace before saving. It goes
            directly to Rust, stays in your system credential store, and is
            cleared from this form after every attempt.
          </p>
          <div className="schedule-actions">
            <button
              type="button"
              className="primary"
              disabled={loading || !integrationToken.trim()}
              onClick={() => void submit()}
            >
              {loading ? "Validating…" : "Connect Notion"}
            </button>
          </div>
    </ConnectedAppTutorialLayout>
  );
}
