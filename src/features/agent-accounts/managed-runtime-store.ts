import { create, type StoreApi, type UseBoundStore } from "zustand";
import {
  managedRuntimeApi,
  type ManagedRuntimeApi,
} from "./managed-runtime-api";
import type {
  ManagedRuntimeConnectionStarted,
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
} from "./managed-runtime-types";
import type { AgentProductId } from "./types";

const ERROR_MESSAGES: Record<string, string> = {
  managed_runtime_selection_invalid:
    "This managed runtime selection is not valid for the product.",
  managed_runtime_not_available:
    "This managed runtime is not available in the current build.",
  managed_runtime_package_missing:
    "The managed runtime package is not installed yet.",
  managed_runtime_package_unverified:
    "The managed runtime package could not be verified.",
  managed_runtime_connection_failed:
    "The provider connection could not be started. Try again.",
  managed_runtime_connection_not_found:
    "That managed runtime connection is no longer active.",
  managed_runtime_api_key_product_invalid:
    "A managed API key can only be entered for OpenCode Go.",
  managed_runtime_api_key_invalid:
    "That OpenCode Go key could not be accepted. Check it and try again.",
  managed_runtime_terminal_not_found:
    "The provider terminal is no longer available. Start again.",
  managed_runtime_terminal_io_failed:
    "The provider terminal could not be reached. Start again.",
};

export type ManagedRuntimeError = {
  code: string;
  message: string;
  recoverable: boolean;
};

export function redactManagedRuntimeProduct(
  product: ManagedRuntimeProduct,
): ManagedRuntimeProduct {
  return {
    providerId: product.providerId,
    productId: product.productId,
    productName: product.productName,
    runtimeId: product.runtimeId,
    runtimeVersion: product.runtimeVersion,
    installState: product.installState,
    connectionKind: product.connectionKind,
    connectAvailable: product.connectAvailable,
    gateCodes: product.gateCodes.filter(isSafeCode),
    billingSource: product.billingSource,
    custodyMode: product.custodyMode,
  };
}

export function redactManagedRuntimeStatus(
  status: ManagedRuntimeConnectionStatus,
): ManagedRuntimeConnectionStatus {
  return {
    providerId: status.providerId,
    productId: status.productId,
    installState: status.installState,
    connectionState: status.connectionState,
    accountId: status.accountId,
    entitlementState: status.entitlementState,
    lastErrorCode: safeCode(status.lastErrorCode),
  };
}

function isSafeCode(value: string): boolean {
  return /^[a-z][a-z0-9_]{0,95}$/.test(value);
}

function safeCode(value: string | null): string | null {
  return value && isSafeCode(value) ? value : value ? "managed_runtime_operation_failed" : null;
}

export function mapManagedRuntimeError(error: unknown): ManagedRuntimeError {
  const candidate =
    typeof error === "object" && error !== null
      ? (error as { code?: unknown; recoverable?: unknown })
      : {};
  const candidateCode =
    typeof candidate.code === "string" ? candidate.code : "";
  const code = Object.prototype.hasOwnProperty.call(
    ERROR_MESSAGES,
    candidateCode,
  )
    ? candidateCode
    : "managed_runtime_operation_failed";
  return {
    code,
    message:
      ERROR_MESSAGES[code] ??
      "The managed runtime operation could not be completed. Try again.",
    recoverable: candidate.recoverable === true,
  };
}

function productKey(providerId: string, productId: AgentProductId): string {
  return `${providerId}:${productId}`;
}

export type ManagedRuntimeState = {
  products: ManagedRuntimeProduct[];
  statuses: Record<string, ManagedRuntimeConnectionStatus>;
  loading: boolean;
  preparingId: string | null;
  connectingId: string | null;
  error: ManagedRuntimeError | null;
  load: () => Promise<boolean>;
  prepare: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeProduct | null>;
  start: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStarted | null>;
  refreshStatus: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStatus | null>;
  connectApiKey: (
    providerId: string,
    productId: AgentProductId,
    apiKey: string,
  ) => Promise<boolean>;
  clearError: () => void;
};

export type ManagedRuntimeStore = UseBoundStore<
  StoreApi<ManagedRuntimeState>
>;

export function createManagedRuntimeStore(
  api: ManagedRuntimeApi = managedRuntimeApi,
): ManagedRuntimeStore {
  return create<ManagedRuntimeState>((set) => ({
    products: [],
    statuses: {},
    loading: false,
    preparingId: null,
    connectingId: null,
    error: null,

    load: async () => {
      set({ loading: true, error: null });
      try {
        const products = (await api.listProducts()).map(
          redactManagedRuntimeProduct,
        );
        const statuses = await Promise.all(
          products.map(async (product) => {
            try {
              return redactManagedRuntimeStatus(
                await api.connectionStatus(product.providerId, product.productId),
              );
            } catch {
              return {
                providerId: product.providerId,
                productId: product.productId,
                installState: product.installState,
                connectionState: "error" as const,
                accountId: null,
                entitlementState: "unknown" as const,
                lastErrorCode: "managed_runtime_connection_failed",
              } satisfies ManagedRuntimeConnectionStatus;
            }
          }),
        );
        set({
          products,
          statuses: Object.fromEntries(
            statuses.map((status) => [
              productKey(status.providerId, status.productId),
              status,
            ]),
          ),
          loading: false,
        });
        return true;
      } catch (error) {
        set({ loading: false, error: mapManagedRuntimeError(error) });
        return false;
      }
    },

    prepare: async (providerId, productId) => {
      const key = productKey(providerId, productId);
      set({ preparingId: key, error: null });
      try {
        const product = redactManagedRuntimeProduct(
          await api.prepareProduct(providerId, productId),
        );
        set((state) => ({
          products: state.products.map((item) =>
            productKey(item.providerId, item.productId) === key ? product : item,
          ),
          preparingId: null,
        }));
        return product;
      } catch (error) {
        set({ preparingId: null, error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    start: async (providerId, productId) => {
      const key = productKey(providerId, productId);
      set({ connectingId: key, error: null });
      try {
        const started = await api.startConnection(providerId, productId);
        set({ connectingId: null });
        return started;
      } catch (error) {
        set({ connectingId: null, error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    refreshStatus: async (providerId, productId) => {
      try {
        const status = redactManagedRuntimeStatus(
          await api.connectionStatus(providerId, productId),
        );
        set((state) => ({
          statuses: {
            ...state.statuses,
            [productKey(providerId, productId)]: status,
          },
        }));
        return status;
      } catch (error) {
        set({ error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    connectApiKey: async (providerId, productId, apiKey) => {
      const key = productKey(providerId, productId);
      set({ connectingId: key, error: null });
      try {
        const status = redactManagedRuntimeStatus(
          await api.connectApiKey(providerId, productId, apiKey),
        );
        set((state) => ({
          statuses: {
            ...state.statuses,
            [key]: status,
          },
          connectingId: null,
        }));
        return true;
      } catch (error) {
        set({ connectingId: null, error: mapManagedRuntimeError(error) });
        return false;
      }
    },

    clearError: () => set({ error: null }),
  }));
}

export const useManagedRuntimeStore = createManagedRuntimeStore();
