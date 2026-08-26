import { create, type StoreApi, type UseBoundStore } from "zustand";
import { agentAccountsApi, type AgentAccountsApi } from "./api";
import type {
  AgentAccount,
  AgentAccountError,
  AgentAuthorizationStarted,
  AgentProductId,
  AgentProviderRegistration,
} from "./types";

const ERROR_MESSAGES: Record<string, string> = {
  account_not_found: "That native-agent account no longer exists.",
  account_store_failed: "Native-agent account details could not be updated.",
  agent_account_state_unavailable:
    "Native-agent account state is unavailable. Try again.",
  native_provider_not_available:
    "Native authorization for this provider is not available in this build.",
  authorization_not_found:
    "That authorization attempt ended or the app restarted. Start again.",
  authorization_expired: "That authorization attempt expired. Start again.",
  authorization_cancelled: "The authorization attempt was cancelled.",
  authorization_state_mismatch:
    "The authorization response could not be verified. Start again.",
  authorization_busy:
    "Too many native authorization attempts are active. Cancel one and retry.",
  credential_missing: "The saved credential is missing. Reconnect the account.",
  credential_invalid: "The saved credential is invalid. Reconnect the account.",
  credential_store_locked:
    "Unlock the system credential store and try again.",
  credential_store_failed:
    "The system credential store could not complete this operation.",
  credential_rollback_failed:
    "The system credential store could not restore the previous key. Reconnect before running this provider.",
  api_key_invalid:
    "That API key is not valid for this provider. Check the key and try again.",
  api_key_provider_not_supported:
    "API-key account setup is not available for that native provider.",
  unsupported_auth_mode:
    "This build cannot manage the account's authorization method.",
  account_not_refreshable:
    "Finish disconnecting this account or reconnect it before refreshing.",
  metadata_delete_failed:
    "The credential was removed, but revoked local metadata remains. Retry disconnecting.",
};

export function mapAgentAccountError(error: unknown): AgentAccountError {
  const candidate =
    typeof error === "object" && error !== null
      ? (error as Partial<AgentAccountError>)
      : {};
  const code =
    typeof candidate.code === "string" && candidate.code in ERROR_MESSAGES
      ? candidate.code
      : "agent_account_failed";
  return {
    code,
    message:
      ERROR_MESSAGES[code] ??
      "The native-agent account operation could not be completed.",
    recoverable: candidate.recoverable === true,
  };
}

export function redactAgentAccount(account: AgentAccount): AgentAccount {
  return {
    id: account.id,
    providerId: account.providerId,
    providerName: account.providerName,
    productId: account.productId,
    productName: account.productName,
    harness: "alfred",
    displayName: account.displayName,
    externalAccountId: account.externalAccountId,
    externalWorkspaceId: account.externalWorkspaceId,
    authMethod: account.authMethod,
    custodyMode: account.custodyMode,
    managedRuntimeId: account.managedRuntimeId,
    managedRuntimeVersion: account.managedRuntimeVersion,
    scopes: [...account.scopes],
    billingSource: account.billingSource,
    billingOwner: account.billingOwner,
    entitlementState: account.entitlementState,
    entitlementSource: account.entitlementSource,
    entitlementObservedAt: account.entitlementObservedAt,
    status: account.status,
    expiresAt: account.expiresAt,
    lastCheckedAt: account.lastCheckedAt,
    lastErrorCode: account.lastErrorCode,
    createdAt: account.createdAt,
    updatedAt: account.updatedAt,
  };
}

export function redactAuthorizationAttempt(
  attempt: AgentAuthorizationStarted,
): AgentAuthorizationStarted {
  return {
    attemptId: attempt.attemptId,
    providerId: attempt.providerId,
    productId: attempt.productId,
    authorizationUrl: attempt.authorizationUrl,
    userCode: attempt.userCode,
    expiresAt: attempt.expiresAt,
  };
}

export type AgentAccountsState = {
  providers: AgentProviderRegistration[];
  accounts: AgentAccount[];
  attempts: Record<string, AgentAuthorizationStarted>;
  loading: boolean;
  busyId: string | null;
  error: AgentAccountError | null;
  load: () => Promise<boolean>;
  start: (providerId: string, productId: AgentProductId) => Promise<AgentAuthorizationStarted | null>;
  complete: (productId: AgentProductId, completionState?: string | null) => Promise<boolean>;
  cancel: (productId: AgentProductId) => Promise<void>;
  connectApiKey: (
    providerId: string,
    productId: AgentProductId,
    apiKey: string,
    accountId?: string,
  ) => Promise<boolean>;
  refresh: (id: string) => Promise<boolean>;
  disconnect: (id: string, metadataOnly?: boolean) => Promise<boolean>;
  clearError: () => void;
};

