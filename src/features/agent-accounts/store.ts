import { create, type StoreApi, type UseBoundStore } from "zustand";
import { agentAccountsApi, type AgentAccountsApi } from "./api";
import type {
  AgentAccount,
  AgentAccountError,
  AgentAuthorizationStarted,
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
    harness: "alfred",
    displayName: account.displayName,
    externalAccountId: account.externalAccountId,
    externalWorkspaceId: account.externalWorkspaceId,
    authMethod: account.authMethod,
    custodyMode: account.custodyMode,
    scopes: [...account.scopes],
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
  start: (providerId: string) => Promise<AgentAuthorizationStarted | null>;
  complete: (providerId: string, completionState?: string | null) => Promise<boolean>;
  cancel: (providerId: string) => Promise<void>;
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

    start: async (providerId) => {
      set({ busyId: providerId, error: null });
      try {
        const attempt = redactAuthorizationAttempt(
          await api.startAuthorization(providerId, "alfred"),
        );
        set((state) => ({
          attempts: { ...state.attempts, [providerId]: attempt },
          busyId: null,
        }));
        return attempt;
      } catch (error) {
        set({ busyId: null, error: mapAgentAccountError(error) });
        return null;
      }
    },

    complete: async (providerId, completionState = null) => {
      const attempt = get().attempts[providerId];
      if (!attempt) return false;
      set({ busyId: providerId, error: null });
      try {
        const account = redactAgentAccount(
          await api.completeAuthorization(
            attempt.attemptId,
            providerId,
            "alfred",
            completionState,
          ),
        );
        set((state) => {
          const attempts = { ...state.attempts };
          delete attempts[providerId];
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
          delete attempts[providerId];
          return {
            attempts,
            busyId: null,
            error: mapAgentAccountError(error),
          };
        });
        return false;
      }
    },

    cancel: async (providerId) => {
      const attempt = get().attempts[providerId];
      if (!attempt) return;
      try {
        await api.cancelAuthorization(attempt.attemptId);
      } catch {
        // Attempts are process-local and expire without cleanup work.
      } finally {
        set((state) => {
          const attempts = { ...state.attempts };
          delete attempts[providerId];
          return { attempts };
        });
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
