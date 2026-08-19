import { create } from "zustand";
import type { StateCreator } from "zustand";
import { integrationsApi, type IntegrationsApi } from "./api";
import type {
  ActionDescriptor,
  AppEventDescriptor,
  AppConnection,
  AppConnectionUsage,
  AppProvider,
  GitHubDeviceAuthorization,
  GitHubDevicePollResult,
  GmailAuthorizationStarted,
  IntegrationError,
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
  event_not_found: "This app event is not available in this version of Alfred.",
  connection_required: "Choose a healthy connected app.",
  scope_missing: "Reconnect this app with the access required by the action.",
  rate_limited: "The provider is rate limiting requests. Try again later.",
  provider_unauthorized: "Reconnect this app and try again.",
  provider_unavailable: "The provider is temporarily unavailable.",
  invalid_input: "The app action configuration is invalid.",
  output_too_large: "The provider result exceeds the safe output limit.",
  output_invalid: "The provider returned an invalid result.",
  event_too_large: "The provider event exceeds the safe payload limit.",
  event_invalid: "The provider returned an invalid event.",
  queue_full: "This trigger is paused until its queued events are processed.",
  slack_token_invalid: "Use a valid Slack bot token beginning with xoxb-.",
  slack_account_inactive: "This Slack workspace or bot account is inactive.",
  slack_identity_invalid: "Slack did not return a valid workspace and bot identity.",
  slack_connection_failed: "Slack rejected this private app connection.",
  slack_webhook_invalid: "Use an HTTPS Incoming Webhook URL from hooks.slack.com.",
  slack_webhook_validation_required:
    "Incoming Webhook setup is not enabled in this version.",
  slack_app_token_required:
    "Add a Slack app-level token to enable Socket Mode mentions.",
  slack_app_token_invalid:
    "Use a valid Slack app-level token with connections:write.",
  slack_app_token_unused:
    "Enable Socket Mode mentions before adding an app-level token.",
  notion_token_invalid: "Use a valid Notion internal integration token.",
  notion_identity_invalid:
    "Notion did not return a valid workspace and integration identity.",
  notion_connection_failed:
    "Notion could not validate or securely save this internal integration.",
  linear_token_invalid: "Use a valid Linear personal API key beginning with lin_.",
  linear_identity_invalid:
    "Linear did not return a valid user and workspace identity.",
  linear_connection_failed:
    "Linear could not validate or securely save this personal API key.",
  sentry_token_invalid:
    "Use a valid Sentry auth token beginning with sntrys_ or sntryu_.",
  sentry_identity_invalid:
    "Sentry did not return a valid user and organization identity.",
  sentry_scopes_missing:
    "This Sentry token does not grant issue access. Use a token with event:read.",
  sentry_connection_failed:
    "Sentry could not validate or securely save this auth token.",
  obsidian_vault_invalid:
    "Choose a readable Obsidian vault folder containing .obsidian.",
  obsidian_connection_failed:
    "The local Obsidian vault connection could not be saved securely.",
  telegram_token_invalid: "Use a valid token from BotFather.",
  telegram_identity_invalid:
    "Telegram did not return a valid bot identity for this token.",
  telegram_webhook_conflict:
    "This bot already has a webhook. Create a fresh BotFather bot dedicated to Alfred.",
  telegram_connection_exists:
    "Disconnect the current Telegram bot before pairing another one.",
  telegram_pairing_expired:
    "This pairing session expired. Validate the bot token again.",
  telegram_pairing_not_found:
    "Press Start in the opened Telegram chat, then try finishing again.",
  telegram_pairing_ambiguous:
    "More than one pairing message matched. Start pairing again.",
  telegram_private_chat_required:
    "Pair Alfred from a private one-to-one Telegram chat.",
  telegram_test_message_invalid:
    "Enter a test message between 1 and 4,096 characters.",
  telegram_test_failed:
    "Telegram rejected the test notification. Make sure the bot chat is available.",
  telegram_test_delivery_unknown:
    "Telegram may have accepted the test, but Alfred could not confirm it. Start pairing again.",
  telegram_connection_failed: "The Telegram credential could not be saved.",
  github_not_configured:
    "This Alfred build is not configured with the public GitHub App client ID.",
  github_pairing_busy:
    "Too many GitHub authorization attempts are active. Close another attempt and try again.",
  github_pairing_failed: "The GitHub authorization attempt could not be updated.",
  github_pairing_expired: "This GitHub authorization attempt expired. Start again.",
  github_authorization_denied: "GitHub authorization was denied.",
  github_authorization_expired: "GitHub rejected the authorization. Start again.",
  github_invalid_response: "GitHub returned an invalid authorization response.",
  github_identity_invalid: "GitHub did not return a valid user identity.",
  github_installation_required:
    "Install the Alfred GitHub App on at least one repository, then authorize again.",
  github_permissions_missing:
    "The GitHub App installation does not grant repository metadata access.",
  github_unavailable: "GitHub is temporarily unavailable.",
  github_connection_failed: "The GitHub credential could not be saved securely.",
  gmail_not_configured:
    "This Alfred build is not configured with the public Gmail OAuth client ID.",
  gmail_pairing_busy:
    "Too many Gmail authorization attempts are active. Close another attempt and try again.",
  gmail_pairing_failed: "The Gmail authorization attempt could not be updated.",
  gmail_pairing_expired:
    "This Gmail authorization attempt expired. Start again.",
  gmail_pairing_cancelled: "The Gmail authorization attempt was cancelled.",
  gmail_authorization_denied: "Google authorization was not completed.",
  gmail_authorization_expired:
    "Google rejected the authorization. Start a new connection attempt.",
  gmail_offline_access_required:
    "Google did not grant offline access. Authorize again.",
  gmail_account_invalid:
    "Google did not return a valid, verified Gmail account identity.",
  gmail_invalid_response: "Google returned an invalid authorization response.",
  gmail_unavailable: "Google authorization is temporarily unavailable.",
  gmail_connection_failed: "The Gmail credential could not be saved securely.",
  microsoft_not_configured:
    "This Alfred build is not configured with the public Microsoft Entra client ID.",
  microsoft_pairing_busy:
    "Too many Microsoft authorization attempts are active. Close another attempt and try again.",
  microsoft_pairing_failed:
    "The Microsoft authorization attempt could not be updated.",
  microsoft_pairing_expired:
    "This Microsoft authorization attempt expired. Start again.",
  microsoft_pairing_cancelled:
    "The Microsoft authorization attempt was cancelled.",
  microsoft_authorization_denied: "Microsoft authorization was not completed.",
  microsoft_authorization_expired:
    "Microsoft rejected the authorization. Start a new connection attempt.",
  microsoft_offline_access_required:
    "Microsoft did not grant offline access. Authorize again.",
  microsoft_invalid_id_token:
    "Microsoft returned an identity token Alfred could not verify.",
  microsoft_identity_invalid:
    "Microsoft did not return a valid account identity.",
  microsoft_account_mismatch:
    "That Microsoft account does not match the connected account. Sign in with the original account.",
  microsoft_personal_account_blocked:
    "This Alfred build only accepts Microsoft work or school accounts.",
  microsoft_work_account_blocked:
    "This Alfred build only accepts personal Microsoft accounts.",
  microsoft_invalid_response:
    "Microsoft returned an invalid authorization response.",
  microsoft_unavailable: "Microsoft authorization is temporarily unavailable.",
  microsoft_connection_failed:
    "The Microsoft credential could not be saved securely.",
  timed_out: "The provider request timed out.",
  delivery_unknown:
    "The provider may have accepted this action. Check the target before retrying.",
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
  eventDescriptors: AppEventDescriptor[];
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
  connectSlackPrivate: (
    input: SlackPrivateConnectionInput,
  ) => Promise<IntegrationError | null>;
  prepareGithubConnection: () => Promise<GitHubDeviceAuthorization | null>;
  pollGithubConnection: (
    pairingSessionId: string,
  ) => Promise<GitHubDevicePollResult | null>;
  cancelGithubPairing: (pairingSessionId: string) => Promise<void>;
  prepareGmailConnection: () => Promise<GmailAuthorizationStarted | null>;
  completeGmailConnection: (
    sessionId: string,
  ) => Promise<IntegrationError | null>;
  cancelGmailAuthorization: (sessionId: string) => Promise<void>;
  prepareMicrosoftConnection: (
    input: MicrosoftPrepareInput,
  ) => Promise<MicrosoftAuthorizationStarted | null>;
  completeMicrosoftConnection: (
    sessionId: string,
  ) => Promise<IntegrationError | null>;
  cancelMicrosoftAuthorization: (sessionId: string) => Promise<void>;
  connectNotionPrivate: (
    input: NotionPrivateConnectionInput,
  ) => Promise<IntegrationError | null>;
  connectLinearPrivate: (
    input: LinearPrivateConnectionInput,
  ) => Promise<IntegrationError | null>;
  connectSentryPrivate: (
    input: SentryAuthTokenConnectionInput,
  ) => Promise<IntegrationError | null>;
  connectObsidianVault: (
    input: ObsidianVaultConnectionInput,
  ) => Promise<IntegrationError | null>;
  prepareTelegramConnection: (
    input: TelegramPrepareInput,
  ) => Promise<TelegramPairingPrepared | null>;
  completeTelegramConnection: (
    input: TelegramCompleteInput,
  ) => Promise<IntegrationError | null>;
  cancelTelegramPairing: (pairingSessionId: string) => Promise<void>;
  clearError: () => void;
};