export type AgentAccountsStore = UseBoundStore<StoreApi<AgentAccountsState>>;

export function createAgentAccountsStore(
  api: AgentAccountsApi = agentAccountsApi,
): AgentAccountsStore {
  return create<AgentAccountsState>((set, get) => ({
    providers: [],
    accounts: [],
    attempts: {},
    loading: false,
    busyId: null,
    error: null,

    load: async () => {
      set({ loading: true, error: null });
      try {
        const [providers, accounts] = await Promise.all([
          api.listProviders(),
          api.listAccounts(),
        ]);
        set({
          providers,
          accounts: accounts.map(redactAgentAccount),
          loading: false,
        });
        return true;
      } catch (error) {
        set({ loading: false, error: mapAgentAccountError(error) });
        return false;
      }
    },

    start: async (providerId, productId) => {
      set({ busyId: productId, error: null });
      try {
        const attempt = redactAuthorizationAttempt(
          await api.startAuthorization(providerId, productId, "alfred"),
        );
        set((state) => ({
          attempts: { ...state.attempts, [productId]: attempt },
          busyId: null,
        }));
        return attempt;
      } catch (error) {
        set({ busyId: null, error: mapAgentAccountError(error) });
        return null;
      }
    },

    complete: async (productId, completionState = null) => {
      const attempt = get().attempts[productId];
      if (!attempt) return false;
      set({ busyId: productId, error: null });
      try {
        const account = redactAgentAccount(
          await api.completeAuthorization(
            attempt.attemptId,
            attempt.providerId,
            productId,
            "alfred",
            completionState,
          ),
        );
        set((state) => {
          const attempts = { ...state.attempts };
          delete attempts[productId];
          return {
            accounts: [
              ...state.accounts.filter((item) => item.id !== account.id),
              account,
            ],
            attempts,
            busyId: null,
          };
        });
        return true;
      } catch (error) {
        set((state) => {
          const attempts = { ...state.attempts };
          delete attempts[productId];
          return {
            attempts,
            busyId: null,
            error: mapAgentAccountError(error),
          };
        });
        return false;
      }
    },

    cancel: async (productId) => {
      const attempt = get().attempts[productId];
      if (!attempt) return;
      try {
        await api.cancelAuthorization(attempt.attemptId);
      } catch {
        // Attempts are process-local and expire without cleanup work.
      } finally {
        set((state) => {
          const attempts = { ...state.attempts };
          delete attempts[productId];
          return { attempts };
        });
      }
    },

    connectApiKey: async (providerId, productId, apiKey, accountId) => {
      const operationId = accountId ?? productId;
      set({ busyId: operationId, error: null });
      try {
        const account = redactAgentAccount(
          await api.connectApiKeyAccount(
            providerId,
            productId,
            "alfred",
            accountId ?? null,
            apiKey,
          ),
        );
        set((state) => ({
          accounts: [
            ...state.accounts.filter((item) => item.id !== account.id),
            account,
          ],
          busyId: null,
        }));
        return true;
      } catch (error) {
        set({ busyId: null, error: mapAgentAccountError(error) });
        return false;
      }
    },

    refresh: async (id) => {
      set({ busyId: id, error: null });
      try {
        const account = redactAgentAccount(await api.refreshAccount(id));
        set((state) => ({
          accounts: state.accounts.map((item) =>
            item.id === id ? account : item,
          ),
          busyId: null,
        }));
        return true;
      } catch (error) {
        const mapped = mapAgentAccountError(error);
        let latest: AgentAccount | null = null;
        try {
          const response = await api.getAccount(id);
          latest = response ? redactAgentAccount(response) : null;
        } catch {
          // Preserve the last safe snapshot if metadata cannot be reloaded.
        }
        set((state) => ({
          accounts: latest
            ? state.accounts.map((item) => (item.id === id ? latest! : item))
            : state.accounts,
          busyId: null,
          error: mapped,
        }));
        return false;
      }
    },

    disconnect: async (id, metadataOnly = false) => {
      set({ busyId: id, error: null });
      try {
        await api.disconnectAccount(id, metadataOnly);
        set((state) => ({
          accounts: state.accounts.filter((item) => item.id !== id),
          busyId: null,
        }));
        return true;
      } catch (error) {
        const mapped = mapAgentAccountError(error);
        let latest: AgentAccount | null = null;
        try {
          const response = await api.getAccount(id);
          latest = response ? redactAgentAccount(response) : null;
        } catch {
          // Keep the last safe snapshot.
        }
        set((state) => ({
          accounts: latest
            ? state.accounts.map((item) => (item.id === id ? latest! : item))
            : state.accounts,
          busyId: null,
          error: mapped,
        }));
        return false;
      }
    },

    clearError: () => set({ error: null }),
  }));
}

export const useAgentAccountsStore = createAgentAccountsStore();
