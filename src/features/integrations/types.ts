export type AppConnectionStatus =
  | "connected"
  | "expired"
  | "error"
  | "revoked";

export type AppProvider = {
  id: string;
  name: string;
  capabilitySummary: string;
  connectionModes: string[];
  connectAvailable: boolean;
};

/** Redacted metadata returned by Rust. Credential references are never part
 * of the frontend contract. */
export type AppConnection = {
  id: string;
  providerId: string;
  displayName: string | null;
  externalAccountId: string | null;
  externalTenantId: string | null;
  connectionMode: string;
  scopes: string[];
  status: AppConnectionStatus;
  expiresAt: string | null;
  lastCheckedAt: string | null;
  lastErrorCode: string | null;
  createdAt: string;
  updatedAt: string;
};

export type ConnectionUsageItem = {
  id: string;
  label: string;
  enabled: boolean;
};

export type AppConnectionUsage = {
  workflows: ConnectionUsageItem[];
  schedules: ConnectionUsageItem[];
  triggers: ConnectionUsageItem[];
};

export type IntegrationError = {
  code: string;
  message: string;
  recoverable: boolean;
};

export type ActionFieldKind =
  | "text"
  | "textarea"
  | "boolean"
  | "enum"
  | "resource_selector";

export type ActionOption = {
  id: string;
  label: string;
};

export type ActionFieldDescriptor = {
  key: string;
  label: string;
  description: string;
  kind: ActionFieldKind;
  required: boolean;
  default: unknown | null;
  /** Backend validation guarantees this remains false. */
  secret: false;
  optionSource: string | null;
  options: ActionOption[];
  supportsInterpolation: boolean;
};

export type ActionDescriptor = {
  providerId: string;
  actionId: string;
  label: string;
  description: string;
  fields: ActionFieldDescriptor[];
  requiredScopes: string[];
  outputSchemaVersion: number;
  outputIsUntrusted: boolean;
};

export type ActionResourceItem = {
  id: string;
  label: string;
};

export type ActionResourcePage = {
  items: ActionResourceItem[];
  nextPageToken: string | null;
};

export type AppEventDeliveryMode = "polling" | "socket" | "subscription";

export type AppEventDescriptor = {
  providerId: string;
  eventType: string;
  label: string;
  description: string;
  requiredScopes: string[];
  deliveryModes: AppEventDeliveryMode[];
  filterFields: ActionFieldDescriptor[];
  fetchesResourceContent: boolean;
  descriptorVersion: number;
  externalEventIdRequired: boolean;
  allowedAttributeKeys: string[];
  pollIntervalSeconds: number;
  pendingCap: number;
};

export type AppEventResourceItem = {
  id: string;
  label: string;
};

export type AppEventResourcePage = {
  items: AppEventResourceItem[];
  nextPageToken: string | null;
};

export type SlackPrivateConnectionInput = {
  mode: "bot" | "incoming_webhook";
  botToken: string;
  appToken?: string | null;
  webhookUrl?: string | null;
  enablePrivateChannels: boolean;
  enableMentions: boolean;
};

export type NotionPrivateConnectionInput = {
  integrationToken: string;
};

export type ObsidianVaultConnectionInput = {
  vaultPath: string;
};

export type GitHubDeviceAuthorization = {
  pairingSessionId: string;
  userCode: string;
  verificationUri: string;
  installationUrl: string | null;
  expiresAt: string;
  intervalSeconds: number;
};

export type GitHubDevicePollResult =
  | { status: "pending"; retryAfterSeconds: number }
  | { status: "connected"; connection: AppConnection };

export type TelegramPrepareInput = {
  botToken: string;
};

export type TelegramPairingPrepared = {
  pairingSessionId: string;
  botUsername: string;
  pairingUrl: string;
  expiresAt: string;
};

export type TelegramCompleteInput = {
  pairingSessionId: string;
  testMessage: string;
};
