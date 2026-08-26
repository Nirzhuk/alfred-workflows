import { useEffect, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useManagedRuntimeStore } from "../../managed-runtime-store";
import type { AgentProductId } from "../../types";

type ManagedRuntimeApiKeyProps = {
  providerId: string;
  productId: AgentProductId;
  productName: string;
  onClose: () => void;
  onConnected: () => void;
};

/** OpenCode Go's key is a one-way transient input, never account metadata. */
export function ManagedRuntimeApiKey({
  providerId,
  productId,
  productName,
  onClose,
  onConnected,
}: ManagedRuntimeApiKeyProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [hasValue, setHasValue] = useState(false);
  const connectApiKey = useManagedRuntimeStore((state) => state.connectApiKey);
  const busy = useManagedRuntimeStore(
    (state) => state.connectingId === `${providerId}:${productId}`,
  );
  const error = useManagedRuntimeStore((state) => state.error);

  useEffect(
    () => () => {
      if (inputRef.current) inputRef.current.value = "";
    },
    [],
  );

  const clearInput = () => {
    if (inputRef.current) inputRef.current.value = "";
    setHasValue(false);
  };

  const close = () => {
    clearInput();
    onClose();
  };

  const submit = async () => {
    const input = inputRef.current;
    if (!input?.value) return;
    let transientKey = input.value;
    clearInput();
    try {
      if (await connectApiKey(providerId, productId, transientKey)) {
        onConnected();
        onClose();
      }
    } finally {
      transientKey = "";
    }
  };

  const titleId = `managed-runtime-api-key-${productId}-title`;
  const descriptionId = `managed-runtime-api-key-${productId}-description`;

  return (
    <Modal
      size="md"
      className="compact-form-modal managed-runtime-api-key-modal"
      onClose={close}
      labelledBy={titleId}
      describedBy={descriptionId}
      closeOnBackdrop={!busy}
      closeOnEscape={!busy}
    >
      <ModalHeader
        title={`Connect ${productName}`}
        titleId={titleId}
        description="OpenCode Go uses its own subscription key. Alfred sends it directly to the managed runtime and does not retain it."
        descriptionId={descriptionId}
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            disabled={busy}
            onClick={close}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />
      <form
        className="compact-form-modal-form"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <div className="compact-form-modal-body">
          <label className="field">
            <span>OpenCode Go subscription key</span>
            <input
              ref={inputRef}
              type="password"
              autoFocus
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              placeholder="Enter your OpenCode Go key"
              disabled={busy}
              onInput={(event) => {
                setHasValue(event.currentTarget.value.length > 0);
              }}
            />
          </label>
          <p className="settings-value">
            OpenCode Go usage is billed to the provider subscription associated
            with this key. It is not exchanged for OpenCode Zen or another API
            product.
          </p>
          <p className="settings-value">
            The field clears before Alfred sends the value. Only the managed
            provider runtime receives it.
          </p>
          {error ? (
            <p className="managed-runtime-inline-error" role="alert">
              {error.message}
            </p>
          ) : null}
        </div>
        <footer className="compact-form-modal-footer">
          <button type="submit" className="primary" disabled={!hasValue || busy}>
            {busy ? "Connecting…" : "Connect"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