export function createIntegrationsState(
  api: IntegrationsApi,
): StateCreator<IntegrationsState> {
  return (set, get) => ({
    providers: [],
    connections: [],
    descriptors: [],
    eventDescriptors: [],
    loading: false,
    disconnectingId: null,
    error: null,

    load: async () => {
      set({ loading: true, error: null });
      try {
        const [providers, connections, descriptors, eventDescriptors] = await Promise.all([
          api.listProviders(),
          api.listConnections(),
          api.listActionDescriptors(),
          api.listEventDescriptors(),
        ]);
        set({
          providers,
          connections: connections.map(redactConnectionPayload),
          descriptors: descriptors.filter(
            (descriptor) =>
              descriptor.fields.every((field) => field.secret === false),
          ),
          eventDescriptors: eventDescriptors.filter((descriptor) =>
            descriptor.filterFields.every((field) => field.secret === false),
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

    connectSlackPrivate: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.connectSlackPrivate(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    prepareGithubConnection: async () => {
      set({ loading: true, error: null });
      try {
        const pairing = await api.prepareGithubConnection();
        set({ loading: false });
        return pairing;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return null;
      }
    },

    pollGithubConnection: async (pairingSessionId) => {
      set({ loading: true, error: null });
      try {
        const result = await api.pollGithubConnection(pairingSessionId);
        if (result.status === "connected") {
          const connected = redactConnectionPayload(result.connection);
          set((state) => ({
            connections: [
              ...state.connections.filter((item) => item.id !== connected.id),
              connected,
            ],
            loading: false,
          }));
          return { ...result, connection: connected };
        }
        set({ loading: false });
        return result;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return null;
      }
    },

    cancelGithubPairing: async (pairingSessionId) => {
      try {
        await api.cancelGithubPairing(pairingSessionId);
      } catch {
        // Device sessions are process-local and expire automatically.
      }
    },

    prepareGmailConnection: async () => {
      set({ loading: true, error: null });
      try {
        const authorization = await api.prepareGmailConnection();
        set({ loading: false });
        return authorization;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return null;
      }
    },

    completeGmailConnection: async (sessionId) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.completeGmailConnection(sessionId),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    cancelGmailAuthorization: async (sessionId) => {
      try {
        await api.cancelGmailAuthorization(sessionId);
      } catch {
        // Authorization sessions are process-local and expire automatically.
      }
    },

    prepareMicrosoftConnection: async (input) => {
      set({ loading: true, error: null });
      try {
        const authorization = await api.prepareMicrosoftConnection(input);
        set({ loading: false });
        return authorization;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return null;
      }
    },

    completeMicrosoftConnection: async (sessionId) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.completeMicrosoftConnection(sessionId),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    cancelMicrosoftAuthorization: async (sessionId) => {
      try {
        await api.cancelMicrosoftAuthorization(sessionId);
      } catch {
        // Authorization sessions are process-local and expire automatically.
      }
    },

    connectNotionPrivate: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.connectNotionPrivate(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    connectLinearPrivate: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.connectLinearPrivate(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    connectSentryPrivate: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.connectSentryPrivate(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    connectObsidianVault: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.connectObsidianVault(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter((item) => item.id !== connected.id),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    prepareTelegramConnection: async (input) => {
      set({ loading: true, error: null });
      try {
        const pairing = await api.prepareTelegramConnection(input);
        set({ loading: false });
        return pairing;
      } catch (error) {
        set({ loading: false, error: normalizeIntegrationError(error) });
        return null;
      }
    },

    completeTelegramConnection: async (input) => {
      set({ loading: true, error: null });
      try {
        const connected = redactConnectionPayload(
          await api.completeTelegramConnection(input),
        );
        set((state) => ({
          connections: [
            ...state.connections.filter(
              (item) => item.providerId !== "telegram",
            ),
            connected,
          ],
          loading: false,
        }));
        return null;
      } catch (error) {
        const normalized = normalizeIntegrationError(error);
        set({ loading: false, error: normalized });
        return normalized;
      }
    },

    cancelTelegramPairing: async (pairingSessionId) => {
      try {
        await api.cancelTelegramPairing(pairingSessionId);
      } catch {
        // Pairing sessions are process-local and expire automatically. Closing
        // the modal must remain safe even if the backend is already gone.
      }
    },

    clearError: () => set({ error: null }),
  });
}

export const useIntegrationsStore = create<IntegrationsState>(
  createIntegrationsState(integrationsApi),
);
