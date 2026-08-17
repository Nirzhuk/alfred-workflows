import { useEffect, useState } from "react";
import { Modal, ModalHeader } from "../../components/modal";
import { useIntegrationsStore } from "./store";

export function NotionPrivateConnect({ onClose }: { onClose: () => void }) {
  const connect = useIntegrationsStore((state) => state.connectNotionPrivate);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [integrationToken, setIntegrationToken] = useState("");

  useEffect(() => {
    clearError();
    return () => setIntegrationToken("");
  }, [clearError]);

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
    <Modal size="md" onClose={onClose} labelledBy="notion-private-title">
      <ModalHeader
        eyebrow="Private, local setup"
        title="Connect a Notion integration"
        titleId="notion-private-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        }
      />
      <div className="schedule-modal-body">
        <p className="muted">
          Alfred fetches only pages and data sources you explicitly share with
          an internal integration. It does not index your workspace.
        </p>
        <ol className="slack-setup-steps">
          <li>Create an internal integration in your Notion workspace.</li>
          <li>Enable read-content capability only.</li>
          <li>
            On each page or database you want Alfred to use, choose Add
            connections and select that integration.
          </li>
          <li>Copy the internal integration token below.</li>
        </ol>
        <p className="hint">
          Alfred validates the bot and workspace identity before saving. The
          token goes directly to Rust, is stored in your system credential
          store, and is cleared from this form after every attempt. Retrieved
          content is kept only in workflow run history under your existing
          local retention settings.
        </p>
        <label className="field">
          <span>Internal integration token</span>
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
        </label>
        {error ? (
          <p className="app-action-warning" role="alert">
            {error.message}
          </p>
        ) : null}
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
      </div>
    </Modal>
  );
}
