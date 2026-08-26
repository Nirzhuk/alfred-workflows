import { invoke } from "@tauri-apps/api/core";
import type {
  AgentAccount,
  AgentAuthorizationStarted,
  AgentHarness,
  AgentProductId,
  AgentProviderRegistration,
} from "./types";

export type AgentAccountsApi = {
  listProviders: () => Promise<AgentProviderRegistration[]>;
  listAccounts: () => Promise<AgentAccount[]>;
  getAccount: (id: string) => Promise<AgentAccount | null>;
  startAuthorization: (
    providerId: string,
    productId: AgentProductId,
    harness: AgentHarness,
  ) => Promise<AgentAuthorizationStarted>;
  completeAuthorization: (
    attemptId: string,
    providerId: string,
    productId: AgentProductId,
    harness: AgentHarness,
    completionState: string | null,
  ) => Promise<AgentAccount>;
  cancelAuthorization: (attemptId: string) => Promise<void>;
  connectApiKeyAccount: (
    providerId: string,
    productId: AgentProductId,
    harness: AgentHarness,
    accountId: string | null,
    apiKey: string,
  ) => Promise<AgentAccount>;
  refreshAccount: (id: string) => Promise<AgentAccount>;
  disconnectAccount: (id: string, metadataOnly: boolean) => Promise<void>;
};

export const agentAccountsApi: AgentAccountsApi = {
  listProviders: () => invoke("list_agent_account_providers"),
  listAccounts: () => invoke("list_agent_accounts"),
  getAccount: (id) => invoke("get_agent_account", { id }),
  startAuthorization: (providerId, productId, harness) =>
    invoke("start_agent_authorization", { providerId, productId, harness }),
  completeAuthorization: (attemptId, providerId, productId, harness, completionState) =>
    invoke("complete_agent_authorization", {
      attemptId,
      providerId,
      productId,
      harness,
      completionState,
    }),
  cancelAuthorization: (attemptId) =>
    invoke("cancel_agent_authorization", { attemptId }),
  connectApiKeyAccount: (providerId, productId, harness, accountId, apiKey) =>
    invoke("connect_agent_api_key_account", {
      providerId,
      productId,
      harness,
      accountId,
      apiKey,
    }),
  refreshAccount: (id) => invoke("refresh_agent_account", { id }),
  disconnectAccount: (id, metadataOnly) =>
    invoke("disconnect_agent_account", { id, metadataOnly }),
};
