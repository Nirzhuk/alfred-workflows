import { beforeEach, describe, expect, test } from "bun:test";
import { useToastStore } from "../src/components/toast/toast-store";
import type { AgentAuthRequired } from "../src/features/workflow/types";

const codexAuth: AgentAuthRequired = {
  provider: "codex",
  label: "Codex",
  loginCommand: "codex login",
};

describe("agent auth toast store", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
  });

  test("creates a persistent provider toast", () => {
    useToastStore.getState().showAgentAuthToast(codexAuth, "Release flow");

    expect(useToastStore.getState().toasts).toEqual([
      {
        id: "agent-auth:codex",
        ...codexAuth,
        workflowName: "Release flow",
      },
    ]);
  });

  test("replaces the same provider in place without growing", () => {
    const store = useToastStore.getState();
    store.showAgentAuthToast(codexAuth, "First workflow");
    store.showAgentAuthToast(
      { ...codexAuth, label: "Latest Codex", loginCommand: "codex login now" },
      "Latest workflow",
    );

    expect(useToastStore.getState().toasts).toEqual([
      {
        id: "agent-auth:codex",
        provider: "codex",
        label: "Latest Codex",
        loginCommand: "codex login now",
        workflowName: "Latest workflow",
      },
    ]);
  });

  test("allows different providers to coexist", () => {
    const store = useToastStore.getState();
    store.showAgentAuthToast(codexAuth);
    store.showAgentAuthToast({
      provider: "claude_code",
      label: "Claude Code",
      loginCommand: "claude auth login",
    });

    expect(useToastStore.getState().toasts.map((toast) => toast.id)).toEqual([
      "agent-auth:codex",
      "agent-auth:claude_code",
    ]);
  });

  test("dismisses a toast explicitly", () => {
    useToastStore.getState().showAgentAuthToast(codexAuth);
    useToastStore.getState().dismissToast("agent-auth:codex");

    expect(useToastStore.getState().toasts).toEqual([]);
  });
});
