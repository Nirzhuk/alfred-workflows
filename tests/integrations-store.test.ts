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
    prepareGithubConnection: async () => ({
      pairingSessionId: "github-pairing-session",
      userCode: "ABCD-EFGH",
      verificationUri: "https://github.com/login/device",
      installationUrl: "https://github.com/apps/alfred/installations/new",
      expiresAt: "2026-08-13T10:10:00Z",
      intervalSeconds: 5,
    }),
    pollGithubConnection: async () => ({
      status: "connected",
      connection: {
        ...connection("@octocat"),
        providerId: "github",
        connectionMode: "github_app_device",
        scopes: ["metadata:read", "issues:write", "pull_requests:write"],
      },
    }),
    cancelGithubPairing: async () => {},
    prepareGmailConnection: async () => ({
      sessionId: "gmail-session",
      authorizationUrl: "https://accounts.google.com/o/oauth2/v2/auth",
      expiresAt: "2026-08-13T10:10:00Z",
    }),
    completeGmailConnection: async () => ({
      ...connection("user@example.com"),
      providerId: "gmail",
      connectionMode: "native_oauth",
      scopes: ["https://www.googleapis.com/auth/gmail.send"],
    }),
    cancelGmailAuthorization: async () => {},
    prepareMicrosoftConnection: async () => ({
      sessionId: "microsoft-session",
      authorizationUrl: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
      expiresAt: "2026-08-13T10:10:00Z",
    }),
    completeMicrosoftConnection: async () => ({
      ...connection("Ada Lovelace"),
      providerId: "microsoft",
      connectionMode: "native_oauth",
      scopes: ["Mail.Send", "Mail.ReadBasic"],
    }),
    cancelMicrosoftAuthorization: async () => {},
    connectNotionPrivate: async () => ({
      ...connection("Product workspace"),
      providerId: "notion",
      connectionMode: "private_bot",
      scopes: ["search", "read_content"],
    }),
    connectObsidianVault: async () => ({
      ...connection("Knowledge"),
      providerId: "obsidian",
      connectionMode: "local_vault",
      scopes: ["vault:search_notes", "vault:read_notes"],
    }),
    prepareTelegramConnection: async () => ({
      pairingSessionId: "pairing-session",
      botUsername: "alfred_fixture_bot",
      pairingUrl:
        "https://t.me/alfred_fixture_bot?start=pairing-nonce-fixture",
      expiresAt: "2026-08-13T10:10:00Z",
    }),
    completeTelegramConnection: async () => ({
      ...connection("Alfred Bot → private chat ••••1234"),
      providerId: "telegram",
      connectionMode: "private_bot",
      scopes: [],
    }),
    cancelTelegramPairing: async () => {},
    beginWhatsappPairing: async () => ({
      state: "awaiting_acknowledgement",
      maskedAccount: null,
      failureCode: null,
      acknowledgementVersion: "1",
    }),
    whatsappPairingState: async () => ({
      state: "awaiting_acknowledgement",
      maskedAccount: null,
      failureCode: null,
      acknowledgementVersion: "1",
    }),
    sendWhatsappPairingTest: async () => ({
      messageId: "test-message",
      submittedAt: "2026-08-13T10:00:00Z",
      maskedDestination: "••••1234",
    }),
    completeWhatsappPairing: async () => ({
      ...connection("WhatsApp"),
      providerId: "whatsapp",
      connectionMode: "linked_device_experimental",
      scopes: [],
    }),
    cancelWhatsappPairing: async () => {},
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

  test("submits a Notion token without retaining it in store state", async () => {
    let submittedToken = "";
    const store = createStore(
      createIntegrationsState(
        api({
          connectNotionPrivate: async (input) => {
            submittedToken = input.integrationToken;
            return {
              ...connection("Product workspace"),
              providerId: "notion",
              connectionMode: "private_bot",
              scopes: ["search", "read_content"],
            };
          },
        }),
      ),
    );

    expect(
      await store.getState().connectNotionPrivate({
        integrationToken: "ntn_notion-token-secret-fixture",
      }),
    ).toBeNull();
    expect(submittedToken).toBe("ntn_notion-token-secret-fixture");
    expect(JSON.stringify(store.getState())).not.toContain(
      "notion-token-secret-fixture",
    );
  });

  test("submits an Obsidian path without retaining it in store state", async () => {
    let submittedPath = "";
    const store = createStore(
      createIntegrationsState(
        api({
          connectObsidianVault: async (input) => {
            submittedPath = input.vaultPath;
            return {
              ...connection("Knowledge"),
              providerId: "obsidian",
              connectionMode: "local_vault",
              scopes: ["vault:search_notes", "vault:read_notes"],
            };
          },
        }),
      ),
    );

    expect(
      await store.getState().connectObsidianVault({
        vaultPath: "/private/example/Knowledge",
      }),
    ).toBeNull();
    expect(submittedPath).toBe("/private/example/Knowledge");
    expect(JSON.stringify(store.getState())).not.toContain(
      "/private/example/Knowledge",
    );
  });

  test("keeps GitHub device authorization data ephemeral and stores only redacted metadata", async () => {
    let polledSession = "";
    const store = createStore(
      createIntegrationsState(
        api({
          pollGithubConnection: async (pairingSessionId) => {
            polledSession = pairingSessionId;
            return {
              status: "connected",
              connection: {
                ...connection("@octocat"),
                providerId: "github",
                connectionMode: "github_app_device",
                scopes: ["metadata:read", "issues:write"],
              },
            };
          },
        }),
      ),
    );

    const pairing = await store.getState().prepareGithubConnection();
    expect(pairing?.verificationUri).toBe("https://github.com/login/device");
    expect(JSON.stringify(store.getState())).not.toContain("ABCD-EFGH");
    const result = await store
      .getState()
      .pollGithubConnection(pairing!.pairingSessionId);
    expect(result?.status).toBe("connected");
    expect(polledSession).toBe("github-pairing-session");
    expect(store.getState().connections).toEqual([
      expect.objectContaining({
        providerId: "github",
        displayName: "@octocat",
      }),
    ]);
  });

  test("does not retain the Telegram token or pairing nonce in store state", async () => {
    let submittedToken = "";
    let completedSession = "";
    const store = createStore(
      createIntegrationsState(
        api({
          prepareTelegramConnection: async (input) => {
            submittedToken = input.botToken;
            return {
              pairingSessionId: "pairing-session-fixture",
              botUsername: "alfred_fixture_bot",
              pairingUrl:
                "https://t.me/alfred_fixture_bot?start=nonce-secret-fixture",
              expiresAt: "2026-08-13T10:10:00Z",
            };
          },
          completeTelegramConnection: async (input) => {
            completedSession = input.pairingSessionId;
            return {
              ...connection("Alfred Bot → private chat ••••1234"),
              providerId: "telegram",
              connectionMode: "private_bot",
              scopes: [],
            };
          },
        }),
      ),
    );

    const pairing = await store.getState().prepareTelegramConnection({
      botToken: "123456:telegram-token-secret-fixture",
    });
    expect(submittedToken).toBe("123456:telegram-token-secret-fixture");
    expect(pairing?.pairingSessionId).toBe("pairing-session-fixture");
    expect(JSON.stringify(store.getState())).not.toContain(
      "telegram-token-secret-fixture",
    );
    expect(JSON.stringify(store.getState())).not.toContain(
      "nonce-secret-fixture",
    );

    expect(
      await store.getState().completeTelegramConnection({
        pairingSessionId: pairing!.pairingSessionId,
        testMessage: "explicit test",
      }),
    ).toBeNull();
    expect(completedSession).toBe("pairing-session-fixture");
    expect(store.getState().connections).toEqual([
      expect.objectContaining({
        providerId: "telegram",
        displayName: "Alfred Bot → private chat ••••1234",
      }),
    ]);
  });
});
