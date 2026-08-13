import { create } from "zustand";
import type { StateCreator } from "zustand";
import { integrationsApi, type IntegrationsApi } from "./api";
import type {
  ActionDescriptor,
  AppConnection,
  AppConnectionUsage,
  AppProvider,
  IntegrationError,
} from "./types";

const ERROR_MESSAGES: Record<string, string> = {
  connection_not_found: "That connected app no longer exists.",
  connection_store_failed: "Connected-app details could not be read or updated.",
  credential_store_locked:
    "Unlock your system credential store and try again, or remove local metadata only.",
  credential_missing:
    "The saved credential is already missing. You can remove local metadata only.",
  disconnect_failed:
    "The credential could not be removed. The connection remains revoked.",
  action_not_found: "This app action is not available in this version of Alfred.",
  connection_required: "Choose a healthy connected app.",
  scope_missing: "Reconnect this app with the access required by the action.",
  rate_limited: "The provider is rate limiting requests. Try again later.",
  provider_unauthorized: "Reconnect this app and try again.",
  provider_unavailable: "The provider is temporarily unavailable.",
  invalid_input: "The app action configuration is invalid.",
  output_too_large: "The provider result exceeds the safe output limit.",
  output_invalid: "The provider returned an invalid result.",
  timed_out: "The provider request timed out.",
  cancelled: "The app action was cancelled.",
};

export function normalizeIntegrationError(error: unknown): IntegrationError {
  const candidate =
    typeof error === "object" && error !== null
      ? (error as Partial<IntegrationError>)
      : {};
  const code =
    typeof candidate.code === "string" && candidate.code in ERROR_MESSAGES
      ? candidate.code
      : "integration_failed";
  return {
    code,
    message:
      ERROR_MESSAGES[code] ??
      "The connected-app operation could not be completed. Try again.",
    recoverable: candidate.recoverable === true,
  };
}

export function redactConnectionPayload(
  connection: AppConnection,
): AppConnection {
  return {
    id: connection.id,
    providerId: connection.providerId,
    displayName: connection.displayName,
    externalAccountId: connection.externalAccountId,
    externalTenantId: connection.externalTenantId,
    connectionMode: connection.connectionMode,
    scopes: [...connection.scopes],
    status: connection.status,
    expiresAt: connection.expiresAt,
    lastCheckedAt: connection.lastCheckedAt,
    lastErrorCode: connection.lastErrorCode,
    createdAt: connection.createdAt,
    updatedAt: connection.updatedAt,
  };
}

export type IntegrationsState = {
  providers: AppProvider[];
  connections: AppConnection[];
  descriptors: ActionDescriptor[];
  loading: boolean;
  disconnectingId: string | null;
  error: IntegrationError | null;
  load: () => Promise<boolean>;
  refresh: () => Promise<boolean>;
  getUsage: (id: string) => Promise<AppConnectionUsage | null>;
  disconnect: (
    id: string,
    metadataOnly?: boolean,
  ) => Promise<IntegrationError | null>;
  clearError: () => void;
};

export function createIntegrationsState(
  api: IntegrationsApi,
): StateCreator<IntegrationsState> {
  return (set, get) => ({
    providers: [],
    connections: [],
    descriptors: [],
    loading: false,
    disconnectingId: null,
    error: null,

    load: async () => {
      set({ loading: true, error: null });
      try {
        const [providers, connections, descriptors] = await Promise.all([
          api.listProviders(),
          api.listConnections(),
          api.listActionDescriptors(),
        ]);
        set({
          providers,
          connections: connections.map(redactConnectionPayload),
          descriptors: descriptors.filter(
            (descriptor) =>
              descriptor.fields.every((field) => field.secret === false),
          ),
          loading: false,
        });
        return true;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return false;
      }
    },

    refresh: async () => get().load(),

    getUsage: async (id) => {
      set({ error: null });
      try {
        return await api.getUsage(id);
      } catch (error) {
        set({ error: normalizeIntegrationError(error) });
        return null;
      }
    },

    disconnect: async (id, metadataOnly = false) => {
      set({ disconnectingId: id, error: null });
      try {
        await api.disconnect(id, metadataOnly);
        set((state) => ({
          connections: state.connections.filter(
            (connection) => connection.id !== id,
          ),
          disconnectingId: null,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        let refreshedConnection: AppConnection | null | undefined;
        try {
          const result = await api.getConnection(id);
          refreshedConnection = result
            ? redactConnectionPayload(result)
            : null;
        } catch {
          refreshedConnection = undefined;
        }
        set((state) => ({
          connections:
            refreshedConnection === undefined
              ? state.connections
              : refreshedConnection === null
                ? state.connections.filter((connection) => connection.id !== id)
                : state.connections.map((connection) =>
                    connection.id === id ? refreshedConnection : connection,
                  ),
          disconnectingId: null,
          error: normalized,
        }));
        return normalized;
      }
    },

    clearError: () => set({ error: null }),
  });
}

export const useIntegrationsStore = create<IntegrationsState>(
  createIntegrationsState(integrationsApi),
);
