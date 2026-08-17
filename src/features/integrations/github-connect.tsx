import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { Modal, ModalHeader } from "../../components/modal";
import { useIntegrationsStore } from "./store";
import type { GitHubDeviceAuthorization } from "./types";

export function GitHubConnect({ onClose }: { onClose: () => void }) {
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

  return (
    <Modal size="md" onClose={() => void close()} labelledBy="github-title">
      <ModalHeader
        eyebrow="Developer workflows"
        title="Connect GitHub"
        titleId="github-title"
        actions={
          <button type="button" className="ghost" onClick={() => void close()}>
            Close
          </button>
        }
      />
      <div className="schedule-modal-body">
        {!pairing ? (
          <>
            <p className="muted">
              Alfred uses a GitHub App device authorization. Access is limited
              to the repositories selected for the app installation and to the
              permissions of your GitHub account.
            </p>
            <ul className="github-connection-notes">
              <li>Repository metadata: read</li>
              <li>Issues and pull requests: read and write</li>
              <li>No repository contents, administration, or code-push access</li>
            </ul>
            <p className="hint">
              Tokens are saved only in your system credential store. Existing
              Git Host nodes continue to use your separate <code>gh</code> CLI login.
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
          </>
        ) : (
          <>
            {pairing.installationUrl ? (
              <div className="github-pairing-card">
                <p className="settings-label">1. Select repositories</p>
                <p className="settings-value">
                  Install or configure the Alfred GitHub App for only the
                  repositories you want available to workflows.
                </p>
                <button
                  type="button"
                  className="ghost"
                  disabled={loading}
                  onClick={() =>
                    void openExternal(pairing.installationUrl!, "GitHub App installation")
                  }
                >
                  Select repositories on GitHub
                </button>
              </div>
            ) : null}
            <div className="github-pairing-card">
              <p className="settings-label">
                {pairing.installationUrl ? "2. Authorize Alfred" : "Authorize Alfred"}
              </p>
              <p className="settings-value">
                Enter this one-time code on GitHub before {" "}
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
                className="ghost"
                disabled={loading}
                onClick={() => void openExternal(pairing.verificationUri, "GitHub")}
              >
                {opened ? "Open GitHub again" : "Open GitHub"}
              </button>
              <p className="hint">
                If the button does not open, visit <code>{pairing.verificationUri}</code>.
              </p>
            </div>
            <p className="hint">
              Organization repositories can require administrator approval or
              an active SAML SSO session. Alfred never receives your GitHub password.
            </p>
            {openError ? (
              <p className="app-action-warning" role="alert">
                {openError}
              </p>
            ) : null}
            {pendingMessage ? <p className="hint" role="status">{pendingMessage}</p> : null}
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
                {loading ? "Checking…" : "I've authorized — connect"}
              </button>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}
