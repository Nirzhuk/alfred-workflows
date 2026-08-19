import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../../components/icon";
import { Modal, ModalHeader } from "../../components/modal";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";
import type { GmailAuthorizationStarted } from "./types";

export function GmailConnect({ onClose }: ConnectDialogProps) {
  const prepare = useIntegrationsStore((state) => state.prepareGmailConnection);
  const complete = useIntegrationsStore(
    (state) => state.completeGmailConnection,
  );
  const cancel = useIntegrationsStore((state) => state.cancelGmailAuthorization);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [authorization, setAuthorization] =
    useState<GmailAuthorizationStarted | null>(null);
  const [opened, setOpened] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const sessionRef = useRef<string | null>(null);

  useEffect(() => {
    clearError();
    return () => {
      const sessionId = sessionRef.current;
      sessionRef.current = null;
      if (sessionId) void cancel(sessionId);
    };
  }, [cancel, clearError]);

  async function start() {
    setOpenError(null);
    const prepared = await prepare();
    if (!prepared) return;
    sessionRef.current = prepared.sessionId;
    setAuthorization(prepared);
  }

  async function openBrowser() {
    if (!authorization) return;
    setOpenError(null);
    try {
      await openUrl(authorization.authorizationUrl);
      setOpened(true);
    } catch {
      setOpenError(
        "Alfred could not open the browser. Copy the address and open it manually.",
      );
    }
  }

  async function finish() {
    const sessionId = sessionRef.current;
    if (!sessionId) return;
    const failure = await complete(sessionId);
    if (failure) {
      if (failure.code === "gmail_pairing_cancelled") {
        onClose();
      }
      return;
    }
    sessionRef.current = null;
    onClose();
  }

  async function close() {
    const sessionId = sessionRef.current;
    sessionRef.current = null;
    if (sessionId) await cancel(sessionId);
    onClose();
  }

  return authorization ? (
    <Modal
      size="lg"
      className="connection-tutorial-modal"
      onClose={() => void close()}
      labelledBy="gmail-title"
    >
      <ModalHeader
        eyebrow="Gmail"
        title="Connect Gmail"
        titleId="gmail-title"
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
        <div className="github-pairing-card">
          <p className="settings-label">Sign in with Google</p>
          <p className="settings-value">
            Alfred opens Google in your system browser. The authorization page
            never sees your Google password, and only the send-mail permission
            is requested.
          </p>
          <button
            type="button"
            className="ghost tutorial-wizard-step-link"
            disabled={loading}
            onClick={() => void openBrowser()}
          >
            {opened ? "Open Google again" : "Open Google"} <ExternalLinkIcon />
          </button>
          {opened ? (
            <p className="hint">
              Complete authorization in the browser, then press Connect.
            </p>
          ) : null}
        </div>
        <div className="github-pairing-card">
          <p className="settings-label">Finish connecting</p>
          <p className="settings-value">
            Authorization expires at{" "}
            {new Date(authorization.expiresAt).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
            . Alfred stores tokens only in your system credential store.
          </p>
        </div>
        {openError ? (
          <p className="app-action-warning" role="alert">
            {openError}
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
            onClick={() => void finish()}
          >
            {loading ? "Waiting for Google…" : "Connect"}
          </button>
        </div>
      </div>
    </Modal>
  ) : (
    <ConnectedAppTutorialLayout
      providerId="gmail"
      providerName="Gmail"
      title="Connect Gmail"
      titleId="gmail-title"
      description={
        <p>
          Alfred uses Google native-app authorization in your system browser.
          Only send access is requested.
        </p>
      }
      badge="Send only"
      formLabel="Then authorize it in your browser"
      steps={[
        {
          title: "Open Google",
          description: (
            <p>
              Alfred starts an authorization and opens Google in your system
              browser.
            </p>
          ),
        },
        {
          title: "Review the permission",
          description: (
            <p>
              Alfred requests only the gmail.send permission — no reading,
              searching, or deleting of mail.
            </p>
          ),
        },
        {
          title: "Send from workflows",
          description: (
            <p>
              Workflows can send plain-text email from this account. No
              attachments or HTML.
            </p>
          ),
        },
      ]}
      onClose={() => void close()}
    >
      <p className="connection-tutorial-form-note">
        Tokens are saved only in your system credential store. Mail reading,
        search, and new-mail triggers require a separately verified access phase
        and are not part of this connection.
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
          {loading ? "Starting…" : "Continue with Google"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
