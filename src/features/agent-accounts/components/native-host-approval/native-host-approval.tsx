import { listen } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";

export type HostApprovalDecision = "once" | "always" | "reject";

export type HostApprovalPrompt = {
  requestId: string;
  sessionId?: string | null;
  permission: string;
  patterns: string[];
  alwaysPatterns: string[];
  toolCallId?: string | null;
};

type NativeHostApprovalDialogProps = {
  prompt: HostApprovalPrompt;
  busy?: boolean;
  onDecision: (decision: HostApprovalDecision) => void;
};

export function NativeHostApprovalContent({
  prompt,
  busy = false,
  onDecision,
}: NativeHostApprovalDialogProps) {
  const titleId = `native-host-approval-${prompt.requestId}-title`;
  const descriptionId = `native-host-approval-${prompt.requestId}-description`;
  const patterns =
    prompt.alwaysPatterns.length > 0 ? prompt.alwaysPatterns : prompt.patterns;

  return (
    <>
      <ModalHeader
        title="Allow this tool?"
        titleId={titleId}
        description="Alfred decides. The managed OpenCode runtime executes the tool only after an explicit choice."
        descriptionId={descriptionId}
      />
      <div className="compact-form-modal-body">
        <p className="settings-value">Permission: {prompt.permission}</p>
        {patterns.length > 0 ? (
          <p className="settings-value">Patterns: {patterns.join(", ")}</p>
        ) : null}
      </div>
      <footer className="compact-form-modal-footer">
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => onDecision("reject")}
        >
          Reject
        </button>
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => onDecision("once")}
        >
          Once
        </button>
        <button
          type="button"
          className="primary"
          disabled={busy}
          onClick={() => onDecision("always")}
        >
          Always
        </button>
      </footer>
    </>
  );
}

export function NativeHostApprovalDialog({
  prompt,
  busy = false,
  onDecision,
}: NativeHostApprovalDialogProps) {
  const titleId = `native-host-approval-${prompt.requestId}-title`;
  const descriptionId = `native-host-approval-${prompt.requestId}-description`;

  return (
    <Modal
      size="md"
      className="compact-form-modal native-host-approval-modal"
      role="alertdialog"
      labelledBy={titleId}
      describedBy={descriptionId}
      closeOnBackdrop={!busy}
      closeOnEscape={!busy}
      onClose={() => {
        if (!busy) onDecision("reject");
      }}
    >
      <NativeHostApprovalContent
        prompt={prompt}
        busy={busy}
        onDecision={onDecision}
      />
    </Modal>
  );
}

export function NativeHostApproval() {
  const [prompt, setPrompt] = useState<HostApprovalPrompt | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    void listen<HostApprovalPrompt>("native://approval-requested", (event) => {
      if (!event.payload?.requestId || !event.payload.permission) return;
      setPrompt(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const decide = async (decision: HostApprovalDecision) => {
    if (!prompt || busy) return;
    setBusy(true);
    try {
      await invoke("resolve_native_approval", {
        requestId: prompt.requestId,
        decision,
      });
      setPrompt(null);
    } finally {
      setBusy(false);
    }
  };

  if (!prompt) return null;
  return (
    <NativeHostApprovalDialog
      prompt={prompt}
      busy={busy}
      onDecision={(decision) => {
        void decide(decision);
      }}
    />
  );
}
