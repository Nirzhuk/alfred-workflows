import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { useIntegrationsStore } from "./store";

export function ObsidianVaultConnect({ onClose }: { onClose: () => void }) {
  const connect = useIntegrationsStore((state) => state.connectObsidianVault);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [vaultPath, setVaultPath] = useState("");

  useEffect(() => {
    clearError();
    return () => setVaultPath("");
  }, [clearError]);

  async function chooseVault() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose an Obsidian vault",
    });
    if (typeof selected === "string") setVaultPath(selected);
  }

  async function submit() {
    if (!vaultPath) return;
    const submittedPath = vaultPath;
    try {
      const result = await connect({ vaultPath: submittedPath });
      if (result === null) onClose();
    } finally {
      setVaultPath("");
    }
  }

  return (
    <ConnectedAppTutorialLayout
      providerId="obsidian"
      providerName="Obsidian"
      title="Connect an Obsidian vault"
      titleId="obsidian-vault-title"
      description={
        <p>
          Alfred reads Markdown notes from one vault when a workflow runs. It
          does not upload, index, edit, or watch your vault.
        </p>
      }
      badge="Local vault"
      formLabel="Then select it in Alfred"
      steps={[
        {
          title: "Choose the vault folder",
          description: (
            <p>
              Select the folder that contains the <code>.obsidian</code>
              directory.
            </p>
          ),
        },
        {
          title: "Add an Obsidian action",
          description: (
            <p>Create a workflow with an Obsidian search or read action.</p>
          ),
        },
        {
          title: "Select a note by its relative path",
          description: (
            <p>
              Use a vault-relative path. Only eligible Markdown files are read
              when the workflow runs.
            </p>
          ),
        },
      ]}
      onClose={onClose}
    >
      <div className="field">
        <span className="connection-tutorial-field-label">
          <span>Obsidian vault</span>
          <span className="connection-tutorial-required">required</span>
        </span>
        <button
          type="button"
          className="ghost connection-tutorial-picker"
          disabled={loading}
          onClick={() => void chooseVault()}
        >
          {vaultPath ? "Choose another vault" : "Choose vault folder…"}
        </button>
        {vaultPath ? (
          <span className="settings-hint connection-tutorial-path">
            {vaultPath}
          </span>
        ) : null}
      </div>
      {error ? (
        <p className="app-action-warning" role="alert">
          {error.message}
        </p>
      ) : null}
      <p className="connection-tutorial-form-note">
        Hidden folders and symlinks are ignored. Workflow configuration
        contains only relative note paths.
      </p>
      <p className="connection-tutorial-security-copy">
        The vault path is sent directly to Rust and kept in the system
        credential store. Retrieved text is treated as untrusted external
        content.
      </p>
      <div className="schedule-actions">
        <button
          type="button"
          className="primary"
          disabled={loading || !vaultPath}
          onClick={() => void submit()}
        >
          {loading ? "Validating…" : "Connect vault"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
