import { expect, test } from "bun:test";
import {
  createManagedRuntimeApi,
  type ManagedRuntimeInvoke,
} from "./managed-runtime-api";
import type { ManagedRuntimeConnectionStatus } from "./managed-runtime-types";

const status: ManagedRuntimeConnectionStatus = {
  providerId: "opencode",
  productId: "opencode_go",
  installState: "ready",
  connectionState: "connected",
  accountId: "account_opaque",
  entitlementState: "eligible",
  lastErrorCode: null,
};

const product = {
  providerId: "opencode",
  productId: "opencode_go" as const,
  productName: "OpenCode Go",
  runtimeId: "opencode_server" as const,
  runtimeVersion: "1.18.23",
  installState: "ready",
  connectionKind: "api_key",
  connectAvailable: true,
  gateCodes: [],
  billingSource: "provider_subscription",
  custodyMode: "runtime_managed" as const,
};

test("uses the exact managed-runtime command names and camelCase arguments", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invokeCommand: ManagedRuntimeInvoke = async <T>(command, args) => {
    calls.push({ command, args });
    if (command === "list_managed_runtime_products") return [product] as T;
    if (command === "prepare_managed_runtime_product") return product as T;
    if (command === "start_managed_runtime_connection") {
      return {
        kind: "browser",
        attemptId: null,
        authorizationUrl: "https://chatgpt.com/auth",
        userCode: null,
        expiresAt: null,
        terminalSessionId: null,
      } as T;
    }
    if (command === "read_managed_runtime_terminal") {
      return {
        sessionId: "terminal-opaque",
        sequence: 3,
        dataBase64: "b3V0cHV0",
      } as T;
    }
    return status as T;
  };
  const api = createManagedRuntimeApi(invokeCommand);

  await api.listProducts();
  await api.prepareProduct("claude_code", "claude_code_subscription");
  await api.startConnection("codex", "chatgpt_codex");
  await api.connectionStatus("opencode", "opencode_go");
  await api.connectApiKey("opencode", "opencode_go", "go-key-secret");
  await api.readTerminal("terminal-opaque", 3);
  await api.writeTerminal("terminal-opaque", "input-secret");
  await api.resizeTerminal("terminal-opaque", 120, 32);
  await api.closeTerminal("terminal-opaque");

  expect(calls).toEqual([
    { command: "list_managed_runtime_products", args: undefined },
    {
      command: "prepare_managed_runtime_product",
      args: { providerId: "claude_code", productId: "claude_code_subscription" },
    },
    {
      command: "start_managed_runtime_connection",
      args: { providerId: "codex", productId: "chatgpt_codex" },
    },
    {
      command: "managed_runtime_connection_status",
      args: { providerId: "opencode", productId: "opencode_go" },
    },
    {
      command: "connect_managed_runtime_api_key",
      args: {
        providerId: "opencode",
        productId: "opencode_go",
        apiKey: "go-key-secret",
      },
    },
    {
      command: "read_managed_runtime_terminal",
      args: { sessionId: "terminal-opaque", cursor: 3 },
    },
    {
      command: "write_managed_runtime_terminal",
      args: { sessionId: "terminal-opaque", input: "input-secret" },
    },
    {
      command: "resize_managed_runtime_terminal",
      args: { sessionId: "terminal-opaque", cols: 120, rows: 32 },
    },
    {
      command: "close_managed_runtime_terminal",
      args: { sessionId: "terminal-opaque" },
    },
  ]);
});

test("rejects managed API-key intake for every product except OpenCode Go", async () => {
  const api = createManagedRuntimeApi(async () => status);
  await expect(
    api.connectApiKey("claude_code", "claude_code_subscription", "secret"),
  ).rejects.toThrow("managed_runtime_api_key_product_invalid");
});
