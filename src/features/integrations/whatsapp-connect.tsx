import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/icon";
import { Modal, ModalHeader } from "../../components/modal";
import { AppLogo } from "./app-logo";
import { integrationsApi } from "./api";
import type { ConnectDialogProps } from "./connect-dialog";
import { useIntegrationsStore } from "./store";
import type { WhatsAppPairingState, WhatsAppQr } from "./types";

const DEFAULT_TEST_MESSAGE =
  "Alfred is linked and ready to send notifications to this chat.";

/** How often the modal re-reads pairing state. The QR itself arrives by event;
 * this only tracks the state machine's own transitions. */
const POLL_INTERVAL_MS = 700;

/** Failures that end the attempt. The user must start over. */
const TERMINAL_FAILURES = new Set([
  "already_linked",
  "invalid_identity",
  "relink_required",
  "test_delivery_unknown",
  "pairing_incomplete",
]);

const FAILURE_COPY: Record<string, string> = {
  acknowledgement_required: "Accept the warning before linking a device.",
  acknowledgement_outdated:
    "The warning has changed. Read it again before linking a device.",
  already_linked:
    "A WhatsApp account is already linked. Disconnect it before linking another.",
  invalid_identity:
    "WhatsApp did not return a personal account. Only a personal account can be linked.",
  relink_required:
    "WhatsApp unlinked this device. Start again to link a new one.",
  test_delivery_unknown:
    "The test message may have arrived, but WhatsApp did not confirm it. Check your own chat. Alfred did not keep this connection.",
  pairing_incomplete:
    "The pairing code expired before it was scanned. Start again for a new code.",
  runtime_unavailable: "Alfred could not reach WhatsApp. Try again.",
  storage_unavailable: "Alfred could not prepare secure local storage.",
  credentials_unavailable:
    "Alfred could not reach the system credential store. Unlock it and try again.",
};

