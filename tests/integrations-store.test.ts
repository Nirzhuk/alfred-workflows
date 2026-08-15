import { describe, expect, test } from "bun:test";
import { createStore } from "zustand/vanilla";
import type { IntegrationsApi } from "../src/features/integrations/api";
import {
  createIntegrationsState,
  normalizeIntegrationError,
} from "../src/features/integrations/store";
import type {
  AppConnection,
  AppProvider,
} from "../src/features/integrations/types";

const provider: AppProvider = {
  id: "slack",
  name: "Slack",
  capabilitySummary: "Messages and channel activity",
  connectionModes: ["native_oauth"],
  connectAvailable: false,
};

function connection(name = "Workspace"): AppConnection {
  return {
    id: "connection-1",
    providerId: "slack",
    displayName: name,
    externalAccountId: null,
    externalTenantId: null,
    connectionMode: "native_oauth",
    scopes: ["channels:read"],
    status: "connected",
    expiresAt: null,
    lastCheckedAt: null,
    lastErrorCode: null,
    createdAt: "2026-08-13T10:00:00Z",
    updatedAt: "2026-08-13T10:00:00Z",
  };
}

function api(overrides: Partial<IntegrationsApi> = {}): IntegrationsApi {
  return {
    listProviders: async () => [provider],
    listConnections: async () => [connection()],
    getConnection: async () => connection(),
    getUsage: async () => ({ workflows: [], schedules: [], triggers: [] }),
    disconnect: async () => {},
    listActionDescriptors: async () => [],
    listActionResources: async () => ({ items: [], nextPageToken: null }),
    listEventDescriptors: async () => [],
    listEventResources: async () => ({ items: [], nextPageToken: null }),
    connectSlackPrivate: async () => connection(),
    ...overrides,
  };
}

describe("Connected Apps store", () => {
  test("loads providers and redacted connection metadata", async () => {
    const unsafe = {
      ...connection(),
      accessToken: "access-secret-fixture",
      credentialRef: "credential-secret-fixture",
    } as AppConnection;
    const store = createStore(
      createIntegrationsState(api({ listConnections: async () => [unsafe] })),
    );

    expect(await store.getState().load()).toBe(true);
    expect(store.getState().providers).toEqual([provider]);
    expect(store.getState().connections).toHaveLength(1);
    const serialized = JSON.stringify(store.getState().connections);
    expect(serialized).not.toContain("access-secret-fixture");
    expect(serialized).not.toContain("credential-secret-fixture");
  });

  test("refresh replaces stale metadata", async () => {
    let reads = 0;
    const store = createStore(
      createIntegrationsState(
        api({
          listConnections: async () => [connection(reads++ === 0 ? "Old" : "New")],
        }),
      ),
    );

    await store.getState().load();
    expect(store.getState().connections[0]?.displayName).toBe("Old");
    expect(await store.getState().refresh()).toBe(true);
    expect(store.getState().connections[0]?.displayName).toBe("New");
  });

  test("removes a connection only after disconnect succeeds", async () => {
    let resolveDisconnect: (() => void) | undefined;
    const store = createStore(
      createIntegrationsState(
        api({
          disconnect: () =>
            new Promise<void>((resolve) => {
              resolveDisconnect = resolve;
            }),
        }),
      ),
    );
    await store.getState().load();

    const pending = store.getState().disconnect("connection-1");
    expect(store.getState().connections).toHaveLength(1);
    resolveDisconnect?.();
    expect(await pending).toBeNull();
    expect(store.getState().connections).toHaveLength(0);
  });

  test("keeps failed disconnects and never displays raw backend details", async () => {
    const store = createStore(
      createIntegrationsState(
        api({
          disconnect: async () => {
            throw {
              code: "credential_store_locked",
              message: "raw provider failure access-secret-fixture",
              recoverable: true,
            };
          },
          getConnection: async () => ({
            ...connection(),
            status: "revoked",
          }),
        }),
      ),
    );
    await store.getState().load();

    const error = await store.getState().disconnect("connection-1");
    expect(error?.code).toBe("credential_store_locked");
    expect(error?.message).not.toContain("access-secret-fixture");
    expect(store.getState().connections[0]?.status).toBe("revoked");
  });

  test("normalizes unknown errors to a stable safe message", () => {
    const error = normalizeIntegrationError(
      "Bearer access-secret-fixture from provider",
    );
    expect(error.code).toBe("integration_failed");
    expect(error.message).not.toContain("access-secret-fixture");
  });

  test("submits a Slack secret without retaining it in store state", async () => {
    let submittedToken = "";
    const store = createStore(
      createIntegrationsState(
        api({
          connectSlackPrivate: async (input) => {
            submittedToken = input.botToken;
            return {
              ...connection(),
              connectionMode: "private_bot",
              scopes: ["chat:write", "channels:read"],
            };
          },
        }),
      ),
    );

    expect(
      await store.getState().connectSlackPrivate({
        mode: "bot",
        botToken: "xoxb-secret-fixture",
        appToken: null,
        webhookUrl: null,
        enablePrivateChannels: false,
        enableMentions: false,
      }),
    ).toBeNull();
    expect(submittedToken).toBe("xoxb-secret-fixture");
    expect(JSON.stringify(store.getState())).not.toContain("xoxb-secret-fixture");
  });
});
