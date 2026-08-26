import type { AgentProductId, CredentialCustodyMode, ManagedRuntimeId } from "./types";

export type ManagedRuntimeInstallState =
  | "missing"
  | "preparing"
  | "ready"
  | "failed"
  | "blocked";

export type ManagedRuntimeConnectionKind =
  | "browser"
  | "device_code"
  | "api_key"
  | "terminal"
  | "unsupported";

export type ManagedRuntimeConnectionState =
  | "disconnected"
  | "connecting"
  | "connected"
  | "limited"
  | "error";

/** Safe product metadata returned by the managed-runtime command boundary. */
export type ManagedRuntimeProduct = {
  providerId: string;
  productId: AgentProductId;
  productName: string;
  runtimeId: ManagedRuntimeId;
  runtimeVersion: string;
  installState: ManagedRuntimeInstallState;
  connectionKind: ManagedRuntimeConnectionKind;
  connectAvailable: boolean;
  gateCodes: string[];
  billingSource: string;
  custodyMode: CredentialCustodyMode;
};

/** Safe ceremony handles. Provider auth material never crosses this type. */
export type ManagedRuntimeConnectionStarted = {
  kind: ManagedRuntimeConnectionKind;
  attemptId: string | null;
  authorizationUrl: string | null;
  userCode: string | null;
  expiresAt: string | null;
  terminalSessionId: string | null;
};

/** Safe connection status; accountId is only an opaque local account id. */
export type ManagedRuntimeConnectionStatus = {
  providerId: string;
  productId: AgentProductId;
  installState: ManagedRuntimeInstallState;
  connectionState: ManagedRuntimeConnectionState;
  accountId: string | null;
  entitlementState:
    | "unknown"
    | "eligible"
    | "limited"
    | "exhausted"
    | "ineligible";
  lastErrorCode: string | null;
};

export type ManagedRuntimeTerminalRead = {
  sessionId: string;
  cursor: number;
  output: string;
  closed: boolean;
};
