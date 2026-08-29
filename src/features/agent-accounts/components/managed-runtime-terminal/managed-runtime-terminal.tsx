import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import {
  managedRuntimeApi,
  type ManagedRuntimeApi,
} from "../../managed-runtime-api";

type ManagedRuntimeTerminalProps = {
  productName: string;
  sessionId: string;
  onClose: () => void;
  api?: ManagedRuntimeApi;
};

/**
 * The PTY is provider-owned. Output is appended directly to the accessible
 * log so it is never parsed or retained in Zustand (and no auth text is
 * interpreted by Alfred).
 */
export function ManagedRuntimeTerminal({
  productName,
  sessionId,
  onClose,
  api = managedRuntimeApi,
}: ManagedRuntimeTerminalProps) {
  const outputRef = useRef<HTMLPreElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const cursorRef = useRef(0);
  const closedRef = useRef(false);
  const [closed, setClosed] = useState(false);
  const [error, setError] = useState(false);

  const appendOutput = useCallback((encodedOutput: string) => {
    if (!encodedOutput || !outputRef.current) return;
    const bytes = decodeTerminalBytes(encodedOutput);
    outputRef.current.append(document.createTextNode(bytes));
    outputRef.current.scrollTop = outputRef.current.scrollHeight;
  }, []);

  useEffect(() => {
    // Each effect run owns its own sequential read chain. A ref-based in-flight
    // guard deadlocks under StrictMode's double mount: the remount sees the
    // first run's in-flight read, returns without scheduling, and the cancelled
    // first run never reschedules either, so the terminal stays blank forever.
    let cancelled = false;
    let timer = 0;
    const read = async () => {
      if (cancelled || closedRef.current) return;
      try {
        const chunk = await api.readTerminal(sessionId, cursorRef.current);
        if (cancelled) return;
        cursorRef.current = chunk.cursor;
        appendOutput(chunk.output);
        if (chunk.closed) {
          closedRef.current = true;
          setClosed(true);
          return;
        }
      } catch {
        if (!cancelled) setError(true);
      }
      if (!cancelled && !closedRef.current) {
        timer = window.setTimeout(() => void read(), 160);
      }
    };
    void read();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [api, appendOutput, sessionId]);

  useEffect(() => {
    void api.resizeTerminal(sessionId, 120, 32).catch(() => {
      // A resize is best effort while the provider-owned terminal starts.
    });
    return () => {
      if (!closedRef.current) {
        void api.closeTerminal(sessionId).catch(() => {});
      }
    };
  }, [api, sessionId]);

  const sendInput = () => {
    const input = inputRef.current;
    if (!input || !input.value) return;
    let transientInput = input.value;
    input.value = "";
    void api.writeTerminal(sessionId, transientInput).catch(() => {
      setError(true);
    }).finally(() => {
      transientInput = "";
    });
  };

  const close = () => {
    closedRef.current = true;
    if (!closed) void api.closeTerminal(sessionId).catch(() => {});
    onClose();
  };

  const titleId = `managed-runtime-terminal-${sessionId}-title`;
  const descriptionId = `managed-runtime-terminal-${sessionId}-description`;

  return (
    <Modal
      size="settings"
      className="managed-runtime-terminal-modal"
      onClose={close}
      labelledBy={titleId}
      describedBy={descriptionId}
      closeOnBackdrop={false}
      closeOnEscape
    >
      <ModalHeader
        title={`${productName} sign-in`}
        titleId={titleId}
        description="Finish Claude sign-in in this window. You do not install a CLI."
        descriptionId={descriptionId}
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close terminal"
            onClick={close}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />
      <div className="managed-runtime-terminal-body">
        <pre
          ref={outputRef}
          className="managed-runtime-terminal-output"
          role="log"
          aria-live="polite"
          aria-label={`${productName} provider terminal output`}
          tabIndex={0}
        />
        <label className="managed-runtime-terminal-input-label">
          <span>Terminal input</span>
          <textarea
            ref={inputRef}
            autoFocus
            rows={2}
            aria-label={`${productName} terminal input`}
            placeholder={closed ? "Terminal closed" : "Type a response…"}
            disabled={closed}
            onChange={sendInput}
          />
        </label>
        {error ? (
          <p className="managed-runtime-inline-error" role="alert">
            The provider terminal could not be reached. Close it and try again.
          </p>
        ) : null}
        {closed ? (
          <p className="managed-runtime-terminal-closed" role="status">
            The provider terminal has closed. You can close this dialog.
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

function decodeTerminalBytes(encodedOutput: string): string {
  try {
    const binary = atob(encodedOutput);
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    // A development fixture may provide plain text; relay it unchanged.
    return encodedOutput;
  }
}

export { decodeTerminalBytes };
