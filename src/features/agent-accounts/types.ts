export type AgentHarness = "alfred";
export type AgentAuthMethod =
  | "oauth_pkce"
  | "device_code"
  | "api_key"
  | "runtime";
export type CredentialCustodyMode = "alfred_managed" | "runtime_managed";
export type AgentProductId =
  | "claude_code_subscription"
  | "claude_api"
  | "chatgpt_codex"
  | "openai_api"
  | "opencode_go"
  | "opencode_zen"
  | "cursor_cloud"
  | "github_copilot_subscription"
  | "gemini_api"
  | "grok_api";
export type ManagedRuntimeId =
  | "claude_code_managed"
  | "codex_python_sdk"
  | "opencode_server";
export type AgentEntitlementState =
  | "unknown"
  | "eligible"
  | "limited"
  | "exhausted"
  | "ineligible";
export type AgentAccountStatus =
  | "connected"
  | "expired"
  | "error"
  | "revoked"
  | "disconnect_pending";

export type AgentProviderRegistration = {
  providerId: string;
  providerName: string;
  productId: AgentProductId;
  productName: string;
  harness: AgentHarness;
  authMethods: string[];
  billingSource: string;
  billingOwner: string;
  credentialCustody: string;
  managedRuntimeId: ManagedRuntimeId | null;
  managedRuntimeVersion: string | null;
  connectAvailable: boolean;
  gateCode: string | null;
};

/** Safe account metadata. Credential and identity references never enter React. */
export type AgentAccount = {
  id: string;
  providerId: string;
  providerName: string;
  productId: AgentProductId;
  productName: string;
  harness: AgentHarness;
  displayName: string | null;
  externalAccountId: string | null;
  externalWorkspaceId: string | null;
  authMethod: AgentAuthMethod;
  custodyMode: CredentialCustodyMode;
  managedRuntimeId: ManagedRuntimeId | null;
  managedRuntimeVersion: string | null;
  scopes: string[];
  billingSource: string;
  billingOwner: string;
  entitlementState: AgentEntitlementState;
  entitlementSource: string;
  entitlementObservedAt: string | null;
  status: AgentAccountStatus;
  expiresAt: string | null;
  lastCheckedAt: string | null;
  lastErrorCode: string | null;
  createdAt: string;
  updatedAt: string;
};

export type AgentAuthorizationStarted = {
  attemptId: string;
  providerId: string;
  productId: AgentProductId;
  authorizationUrl: string | null;
  userCode: string | null;
  expiresAt: string;
};

export function usesAlfredManagedApiKey(
  authMethod: AgentAuthMethod | readonly string[],
  custodyMode: string,
): boolean {
  const hasApiKey =
    typeof authMethod === "string"
      ? authMethod === "api_key"
      : authMethod.includes("api_key");
  return hasApiKey && custodyMode === "alfred_managed";
}

export type AgentAccountError = {
  code: string;
  message: string;
  recoverable: boolean;
};
