import { invoke } from "@tauri-apps/api/core";
import type {
  ActionDescriptor,
  ActionResourcePage,
  AppConnection,
  AppConnectionUsage,
  AppProvider,
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
};
