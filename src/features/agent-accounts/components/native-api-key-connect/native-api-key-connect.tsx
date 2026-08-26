import { useEffect, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useAgentAccountsStore } from "../../store";
import type { AgentProductId } from "../../types";

type NativeApiKeyCopy = {
  providerName: string;
  keyLabel: string;
  placeholder: string;
  description: string;
  billing: string;
};

const PROVIDER_COPY: Record<string, NativeApiKeyCopy> = {
  claude_code: {
    providerName: "Claude",
    keyLabel: "Anthropic API key",
    placeholder: "sk-ant-…",
    description:
      "Enter an Anthropic API key created for Claude API usage. Claude subscription sign-in is separate and is not imported.",
    billing: "Native runs are billed to the Anthropic API organization for this key.",
  },
  gemini: {
    providerName: "Gemini",
    keyLabel: "Gemini API key",
    placeholder: "Enter your Gemini API key",
    description:
      "Enter a Gemini API key created in Google AI Studio. Google account or Gemini subscription credentials are not imported.",
    billing: "Native runs are billed under the Google API project for this key.",
  },
  grok: {
    providerName: "Grok",
    keyLabel: "xAI API key",
    placeholder: "xai-…",
    description:
      "Enter an API key created in the xAI console. Grok subscription credentials are separate and are not imported.",
    billing: "Native runs are billed to the xAI API team for this key.",
  },
};

type NativeApiKeyConnectProps = {
  providerId: string;
  providerName: string;
  productId: AgentProductId;
  accountId?: string;
  onClose: () => void;
};

export function NativeApiKeyConnect({
  providerId,
  providerName,
  productId,
  accountId,
  onClose,
}: NativeApiKeyConnectProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [hasValue, setHasValue] = useState(false);
  const connectApiKey = useAgentAccountsStore((state) => state.connectApiKey);
  const busyId = useAgentAccountsStore((state) => state.busyId);
  const error = useAgentAccountsStore((state) => state.error);
  const providerCopy = Object.prototype.hasOwnProperty.call(
    PROVIDER_COPY,
    providerId,
  )
    ? PROVIDER_COPY[providerId]
    : undefined;
  const copy = providerCopy ?? {
    providerName,
    keyLabel: `${providerName} API key`,
    placeholder: `Enter your ${providerName} API key`,
    description: `Enter an API key issued for ${providerName}. Subscription and CLI credentials are separate and are not imported.`,
    billing: `Native runs are billed to the account that owns this ${providerName} API key.`,
  };
  const operationId = accountId ?? productId;
  const busy = busyId === operationId;

  const clearInput = () => {
    if (inputRef.current) inputRef.current.value = "";
    setHasValue(false);
  };

  useEffect(
    () => () => {
      if (inputRef.current) inputRef.current.value = "";
    },
    [],
  );

  const close = () => {
    clearInput();
    onClose();
  };

  const submit = async () => {
    const input = inputRef.current;
    if (!input || !input.value) return;
    let apiKey = input.value;
    clearInput();
    try {
      if (await connectApiKey(providerId, productId, apiKey, accountId)) onClose();
    } finally {
      apiKey = "";
    }
  };

  const title = `${accountId ? "Reconnect" : "Connect"} ${copy.providerName}`;
  const titleId = `native-api-key-${providerId}-title`;
  const descriptionId = `native-api-key-${providerId}-description`;

  return (
    <Modal
      size="md"
      className="compact-form-modal"
      onClose={close}
      labelledBy={titleId}
      describedBy={descriptionId}
      closeOnBackdrop={!busy}
      closeOnEscape={!busy}
    >
      <ModalHeader
        title={title}
        titleId={titleId}
        description={copy.description}
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
            <span>{copy.keyLabel}</span>
            <input
              ref={inputRef}
              type="password"
              autoFocus
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              placeholder={copy.placeholder}
              disabled={busy}
              onInput={(event) => {
                setHasValue(event.currentTarget.value.length > 0);
              }}
            />
          </label>
          <p className="settings-value">{copy.billing}</p>
          <p className="settings-value">
            Alfred sends this value directly to the native command, clears the
            field immediately, and stores the key only in your operating
            system credential store. It is never saved in account metadata.
          </p>
          {error ? (
            <p className="app-action-warning" role="alert">
              {error.message}
            </p>
          ) : null}
        </div>

        <footer className="compact-form-modal-footer">
          <button type="submit" className="primary" disabled={!hasValue || busy}>
            {busy ? "Saving…" : accountId ? "Replace key" : "Save key"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
