import type { AppTriggerConfig } from "../workflow/types";
import type { IntegrationsApi } from "./api";
import { normalizeIntegrationError } from "./store";
import type {
  AppConnection,
  AppEventDescriptor,
  AppEventResourcePage,
} from "./types";

export function emptyAppTriggerConfig(): AppTriggerConfig {
  return {
    providerId: "",
    eventType: "",
    connectionId: "",
    filters: {},
    descriptorVersion: 1,
  };
}

export function selectAppEventProvider(
  config: AppTriggerConfig,
  providerId: string,
): AppTriggerConfig {
  if (providerId === config.providerId) return config;
  return {
    ...config,
    providerId,
    eventType: "",
    connectionId: "",
    filters: {},
    descriptorVersion: 1,
  };
}

export function selectAppEvent(
  config: AppTriggerConfig,
  descriptor: AppEventDescriptor | null,
): AppTriggerConfig {
  if (descriptor?.eventType === config.eventType) return config;
  return {
    ...config,
    eventType: descriptor?.eventType ?? "",
    connectionId: "",
    filters: Object.fromEntries(
      (descriptor?.filterFields ?? [])
        .filter((field) => field.default !== null)
        .map((field) => [field.key, field.default]),
    ),
    descriptorVersion: descriptor?.descriptorVersion ?? 1,
  };
}

export function compatibleEventConnections(
  connections: AppConnection[],
  descriptor: AppEventDescriptor | null,
): AppConnection[] {
  if (!descriptor) return [];
  return connections.filter(
    (connection) =>
      connection.providerId === descriptor.providerId &&
      descriptor.requiredScopes.every((scope) =>
        connection.scopes.includes(scope),
      ),
  );
}

export function validateAppEventForm(
  config: AppTriggerConfig,
  descriptor: AppEventDescriptor | null,
): string[] {
  const errors: string[] = [];
  if (!config.providerId) errors.push("Choose a provider.");
  if (!config.eventType) errors.push("Choose an event.");
  if (!config.connectionId) errors.push("Choose a connected account.");
  if (!descriptor) {
    if (config.eventType) {
      errors.push("This event is unavailable in this Alfred version.");
    }
    return errors;
  }
  for (const field of descriptor.filterFields) {
    const value = config.filters[field.key];
    if (
      field.required &&
      (value === undefined || value === null || value === "")
    ) {
      errors.push(`${field.label} is required.`);
      continue;
    }
    if (value === undefined || value === null || value === "") continue;
    const valid =
      field.kind === "boolean"
        ? typeof value === "boolean"
        : field.kind === "enum"
          ? typeof value === "string" &&
            field.options.some((option) => option.id === value)
          : typeof value === "string" && value.length <= 2_048;
    if (!valid) errors.push(`${field.label} has an invalid value.`);
  }
  return errors;
}

export type EventResourceLoadResult =
  | { page: AppEventResourcePage; error: null }
  | { page: null; error: string };

export async function loadEventResourceOptions(
  api: IntegrationsApi,
  request: Parameters<IntegrationsApi["listEventResources"]>[0],
): Promise<EventResourceLoadResult> {
  try {
    return { page: await api.listEventResources(request), error: null };
  } catch (error) {
    return { page: null, error: normalizeIntegrationError(error).message };
  }
}
