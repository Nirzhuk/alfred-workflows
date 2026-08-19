import { invoke } from "@tauri-apps/api/core";
import type {
  ActionDescriptor,
  ActionResourcePage,
  AppEventDescriptor,
  AppEventResourcePage,
  AppConnection,
  AppConnectionUsage,
  AppProvider,
  GitHubDeviceAuthorization,
  GitHubDevicePollResult,
  GmailAuthorizationStarted,
  LinearPrivateConnectionInput,
  MicrosoftAuthorizationStarted,
  MicrosoftPrepareInput,
  NotionPrivateConnectionInput,
  ObsidianVaultConnectionInput,
  SentryAuthTokenConnectionInput,
  SlackPrivateConnectionInput,
  TelegramCompleteInput,
  TelegramPairingPrepared,
  TelegramPrepareInput,
  WhatsAppPairingState,
  WhatsAppTestSend,
} from "./types";

export type IntegrationsApi = {
  listProviders: () => Promise<AppProvider[]>;
  listConnections: () => Promise<AppConnection[]>;
  getConnection: (id: string) => Promise<AppConnection | null>;
  getUsage: (id: string) => Promise<AppConnectionUsage>;
  disconnect: (id: string, metadataOnly: boolean) => Promise<void>;
  listActionDescriptors: (providerId?: string) => Promise<ActionDescriptor[]>;
  listActionResources: (input: {
    connectionId: string;
    providerId: string;
    actionId: string;
    fieldKey: string;
    query: string;
    pageToken?: string | null;
  }) => Promise<ActionResourcePage>;
  listEventDescriptors: (providerId?: string) => Promise<AppEventDescriptor[]>;
  listEventResources: (input: {
    connectionId: string;
    providerId: string;
    eventType: string;
    fieldKey: string;
    query: string;
    pageToken?: string | null;
  }) => Promise<AppEventResourcePage>;
  connectSlackPrivate: (
    input: SlackPrivateConnectionInput,
  ) => Promise<AppConnection>;
  prepareGithubConnection: () => Promise<GitHubDeviceAuthorization>;
  pollGithubConnection: (
    pairingSessionId: string,
  ) => Promise<GitHubDevicePollResult>;
  cancelGithubPairing: (pairingSessionId: string) => Promise<void>;
  prepareGmailConnection: () => Promise<GmailAuthorizationStarted>;
  completeGmailConnection: (sessionId: string) => Promise<AppConnection>;
  cancelGmailAuthorization: (sessionId: string) => Promise<void>;
  prepareMicrosoftConnection: (
    input: MicrosoftPrepareInput,
  ) => Promise<MicrosoftAuthorizationStarted>;
  completeMicrosoftConnection: (sessionId: string) => Promise<AppConnection>;
  cancelMicrosoftAuthorization: (sessionId: string) => Promise<void>;
  connectNotionPrivate: (
    input: NotionPrivateConnectionInput,
  ) => Promise<AppConnection>;
  connectLinearPrivate: (
    input: LinearPrivateConnectionInput,
  ) => Promise<AppConnection>;
  connectSentryPrivate: (
    input: SentryAuthTokenConnectionInput,
  ) => Promise<AppConnection>;
  connectObsidianVault: (
    input: ObsidianVaultConnectionInput,
  ) => Promise<AppConnection>;
  prepareTelegramConnection: (
    input: TelegramPrepareInput,
  ) => Promise<TelegramPairingPrepared>;
  completeTelegramConnection: (
    input: TelegramCompleteInput,
  ) => Promise<AppConnection>;
  cancelTelegramPairing: (pairingSessionId: string) => Promise<void>;
  beginWhatsappPairing: (
    acknowledgedVersion: string,
  ) => Promise<WhatsAppPairingState>;
  whatsappPairingState: () => Promise<WhatsAppPairingState>;
  sendWhatsappPairingTest: (message: string) => Promise<WhatsAppTestSend>;
  completeWhatsappPairing: () => Promise<AppConnection>;
  cancelWhatsappPairing: () => Promise<void>;
};

export const integrationsApi: IntegrationsApi = {
  listProviders: () => invoke("list_app_providers"),
  listConnections: () => invoke("list_app_connections"),
  getConnection: (id) => invoke("get_app_connection", { id }),
  getUsage: (id) => invoke("get_app_connection_usage", { id }),
  disconnect: (id, metadataOnly) =>
    invoke("disconnect_app_connection", { id, metadataOnly }),
  listActionDescriptors: (providerId) =>
    invoke("list_app_action_descriptors", { providerId: providerId ?? null }),
  listActionResources: (input) =>
    invoke("list_app_action_resources", {
      ...input,
      pageToken: input.pageToken ?? null,
    }),
  listEventDescriptors: (providerId) =>
    invoke("list_app_event_descriptors", { providerId: providerId ?? null }),
  listEventResources: (input) =>
    invoke("list_app_event_resources", {
      ...input,
      pageToken: input.pageToken ?? null,
    }),
  connectSlackPrivate: (input) =>
    invoke("connect_slack_private", { input }),
  prepareGithubConnection: () => invoke("prepare_github_connection"),
  pollGithubConnection: (pairingSessionId) =>
    invoke("poll_github_connection", { pairingSessionId }),
  cancelGithubPairing: (pairingSessionId) =>
    invoke("cancel_github_pairing", { pairingSessionId }),
  prepareGmailConnection: () => invoke("prepare_gmail_connection"),
  completeGmailConnection: (sessionId) =>
    invoke("complete_gmail_connection", { sessionId }),
  cancelGmailAuthorization: (sessionId) =>
    invoke("cancel_gmail_authorization", { sessionId }),
  prepareMicrosoftConnection: (input) =>
    invoke("prepare_microsoft_connection", { input }),
  completeMicrosoftConnection: (sessionId) =>
    invoke("complete_microsoft_connection", { sessionId }),
  cancelMicrosoftAuthorization: (sessionId) =>
    invoke("cancel_microsoft_authorization", { sessionId }),
  connectNotionPrivate: (input) =>
    invoke("connect_notion_private", { input }),
  connectLinearPrivate: (input) =>
    invoke("connect_linear_private", { input }),
  connectSentryPrivate: (input) =>
    invoke("connect_sentry_private", { input }),
  connectObsidianVault: (input) =>
    invoke("connect_obsidian_vault", { input }),
  prepareTelegramConnection: (input) =>
    invoke("prepare_telegram_connection", { input }),
  completeTelegramConnection: (input) =>
    invoke("complete_telegram_connection", { input }),
  cancelTelegramPairing: (pairingSessionId) =>
    invoke("cancel_telegram_pairing", { pairingSessionId }),
  beginWhatsappPairing: (acknowledgedVersion) =>
    invoke("begin_whatsapp_pairing", { acknowledgedVersion }),
  whatsappPairingState: () => invoke("whatsapp_pairing_state"),
  sendWhatsappPairingTest: (message) =>
    invoke("send_whatsapp_pairing_test", { message }),
  completeWhatsappPairing: () => invoke("complete_whatsapp_pairing"),
  cancelWhatsappPairing: () => invoke("cancel_whatsapp_pairing"),
};
