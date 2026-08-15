import { useEffect, useState } from "react";
import { Modal, ModalHeader } from "../../components/modal";
import { useIntegrationsStore } from "./store";

export function SlackPrivateConnect({ onClose }: { onClose: () => void }) {
  const connect = useIntegrationsStore((state) => state.connectSlackPrivate);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [botToken, setBotToken] = useState("");
  const [appToken, setAppToken] = useState("");
  const [privateChannels, setPrivateChannels] = useState(false);
  const [mentions, setMentions] = useState(false);

  useEffect(() => {
    clearError();
    return () => {
      // Explicitly remove secret text from React state when the form leaves.
      setBotToken("");
      setAppToken("");
    };
  }, [clearError]);

  async function submit() {
    if (!botToken.trim()) return;
    const submittedToken = botToken;
    try {
      const result = await connect({
        mode: "bot",
        botToken: submittedToken,
        appToken: mentions ? appToken : null,
        webhookUrl: null,
        enablePrivateChannels: privateChannels,
        enableMentions: mentions,
      });
      if (result === null) onClose();
    } finally {
      // The backend has copied the command payload into Rust by this point.
      // Never retain the token after success or failure.
      setBotToken("");
      setAppToken("");
    }
  }

  return (
    <Modal size="md" onClose={onClose} labelledBy="slack-private-title">
      <ModalHeader
        eyebrow="Advanced setup"
        title="Connect a private Slack app"
        titleId="slack-private-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        }
      />
      <div className="schedule-modal-body">
        <p className="muted">
          This is a developer/private-workspace connection, not the public
          Alfred bot. Messages are sent by a bot from a Slack app you own.
        </p>
        <ol className="slack-setup-steps">
          <li>Create a Slack app for the target workspace.</li>
          <li>
            Add bot scopes <code>chat:write</code> and{" "}
            <code>channels:read</code>.
          </li>
          <li>
            If you need private-channel selectors, also add{" "}
            <code>groups:read</code> and enable the option below.
          </li>
          <li>
            For mention triggers, add <code>app_mentions:read</code>, subscribe
            to the <code>app_mention</code> bot event, enable Socket Mode, and
            generate an app-level token with <code>connections:write</code>.
          </li>
          <li>Install the app and copy its bot token.</li>
        </ol>
        <p className="hint">
          Alfred validates the token with Slack before saving it. The token is
          sent directly to Rust, stored in your system credential store, and
          cleared from this form after every attempt.
        </p>
        <label className="field">
          <span>Bot token</span>
          <input
            type="password"
            value={botToken}
            autoComplete="off"
            spellCheck={false}
            placeholder="xoxb-…"
            onChange={(event) => setBotToken(event.currentTarget.value)}
          />
        </label>
        <label className="field checkbox-field">
          <input
            type="checkbox"
            checked={privateChannels}
            onChange={(event) =>
              setPrivateChannels(event.currentTarget.checked)
            }
          />
          <span>Allow private-channel selectors (requires groups:read)</span>
        </label>
        <label className="field checkbox-field">
          <input
            type="checkbox"
            checked={mentions}
            onChange={(event) => {
              const checked = event.currentTarget.checked;
              setMentions(checked);
              if (!checked) setAppToken("");
            }}
          />
          <span>Receive app mentions while Alfred is open</span>
        </label>
        {mentions ? (
          <label className="field">
            <span>Socket Mode app token</span>
            <input
              type="password"
              value={appToken}
              autoComplete="off"
              spellCheck={false}
              placeholder="xapp-…"
              onChange={(event) => setAppToken(event.currentTarget.value)}
            />
            <span className="hint">
              Stored with the bot token in your system credential store. It is
              used only to obtain temporary Slack WebSocket URLs.
            </span>
          </label>
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
            disabled={
              loading || !botToken.trim() || (mentions && !appToken.trim())
            }
            onClick={() => void submit()}
          >
            {loading ? "Validating…" : "Connect private app"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
