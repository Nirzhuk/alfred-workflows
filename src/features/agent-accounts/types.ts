export type AgentHarness = "alfred";
export type AgentAuthMethod = "oauth_pkce" | "device_code" | "runtime";
export type CredentialCustodyMode = "alfred_managed" | "runtime_managed";
export type AgentAccountStatus =
  | "connected"
  | "expired"
  | "error"
  | "revoked"
  | "disconnect_pending";

export type AgentProviderRegistration = {
  providerId: string;
  providerName: string;
  harness: AgentHarness;
  authMethods: string[];
  billingSource: string;
  credentialCustody: string;
  connectAvailable: boolean;
  gateCode: string | null;
};

/** Safe account metadata. Credential and identity references never enter React. */
export type AgentAccount = {
  id: string;
  providerId: string;
  providerName: string;
  harness: AgentHarness;
  displayName: string | null;
  externalAccountId: string | null;
  externalWorkspaceId: string | null;
  authMethod: AgentAuthMethod;
  custodyMode: CredentialCustodyMode;
  scopes: string[];
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
  authorizationUrl: string | null;
  userCode: string | null;
  expiresAt: string;
};

export type AgentAccountError = {
  code: string;
  message: string;
  recoverable: boolean;
};
