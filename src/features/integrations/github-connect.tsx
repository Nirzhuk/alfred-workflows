import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../../components/icon";
import { Modal, ModalHeader } from "../../components/modal";
import { AppLogo } from "./app-logo";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";
import type { GitHubDeviceAuthorization } from "./types";

export function GitHubConnect({ onClose }: ConnectDialogProps) {
  const prepare = useIntegrationsStore((state) => state.prepareGithubConnection);
  const poll = useIntegrationsStore((state) => state.pollGithubConnection);
  const cancel = useIntegrationsStore((state) => state.cancelGithubPairing);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [pairing, setPairing] = useState<GitHubDeviceAuthorization | null>(null);
  const [opened, setOpened] = useState(false);
  const [copied, setCopied] = useState(false);
  const [pendingMessage, setPendingMessage] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const pairingSessionRef = useRef<string | null>(null);

  useEffect(() => {
    clearError();
    return () => {
      const sessionId = pairingSessionRef.current;
      pairingSessionRef.current = null;
      if (sessionId) void cancel(sessionId);
    };
  }, [cancel, clearError]);

  async function start() {
    setPendingMessage(null);
    setOpenError(null);
    const prepared = await prepare();
    if (!prepared) return;
    pairingSessionRef.current = prepared.pairingSessionId;
    setPairing(prepared);
  }

  async function openExternal(url: string, label: string) {
    setOpenError(null);
    try {
      await openUrl(url);
      if (label === "GitHub") setOpened(true);
    } catch {
      setOpenError(`Alfred could not open ${label}. Open the address shown and continue there.`);
    }
  }

  async function copyCode() {
    if (!pairing) return;
    try {
      await navigator.clipboard.writeText(pairing.userCode);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_500);
    } catch {
      setCopied(false);
    }
  }

  async function checkAuthorization() {
    if (!pairing) return;
    setPendingMessage(null);
    const result = await poll(pairing.pairingSessionId);
    if (!result) return;
    if (result.status === "connected") {
      pairingSessionRef.current = null;
      onClose();
      return;
    }
    setPendingMessage(
      `GitHub is still waiting for authorization. Check again in ${Math.max(1, result.retryAfterSeconds)} seconds.`,
    );
  }

  async function close() {
    const sessionId = pairingSessionRef.current;
    pairingSessionRef.current = null;
    if (sessionId) await cancel(sessionId);
    onClose();
  }

  return pairing ? (
    <Modal
      size="lg"
      className="connection-tutorial-modal"
      onClose={() => void close()}
      labelledBy="github-title"
      describedBy="github-description"
    >
      <ModalHeader
        leading={<AppLogo providerId="github" providerName="GitHub" size={40} />}
        title="Connect GitHub"
        titleId="github-title"
        description="Authorize Alfred and choose the repositories available to workflows."
        descriptionId="github-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={() => void close()}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />
      <div className="schedule-modal-body">
        {pairing.installationUrl ? (
          <div className="github-pairing-card">
            <p className="settings-label">1. Select repositories</p>
            <p className="settings-value">
              Install or configure the Alfred GitHub App for only the
              repositories you want available to workflows.
            </p>
            <button
              type="button"
              className="ghost tutorial-wizard-step-link"
              disabled={loading}
              onClick={() =>
                void openExternal(pairing.installationUrl!, "GitHub App installation")
              }
            >
              Select repositories on GitHub <ExternalLinkIcon />
            </button>
          </div>
        ) : null}
        <div className="github-pairing-card">
          <p className="settings-label">
            {pairing.installationUrl ? "2. Authorize Alfred" : "Authorize Alfred"}
          </p>
          <p className="settings-value">
            Enter this one-time code on GitHub before{" "}
            {new Date(pairing.expiresAt).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
            .
          </p>
          <div className="github-device-code-row">
            <code aria-label="GitHub device code">{pairing.userCode}</code>
            <button type="button" className="ghost" onClick={() => void copyCode()}>
              {copied ? "Copied" : "Copy code"}
            </button>
          </div>
          <button
            type="button"
            className="ghost tutorial-wizard-step-link"
            disabled={loading}
            onClick={() => void openExternal(pairing.verificationUri, "GitHub")}
          >
            {opened ? "Open GitHub again" : "Open GitHub"} <ExternalLinkIcon />
          </button>
          <p className="hint">
            If the button does not open, visit <code>{pairing.verificationUri}</code>.
          </p>
          <p className="hint">
            Only enter a code you just generated here in Alfred. Never enter a
            code someone else sent you.
          </p>
        </div>
        <p className="hint">
          Organization repositories can require administrator approval or an
          active SAML SSO session. Alfred never receives your GitHub password.
        </p>
        {openError ? (
          <p className="app-action-warning" role="alert">
            {openError}
          </p>
        ) : null}
        {pendingMessage ? (
          <p className="hint" role="status">
            {pendingMessage}
          </p>
        ) : null}
        {error ? (
          <p className="app-action-warning" role="alert">
            {error.message}
          </p>
        ) : null}
        <div className="schedule-actions">
          <button
            type="button"
            className="primary"
            disabled={loading}
            onClick={() => void checkAuthorization()}
          >
            {loading ? "Checking…" : "I've authorized. Connect"}
          </button>
        </div>
      </div>
    </Modal>
  ) : (
    <ConnectedAppTutorialLayout
      providerId="github"
      providerName="GitHub"
      title="Connect GitHub"
      titleId="github-title"
      description={
        <p>
          Alfred uses GitHub App device authorization. Access is limited to the
          repositories selected for the app installation.
        </p>
      }
      badge="Scoped access"
      formLabel="Then authorize it in GitHub"
      steps={[
        {
          title: "Select repositories",
          description: (
            <p>
              Choose only the repositories you want available to workflows.
              Repository metadata is read; no repository contents are requested.
            </p>
          ),
        },
        {
          title: "Authorize Alfred",
          description: (
            <p>
              Alfred starts a device authorization and gives you a one-time code
              to enter on GitHub.
            </p>
          ),
        },
        {
          title: "Review the boundary",
          description: (
            <p>
              Issues and pull requests are read and write. No repository contents, administration, or code-push access is granted.
            </p>
          ),
        },
      ]}
      onClose={() => void close()}
    >
      <p className="connection-tutorial-form-note">
        Tokens are saved only in your system credential store. Existing Git Host
        nodes continue to use your separate <code>gh</code> CLI login.
      </p>
      {error ? (
        <p className="app-action-warning" role="alert">
          {error.message}
        </p>
      ) : null}
      <div className="schedule-actions">
        <button
          type="button"
          className="primary"
          disabled={loading}
          onClick={() => void start()}
        >
          {loading ? "Starting…" : "Start GitHub authorization"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
