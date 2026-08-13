import { useEffect, useState } from "react";
import type { AgentAuthToast } from "./toast-store";
import { useToastStore } from "./toast-store";

function AuthToastCard({ toast }: { toast: AgentAuthToast }) {
  const dismissToast = useToastStore((state) => state.dismissToast);
  const [copyFailed, setCopyFailed] = useState(false);

  useEffect(() => setCopyFailed(false), [toast.loginCommand]);

  const copyCommand = async () => {
    try {
      if (!navigator.clipboard?.writeText) {
        setCopyFailed(true);
        return;
      }
      await navigator.clipboard.writeText(toast.loginCommand);
      setCopyFailed(false);
    } catch {
      setCopyFailed(true);
    }
  };

  return (
    <section className="auth-toast" role="alert">
      <div className="auth-toast-header">
        <div>
          <p className="auth-toast-eyebrow">Authentication required</p>
          <h2>{toast.label}</h2>
        </div>
        <button
          type="button"
          className="auth-toast-dismiss"
          aria-label={`Dismiss ${toast.label} authentication notice`}
          onClick={() => dismissToast(toast.id)}
        >
          ×
        </button>
      </div>
      <p className="auth-toast-instructions">
        {toast.workflowName ? `${toast.workflowName} needs` : "This workflow needs"}{" "}
        you to sign in from a terminal, then retry the run.
      </p>
      <code className="auth-toast-command user-select-text">
        {toast.loginCommand}
      </code>
      <div className="auth-toast-actions">
        <button type="button" onClick={() => void copyCommand()}>
          Copy command
        </button>
        {copyFailed ? (
          <span className="auth-toast-copy-fallback">
            Select and copy manually.
          </span>
        ) : null}
      </div>
    </section>
  );
}

export function ToastViewport() {
  const toasts = useToastStore((state) => state.toasts);

  return (
    <div
      className="toast-viewport"
      aria-live="assertive"
      aria-label="Authentication notices"
    >
      {toasts.map((toast) => (
        <AuthToastCard key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
