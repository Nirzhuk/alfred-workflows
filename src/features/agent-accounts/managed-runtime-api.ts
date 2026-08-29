import { invoke } from "@tauri-apps/api/core";
import type { AgentProductId } from "./types";
import type {
  ManagedRuntimeConnectionStarted,
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
  ManagedRuntimeTerminalRead,
} from "./managed-runtime-types";

type ManagedRuntimeProductResponse = Omit<ManagedRuntimeProduct, "installState" | "connectionKind"> & {
  installState: string;
  connectionKind: string;
};

type ManagedRuntimeConnectionStartedResponse = {
  kind: string;
  attemptId?: string | null;
  authorizationUrl?: string | null;
  userCode?: string | null;
  expiresAt?: string | null;
  terminalSessionId?: string | null;
};

type ManagedRuntimeConnectionStatusResponse = {
  providerId?: string;
  productId?: AgentProductId;
  installState?: string;
  connectionState?: string;
  state?: string;
  accountId?: string | null;
  entitlementState?: ManagedRuntimeConnectionStatus["entitlementState"];
  lastErrorCode?: string | null;
  gateCodes?: string[];
};

type ManagedRuntimeTerminalReadResponse =
  | {
      sessionId: string;
      cursor: number;
      output: string;
      closed: boolean;
    }
  | {
      sessionId: string;
      sequence: number;
      dataBase64: string;
    }
  | null;

export type ManagedRuntimeInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export type ManagedRuntimeApi = {
  listProducts: () => Promise<ManagedRuntimeProduct[]>;
  prepareProduct: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeProduct>;
  startConnection: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStarted>;
  connectionStatus: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStatus>;
  connectApiKey: (
    providerId: string,
    productId: AgentProductId,
    apiKey: string,
  ) => Promise<ManagedRuntimeConnectionStatus>;
  readTerminal: (
    sessionId: string,
    cursor: number,
  ) => Promise<ManagedRuntimeTerminalRead>;
  writeTerminal: (
    sessionId: string,
    input: string,
  ) => Promise<void>;
  resizeTerminal: (
    sessionId: string,
    cols: number,
    rows: number,
  ) => Promise<void>;
  closeTerminal: (sessionId: string) => Promise<void>;
};

export function createManagedRuntimeApi(
  invokeCommand: ManagedRuntimeInvoke = invoke,
): ManagedRuntimeApi {
  return {
    listProducts: async () => {
      const products = await invokeCommand<ManagedRuntimeProductResponse[]>(
        "list_managed_runtime_products",
      );
      return products.map(normalizeProduct);
    },
    prepareProduct: (providerId, productId) =>
      invokeCommand<ManagedRuntimeProductResponse>("prepare_managed_runtime_product", {
        providerId,
        productId,
      }).then(normalizeProduct),
    startConnection: (providerId, productId) =>
      invokeCommand<ManagedRuntimeConnectionStartedResponse>("start_managed_runtime_connection", {
        providerId,
        productId,
      }).then((started) => normalizeStarted(started, productId)),
    connectionStatus: (providerId, productId) =>
      invokeCommand<ManagedRuntimeConnectionStatusResponse>("managed_runtime_connection_status", {
        providerId,
        productId,
      }).then((status) => normalizeStatus(status, providerId, productId)),
    connectApiKey: (providerId, productId, apiKey) => {
      if (productId !== "opencode_go") {
        return Promise.reject(new Error("managed_runtime_api_key_product_invalid"));
      }
      return invokeCommand<ManagedRuntimeConnectionStatusResponse>("connect_managed_runtime_api_key", {
        providerId,
        productId,
        apiKey,
      }).then((status) => normalizeStatus(status, providerId, productId));
    },
    readTerminal: async (sessionId, cursor) => {
      const response = await invokeCommand<ManagedRuntimeTerminalReadResponse>(
        "read_managed_runtime_terminal",
        { sessionId, cursor },
      );
      if (!response) {
        return { sessionId, cursor: 0, output: "", closed: false };
      }
      if ("output" in response) return response;
      return {
        sessionId: response.sessionId,
        cursor: response.sequence + 1,
        output: response.dataBase64,
        closed: false,
      };
    },
    writeTerminal: (sessionId, input) =>
      invokeCommand("write_managed_runtime_terminal", {
        sessionId,
        input,
      }),
    resizeTerminal: (sessionId, cols, rows) =>
      invokeCommand("resize_managed_runtime_terminal", {
        sessionId,
        cols,
        rows,
      }),
    closeTerminal: (sessionId) =>
      invokeCommand("close_managed_runtime_terminal", { sessionId }),
  };
}

function normalizeProduct(
  product: ManagedRuntimeProductResponse,
): ManagedRuntimeProduct {
  return {
    ...product,
    productId: product.productId,
    runtimeId: product.runtimeId,
    installState: normalizeInstallState(product.installState),
    connectionKind: normalizeConnectionKind(product.connectionKind, product.productId),
    gateCodes: [...(product.gateCodes ?? [])],
  };
}

function normalizeInstallState(value: string): ManagedRuntimeProduct["installState"] {
  if (value === "not_installed" || value === "missing") return "missing";
  if (value === "unavailable" || value === "blocked") return "blocked";
  if (value === "active" || value === "installed" || value === "ready") return "ready";
  if (value === "preparing") return "preparing";
  if (value === "failed") return "failed";
  return "blocked";
}

function normalizeConnectionKind(
  value: string,
  productId: AgentProductId,
): ManagedRuntimeProduct["connectionKind"] {
  if (value === "terminal") return "terminal";
  if (value === "device_code") return "device_code";
  if (value === "browser") return "browser";
  if (value === "api_key" || (productId === "opencode_go" && value === "local_server")) {
    return "api_key";
  }
  if (value === "browser_or_device_code") return "browser";
  return "unsupported";
}

function normalizeStarted(
  started: ManagedRuntimeConnectionStartedResponse,
  productId: AgentProductId,
): ManagedRuntimeConnectionStarted {
  const kind =
    started.kind === "browser_or_device_code"
      ? started.userCode
        ? "device_code"
        : "browser"
      : normalizeConnectionKind(started.kind, productId);
  return {
    kind,
    attemptId: started.attemptId ?? null,
    authorizationUrl: started.authorizationUrl ?? null,
    userCode: started.userCode ?? null,
    expiresAt: started.expiresAt ?? null,
    terminalSessionId: started.terminalSessionId ?? null,
  };
}

function normalizeStatus(
  status: ManagedRuntimeConnectionStatusResponse,
  providerId: string,
  productId: AgentProductId,
): ManagedRuntimeConnectionStatus {
  const state = status.connectionState ?? status.state ?? "error";
  return {
    providerId: status.providerId ?? providerId,
    productId: status.productId ?? productId,
    installState: normalizeInstallState(status.installState ?? (state === "blocked" ? "blocked" : "ready")),
    connectionState: normalizeConnectionState(state),
    accountId: status.accountId ?? null,
    entitlementState: status.entitlementState ?? "unknown",
    lastErrorCode: status.lastErrorCode ?? status.gateCodes?.[0] ?? null,
  };
}

function normalizeConnectionState(
  value: string,
): ManagedRuntimeConnectionStatus["connectionState"] {
  if (value === "connecting") return "connecting";
  if (value === "connected") return "connected";
  if (value === "limited") return "limited";
  if (value === "disconnected") return "disconnected";
  return "error";
}

export const managedRuntimeApi = createManagedRuntimeApi();
