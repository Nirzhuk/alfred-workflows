import { expect, test } from "bun:test";
import type { AgentAccountsApi } from "./api";
import { createAgentAccountsStore, redactAgentAccount } from "./store";
import type {
  AgentAccount,
  AgentAuthorizationStarted,
  AgentProviderRegistration,
} from "./types";

const provider: AgentProviderRegistration = {
  providerId: "codex",
  providerName: "Codex",
  harness: "alfred",
  authMethods: ["chatgpt_oauth", "chatgpt_device_code"],
  billingSource: "chatgpt_subscription",
  credentialCustody: "runtime_managed",
  connectAvailable: true,
  gateCode: null,
};

const account: AgentAccount = {
  id: "account_opaque",
  providerId: "codex",
  providerName: "Codex",
  harness: "alfred",
  displayName: "Codex User",
  externalAccountId: "user-1",
  externalWorkspaceId: null,
  authMethod: "oauth_pkce",
  custodyMode: "alfred_managed",
  scopes: ["models:read"],
  status: "connected",
  expiresAt: null,
  lastCheckedAt: null,
  lastErrorCode: null,
  createdAt: "now",
  updatedAt: "now",
};

const attempt: AgentAuthorizationStarted = {
  attemptId: "attempt-opaque",
  providerId: "codex",
  authorizationUrl: "https://provider.invalid/authorize",
  userCode: null,
  expiresAt: "2099-01-01T00:00:00Z",
};

function fakeApi(overrides: Partial<AgentAccountsApi> = {}): AgentAccountsApi {
  return {
    listProviders: async () => [provider],
    listAccounts: async () => [account],
    getAccount: async () => account,
    startAuthorization: async () => attempt,
    completeAuthorization: async () => account,
    cancelAuthorization: async () => {},
    connectApiKeyAccount: async () => account,
    refreshAccount: async () => account,
    disconnectAccount: async () => {},
    ...overrides,
  };
}

test("keeps only the redacted account DTO shape in React state", async () => {
  const unsafe = {
    ...account,
    credentialRef: "credential-secret",
    identityKey: "identity-secret",
    accessToken: "token-secret",
  } as AgentAccount;
  const store = createAgentAccountsStore(
    fakeApi({ listAccounts: async () => [unsafe] }),
  );
  expect(await store.getState().load()).toBe(true);
  const serialized = JSON.stringify(store.getState().accounts);
  expect(serialized).not.toContain("credential-secret");
  expect(serialized).not.toContain("identity-secret");
  expect(serialized).not.toContain("token-secret");
  expect(redactAgentAccount(unsafe)).toEqual(account);
});

test("authorization cancellation removes process-local attempt state", async () => {
  let cancelled = "";
  const store = createAgentAccountsStore(
    fakeApi({
      cancelAuthorization: async (attemptId) => {
        cancelled = attemptId;
      },
    }),
  );
  expect(await store.getState().start("codex")).toEqual(attempt);
  expect(store.getState().attempts.codex).toEqual(attempt);
  await store.getState().cancel("codex");
  expect(cancelled).toBe("attempt-opaque");
  expect(store.getState().attempts.codex).toBeUndefined();
});

test("authorization state keeps only the safe start fields", async () => {
  const unsafe = {
    ...attempt,
    accessToken: "token-secret",
    authorizationCode: "code-secret",
    pkceVerifier: "verifier-secret",
  } as AgentAuthorizationStarted;
  const store = createAgentAccountsStore(
    fakeApi({ startAuthorization: async () => unsafe }),
  );
  await store.getState().start("codex");
  const serialized = JSON.stringify(store.getState().attempts);
  expect(serialized).not.toContain("token-secret");
  expect(serialized).not.toContain("code-secret");
  expect(serialized).not.toContain("verifier-secret");
});

test("API-key connect passes the secret transiently and keeps only redacted metadata", async () => {
  const apiKey = "sk-ant-test-transient-secret-value";
  let received: string | null = null;
  const apiKeyAccount = {
    ...account,
    id: "account_claude",
    providerId: "claude_code",
    providerName: "Claude",
    displayName: "API key",
    externalAccountId: null,
    authMethod: "api_key" as const,
    scopes: [],
    credentialRef: "agent-account-hidden-ref",
    rawApiKey: apiKey,
  } as AgentAccount;
  const store = createAgentAccountsStore(
    fakeApi({
      connectApiKeyAccount: async (providerId, harness, accountId, secret) => {
        expect(providerId).toBe("claude_code");
        expect(harness).toBe("alfred");
        expect(accountId).toBeNull();
        received = secret;
        return apiKeyAccount;
      },
    }),
  );

  expect(await store.getState().connectApiKey("claude_code", apiKey)).toBe(true);
  expect(received).toBe(apiKey);
  const serialized = JSON.stringify(store.getState());
  expect(serialized).not.toContain(apiKey);
  expect(serialized).not.toContain("agent-account-hidden-ref");
  expect(store.getState().accounts).toEqual([
    redactAgentAccount(apiKeyAccount),
  ]);
});

test("API-key failures expose only mapped errors and never retain the submitted key", async () => {
  const apiKey = "xai-test-transient-secret-value";
  const store = createAgentAccountsStore(
    fakeApi({
      connectApiKeyAccount: async () => {
        throw {
          code: "credential_store_locked",
          message: `unsafe backend detail containing ${apiKey}`,
          recoverable: true,
        };
      },
    }),
  );

  expect(await store.getState().connectApiKey("grok", apiKey)).toBe(false);
  expect(store.getState().error).toEqual({
    code: "credential_store_locked",
    message: "Unlock the system credential store and try again.",
    recoverable: true,
  });
  expect(JSON.stringify(store.getState())).not.toContain(apiKey);
});

test("partial disconnect reloads the recovery state instead of removing it", async () => {
  const pending = {
    ...account,
    status: "disconnect_pending" as const,
    lastErrorCode: "credential_store_locked",
  };
  const store = createAgentAccountsStore(
    fakeApi({
      disconnectAccount: async () => {
        throw {
          code: "credential_store_locked",
          recoverable: true,
        };
      },
      getAccount: async () => pending,
    }),
  );
  await store.getState().load();
  expect(await store.getState().disconnect(account.id)).toBe(false);
  expect(store.getState().accounts[0]?.status).toBe("disconnect_pending");
  expect(store.getState().error?.code).toBe("credential_store_locked");
});
