import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../../components/icon";
import { Modal, ModalHeader } from "../../components/modal";
import { AppLogo } from "./app-logo";
import { ConnectedAppTutorialLayout } from "./components/connected-app-tutorial-layout";
import { ExternalLinkIcon } from "./components/external-link-icon";
import { TelegramSetupProgress } from "./components/telegram-setup-progress";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";
import type { TelegramPairingPrepared } from "./types";

const DEFAULT_TEST_MESSAGE =
  "Alfred is connected and ready to send personal notifications.";
const TERMINAL_PAIRING_ERRORS = new Set([
  "telegram_token_invalid",
  "telegram_pairing_expired",
  "telegram_pairing_ambiguous",
  "telegram_private_chat_required",
  "telegram_test_delivery_unknown",
]);

export function TelegramConnect({ onClose }: ConnectDialogProps) {
  const prepare = useIntegrationsStore(
    (state) => state.prepareTelegramConnection,
  );
  const complete = useIntegrationsStore(
    (state) => state.completeTelegramConnection,
  );
  const cancel = useIntegrationsStore((state) => state.cancelTelegramPairing);
  const loading = useIntegrationsStore((state) => state.loading);
  const error = useIntegrationsStore((state) => state.error);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [botToken, setBotToken] = useState("");
  const [pairing, setPairing] = useState<TelegramPairingPrepared | null>(null);
  const [telegramOpened, setTelegramOpened] = useState(false);
  const [testMessage, setTestMessage] = useState(DEFAULT_TEST_MESSAGE);
  const [openError, setOpenError] = useState<string | null>(null);
  const pairingSessionRef = useRef<string | null>(null);

  useEffect(() => {
    clearError();
    return () => {
      setBotToken("");
      setTestMessage("");
      const sessionId = pairingSessionRef.current;
      pairingSessionRef.current = null;
      if (sessionId) void cancel(sessionId);
    };
  }, [cancel, clearError]);

  async function validateToken() {
    if (!botToken.trim()) return;
    const submittedToken = botToken;
    try {
      const prepared = await prepare({ botToken: submittedToken });
      if (!prepared) return;
      pairingSessionRef.current = prepared.pairingSessionId;
      setPairing(prepared);
      setTelegramOpened(false);
    } finally {
      // Rust has copied the command payload by this point. Retain no token in
      // the form, regardless of validation success or failure.
      setBotToken("");
    }
  }

  async function openTelegramLink(url: string) {
    setOpenError(null);
    try {
      await openUrl(url);
      return true;
    } catch {
      setOpenError(
        "Alfred could not open Telegram. Check that Telegram is installed and try again.",
      );
      return false;
    }
  }

  async function openTelegram() {
    if (!pairing) return;
    if (await openTelegramLink(pairing.pairingUrl)) setTelegramOpened(true);
  }

  async function openBotFather() {
    await openTelegramLink("https://t.me/BotFather");
  }

  async function finishPairing() {
    if (!pairing || !testMessage.trim()) return;
    const result = await complete({
      pairingSessionId: pairing.pairingSessionId,
      testMessage,
    });
    if (result === null) {
      pairingSessionRef.current = null;
      setTestMessage("");
      onClose();
    } else if (TERMINAL_PAIRING_ERRORS.has(result.code)) {
      const sessionId = pairingSessionRef.current;
      pairingSessionRef.current = null;
      if (sessionId) await cancel(sessionId);
      setPairing(null);
      setTelegramOpened(false);
      setTestMessage(DEFAULT_TEST_MESSAGE);
    }
  }

  async function close() {
    setBotToken("");
    setTestMessage("");
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
        labelledBy="telegram-title"
        describedBy="telegram-description"
      >
        <ModalHeader
          leading={
            <AppLogo providerId="telegram" providerName="Telegram" size={40} />
          }
          title="Connect Telegram"
          titleId="telegram-title"
          description="Finish linking your dedicated bot to your private chat."
          descriptionId="telegram-description"
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
          <TelegramSetupProgress pairingStarted />
          <div className="telegram-pairing-card">
            <p className="settings-label">@{pairing.botUsername}</p>
            <p className="settings-value">
              Open this bot, press <strong>Start</strong>, then return to
              Alfred. The one-use link expires at{" "}
              {new Date(pairing.expiresAt).toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              })}
              .
            </p>
            <button
              type="button"
              className="ghost telegram-open-button"
              disabled={loading}
              onClick={() => void openTelegram()}
            >
              {telegramOpened ? "Open Telegram again" : "Open Telegram"}
            </button>
          </div>
          {openError ? (
            <p className="app-action-warning" role="alert">
              {openError}
            </p>
          ) : null}
          <label className="field">
            <span>Test notification</span>
            <textarea
              value={testMessage}
              maxLength={4096}
              rows={3}
              onChange={(event) => setTestMessage(event.currentTarget.value)}
            />
            <span className="hint">
              Pairing is saved only after Telegram accepts this explicit test.
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
              disabled={loading || !telegramOpened || !testMessage.trim()}
              onClick={() => void finishPairing()}
            >
              {loading ? "Checking and sending…" : "Finish pairing and send test"}
            </button>
          </div>
        </div>
      </Modal>
    ) : (
      <ConnectedAppTutorialLayout
        providerId="telegram"
        providerName="Telegram"
        title="Connect Telegram"
        titleId="telegram-title"
        description={
          <p>
            Create a fresh bot dedicated to Alfred. It sends plain-text
            notifications only to your private Telegram chat.
          </p>
        }
        badge="Private chat"
        steps={[
          {
            title: "Create a dedicated bot",
            description: (
              <>
                <p>
                  Use <code>/newbot</code> in Telegram and keep this bot
                  separate from other automations.
                </p>
                <button
                  type="button"
                  className="ghost tutorial-wizard-step-link"
                  onClick={() => void openBotFather()}
                >
                  Open @BotFather <ExternalLinkIcon />
                </button>
              </>
            ),
          },
          {
            title: "Paste the BotFather token",
            description: (
              <p>
                Copy the token BotFather gives you and validate it below.
                Alfred rejects bots that already have a webhook.
              </p>
            ),
          },
          {
            title: "Start the bot and send a test",
            description: (
              <p>
                After validation, open the one-use pairing link, press
                <strong> Start</strong>, then return to Alfred.
              </p>
            ),
          },
        ]}
        onClose={() => void close()}
      >
        {openError ? (
          <p className="connection-tutorial-inline-error" role="alert">
            {openError}
          </p>
        ) : null}
        <label className="field">
          <span className="connection-tutorial-field-label">
            <span>BotFather token</span>
            <span className="connection-tutorial-required">required</span>
          </span>
          <input
            type="password"
            value={botToken}
            autoComplete="off"
            spellCheck={false}
            placeholder="123456789:AA…"
            onChange={(event) => setBotToken(event.currentTarget.value)}
          />
          <span className="connection-tutorial-input-hint">
            Validated with Telegram before it is saved.
          </span>
        </label>
        {error ? (
          <p className="app-action-warning" role="alert">
            {error.message}
          </p>
        ) : null}
        <p className="connection-tutorial-security-copy">
          The token goes directly to Rust, is cleared from this form after
          every attempt, and is saved only after the mandatory test succeeds.
        </p>
        <div className="schedule-actions">
          <button
            type="button"
            className="primary"
            disabled={loading || !botToken.trim()}
            onClick={() => void validateToken()}
          >
            {loading ? "Validating…" : "Validate bot"}
          </button>
        </div>
      </ConnectedAppTutorialLayout>
  );
}
