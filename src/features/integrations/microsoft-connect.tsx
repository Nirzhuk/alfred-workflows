import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../components/icon";
import { Modal, ModalHeader } from "../../components/modal";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";
import type { MicrosoftAuthorizationStarted } from "./types";

export function MicrosoftConnect({
  onClose,
  reconnectConnectionId = null,
}: ConnectDialogProps) {
  const prepare = useIntegrationsStore(
    (state) => state.prepareMicrosoftConnection,
  );
  const complete = useIntegrationsStore(
    (state) => state.completeMicrosoftConnection,
  );
  const cancel = useIntegrationsStore(
    (state) => state.cancelMicrosoftAuthorization,
  );
  const connections = useIntegrationsStore((state) => state.connections);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const existing = useMemo(
    () =>
      reconnectConnectionId
        ? connections.find(
            (connection) => connection.id === reconnectConnectionId,
          )
        : undefined,
    [connections, reconnectConnectionId],
  );
  const [sendMail, setSendMail] = useState(
    () => existing?.scopes.includes("Mail.Send") ?? false,
  );
  const [readMail, setReadMail] = useState(
    () => existing?.scopes.includes("Mail.ReadBasic") ?? false,
  );
  const [calendar, setCalendar] = useState(
    () => existing?.scopes.includes("Calendars.ReadWrite") ?? false,
  );
  const [authorization, setAuthorization] =
    useState<MicrosoftAuthorizationStarted | null>(null);
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
    const prepared = await prepare({
      sendMail,
      readMail,
      calendar,
      reconnectConnectionId: reconnectConnectionId ?? null,
    });
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
      if (failure.code === "microsoft_pairing_cancelled") {
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
      labelledBy="microsoft-title"
    >
      <ModalHeader
        eyebrow="Microsoft 365"
        title="Connect Microsoft 365"
        titleId="microsoft-title"
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
          <p className="settings-label">Sign in with Microsoft</p>
          <p className="settings-value">
            Alfred opens Microsoft in your system browser. The authorization
            page never sees your Microsoft password, and Alfred never uses a
            desktop client secret.
          </p>
          <button
            type="button"
            className="ghost tutorial-wizard-step-link"
            disabled={loading}
            onClick={() => void openBrowser()}
          >
            {opened ? "Open Microsoft again" : "Open Microsoft"}{" "}
            <ExternalLinkIcon />
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
            {loading ? "Waiting for Microsoft…" : "Connect"}
          </button>
        </div>
      </div>
    </Modal>
  ) : (
    <ConnectedAppTutorialLayout
      providerId="microsoft"
      providerName="Microsoft 365"
      title="Connect Microsoft 365"
      titleId="microsoft-title"
      description={
        <p>
          Alfred uses Microsoft native-app authorization in your system
          browser. Choose only the mail and calendar capabilities you need.
        </p>
      }
      badge="Least privilege"
      formLabel="Then choose access and authorize it in your browser"
      steps={[
        {
          title: "Choose capabilities",
          description: (
            <p>
              Identity is always requested. Mail.Send, Mail.ReadBasic, and
              Calendars.ReadWrite are added only when you enable them.
            </p>
          ),
        },
        {
          title: "Open Microsoft",
          description: (
            <p>
              Alfred starts an authorization and opens Microsoft in your system
              browser. Conditional Access and MFA complete there.
            </p>
          ),
        },
        {
          title: "Use mail and calendar in workflows",
          description: (
            <p>
              Workflows can send mail, list metadata with no reading of full
              message bodies, and create events. New-mail triggers run only
              while Alfred is open.
            </p>
          ),
        },
      ]}
      onClose={() => void close()}
    >
      <p className="connection-tutorial-form-note">
        Mail.Read is never requested. Previews use Mail.ReadBasic bodyPreview
        only. Attachments stay out of Alfred. Calendar and mail events poll
        locally while Alfred is open; Graph webhooks are not part of this
        connection.
      </p>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={sendMail}
          onChange={(event) => setSendMail(event.currentTarget.checked)}
        />
        <span>
          Send Outlook email
          <small>Mail.Send</small>
        </span>
      </label>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={readMail}
          onChange={(event) => setReadMail(event.currentTarget.checked)}
        />
        <span>
          Read recent mail metadata
          <small>Mail.ReadBasic — subject, sender, and bodyPreview only</small>
        </span>
      </label>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={calendar}
          onChange={(event) => setCalendar(event.currentTarget.checked)}
        />
        <span>
          Create calendar events
          <small>Calendars.ReadWrite</small>
        </span>
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
          disabled={loading}
          onClick={() => void start()}
        >
          {loading ? "Starting…" : "Continue with Microsoft"}
        </button>
      </div>
    </ConnectedAppTutorialLayout>
  );
}
