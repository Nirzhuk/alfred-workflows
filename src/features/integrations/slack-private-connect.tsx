import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import { useIntegrationsStore } from "./store";

const SLACK_APPS_URL = "https://api.slack.com/apps";

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
    <ConnectedAppTutorialLayout
      providerId="slack"
      providerName="Slack"
      title="Connect a private Slack app"
      titleId="slack-private-title"
      description={
        <p>
          This uses a Slack app you own, not the public Alfred bot. Messages are
          sent by your app&apos;s bot.
        </p>
      }
      badge="Private app"
      steps={[
        {
          title: "Create a Slack app",
          description: (
            <>
              <p>From scratch, in the workspace Alfred should post to.</p>
              <button
                type="button"
                className="ghost tutorial-wizard-step-link"
                onClick={() => void openUrl(SLACK_APPS_URL)}
              >
                api.slack.com/apps <ExternalLinkIcon />
              </button>
            </>
          ),
        },
        {
          title: "Add the bot scopes",
          description: (
            <>
              <p>Under OAuth &amp; Permissions → Bot Token Scopes.</p>
              <div className="tutorial-code-row" aria-label="Required bot scopes">
                <code>chat:write</code>
                <code>channels:read</code>
              </div>
            </>
          ),
        },
        {
          title: "Add optional capabilities",
          description: (
            <>
              <p>Private channels and mentions need extra scopes.</p>
              <div
                className="tutorial-code-row"
                aria-label="Optional bot scopes. Mentions also require connections:write in Socket Mode."
              >
                <code>groups:read</code>
                <code>app_mentions:read</code>
              </div>
            </>
          ),
        },
        {
          title: "Install and copy the token",
          description: (
            <>
              <p>
                Install to workspace, then copy the Bot User OAuth Token.
              </p>
              <code className="tutorial-token-example">xoxb-…</code>
            </>
          ),
        },
      ]}
      onClose={onClose}
    >
          <label className="field">
            <span className="connection-tutorial-field-label">
              <span>Bot token</span>
              <span className="connection-tutorial-required">required</span>
            </span>
            <input
              type="password"
              value={botToken}
              autoComplete="off"
              spellCheck={false}
              placeholder="xoxb-…"
              onChange={(event) => setBotToken(event.currentTarget.value)}
            />
            <span className="connection-tutorial-input-hint">
              Starts with xoxb-. The app-level xapp- token belongs elsewhere.
            </span>
          </label>
          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={privateChannels}
              onChange={(event) =>
                setPrivateChannels(event.currentTarget.checked)
              }
            />
            <span>
              Allow private-channel selectors
              <small>requires groups:read</small>
            </span>
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
          <p className="connection-tutorial-security-copy">
            Alfred validates the credential with Slack before saving. It is sent
            straight to the Rust core, stored in your system credential store,
            and cleared from this form after every attempt.
          </p>
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
    </ConnectedAppTutorialLayout>
  );
}