export function WhatsAppConnect({ onClose }: ConnectDialogProps) {
  const load = useIntegrationsStore((state) => state.load);
  const [acknowledged, setAcknowledged] = useState(false);
  const [pairing, setPairing] = useState<WhatsAppPairingState | null>(null);
  const [qr, setQr] = useState<WhatsAppQr | null>(null);
  const [testMessage, setTestMessage] = useState(DEFAULT_TEST_MESSAGE);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const startedRef = useRef(false);

  // The QR arrives already rendered; a superseded code is cleared immediately so
  // the modal never shows something that can no longer be scanned.
  useEffect(() => {
    const unlisten = Promise.all([
      listen<WhatsAppQr>("whatsapp://qr", (event) => setQr(event.payload)),
      listen("whatsapp://qr-expired", () => setQr(null)),
    ]);
    return () => {
      void unlisten.then((handlers) =>
        handlers.forEach((stop) => {
          stop();
        }),
      );
    };
  }, []);

  // Closing the modal always abandons the attempt: Rust stops the runtime,
  // attempts a remote logout if the device linked, and deletes staging state.
  useEffect(() => {
    return () => {
      if (startedRef.current) void integrationsApi.cancelWhatsappPairing();
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      setPairing(await integrationsApi.whatsappPairingState());
    } catch {
      // A dropped poll is not worth surfacing; the next tick recovers.
    }
  }, []);

  useEffect(() => {
    if (!startedRef.current) return;
    if (pairing?.state === "ready" || pairing?.state === "failed") return;
    const timer = setInterval(() => void refresh(), POLL_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [pairing?.state, refresh]);

  async function begin() {
    setBusy(true);
    setError(null);
    try {
      const state = await integrationsApi.beginWhatsappPairing("1");
      startedRef.current = true;
      setPairing(state);
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  }

  async function sendTest() {
    setBusy(true);
    setError(null);
    try {
      await integrationsApi.sendWhatsappPairingTest(testMessage);
      await refresh();
    } catch (cause) {
      setError(describe(cause));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function finish() {
    setBusy(true);
    setError(null);
    try {
      await integrationsApi.completeWhatsappPairing();
      startedRef.current = false;
      await load();
      onClose();
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  }

  const failure = pairing?.failureCode ?? null;
  const terminal = failure !== null && TERMINAL_FAILURES.has(failure);

  return (
    <Modal
      onClose={onClose}
      labelledBy="whatsapp-title"
      describedBy="whatsapp-description"
    >
      <ModalHeader
        leading={
          <AppLogo providerId="whatsapp" providerName="WhatsApp" size={40} />
        }
        title="Connect WhatsApp"
        titleId="whatsapp-title"
        description="Scan the QR code to link one private chat for workflow notifications."
        descriptionId="whatsapp-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      <div className="schedule-modal-body">
        <p className="app-action-warning" role="note">
          <strong>Experimental and unofficial.</strong> This uses an
          unofficial reimplementation of the WhatsApp Web protocol, not an API
          offered by Meta. Protocol changes can break it at any time, and
          WhatsApp may restrict or suspend the linked account. Alfred cannot
          guarantee your account&rsquo;s safety.
        </p>
        <p className="settings-value">
          Alfred sends plain text to your own &ldquo;Message yourself&rdquo;
          chat, and only while Alfred is running. It never reads your messages,
          history, or contacts, and it cannot send to anyone else.
        </p>

        {!startedRef.current && (
          <>
            <label className="field whatsapp-acknowledge">
              <input
                type="checkbox"
                checked={acknowledged}
                onChange={(event) => setAcknowledged(event.target.checked)}
              />
              <span>
                I understand this is unofficial and that my WhatsApp account
                could be restricted or suspended.
              </span>
            </label>
            <button
              type="button"
              className="primary"
              disabled={!acknowledged || busy}
              onClick={() => void begin()}
            >
              {busy ? "Starting…" : "Link a device"}
            </button>
          </>
        )}

        {startedRef.current && pairing?.state === "awaiting_scan" && (
          <div className="telegram-pairing-card whatsapp-qr">
            {qr ? (
              <>
                {/* Rust renders the code; the scannable payload never exists
                    here as text. */}
                <div
                  className="whatsapp-qr-code"
                  role="img"
                  aria-label="WhatsApp pairing QR code"
                  dangerouslySetInnerHTML={{ __html: qr.svg }}
                />
                <p className="settings-value">
                  On your phone, open <strong>WhatsApp</strong> →{" "}
                  <strong>Linked Devices</strong> →{" "}
                  <strong>Link a device</strong>, then scan this code. It
                  expires in about {qr.expiresInSeconds} seconds and refreshes
                  itself.
                </p>
              </>
            ) : (
              <p className="settings-value">Preparing a pairing code…</p>
            )}
          </div>
        )}

        {pairing?.state === "starting" && (
          <p className="settings-value">Starting the WhatsApp connection…</p>
        )}

        {pairing?.state === "awaiting_test" && (
          <>
            <p className="settings-value">
              Linked to <strong>{pairing.maskedAccount}</strong>. Send a test
              message to confirm it works. Alfred keeps the connection only if
              this succeeds.
            </p>
            <label className="field">
              <span className="settings-label">Test message</span>
              <textarea
                value={testMessage}
                onChange={(event) => setTestMessage(event.target.value)}
                rows={3}
                maxLength={4096}
              />
            </label>
            <button
              type="button"
              className="primary"
              disabled={busy || !testMessage.trim()}
              onClick={() => void sendTest()}
            >
              {busy ? "Sending…" : "Send test message"}
            </button>
          </>
        )}

        {pairing?.state === "ready" && (
          <>
            <p className="settings-value">
              The test message was submitted to{" "}
              <strong>{pairing.maskedAccount}</strong>. Check your own WhatsApp
              chat, then finish.
            </p>
            <button
              type="button"
              className="primary"
              disabled={busy}
              onClick={() => void finish()}
            >
              {busy ? "Finishing…" : "Finish"}
            </button>
          </>
        )}

        {failure && (
          <p className="connection-tutorial-inline-error" role="alert">
            {FAILURE_COPY[failure] ?? "WhatsApp pairing failed."}
          </p>
        )}
        {error && (
          <p className="connection-tutorial-inline-error" role="alert">
            {error}
          </p>
        )}
        {terminal && (
          <button
            type="button"
            className="ghost"
            onClick={onClose}
            disabled={busy}
          >
            Close
          </button>
        )}
      </div>
    </Modal>
  );
}

function describe(cause: unknown): string {
  if (typeof cause === "object" && cause !== null && "code" in cause) {
    const code = String((cause as { code: unknown }).code);
    return FAILURE_COPY[code] ?? "WhatsApp pairing failed.";
  }
  return "WhatsApp pairing failed.";
}
