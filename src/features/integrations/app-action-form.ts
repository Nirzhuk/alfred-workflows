import type { IntegrationsApi } from "./api";
import { normalizeIntegrationError } from "./store";
import type {
  ActionDescriptor,
  ActionResourcePage,
  AppConnection,
} from "./types";
import type { AppActionNodeData } from "../workflow/types";

export function selectAppActionProvider(
  data: AppActionNodeData,
  providerId: string,
  connections: AppConnection[],
): AppActionNodeData {
  if (providerId === data.providerId) return data;
  return {
    ...data,
    providerId,
    actionId: "",
    connectionId: defaultConnectionId(connections, providerId),
    input: {},
  };
}

export function selectAppAction(
  data: AppActionNodeData,
  descriptor: ActionDescriptor | null,
  connections: AppConnection[],
): AppActionNodeData {
  const actionId = descriptor?.actionId ?? "";
  if (actionId === data.actionId) return data;
  const input = Object.fromEntries(
    (descriptor?.fields ?? [])
      .filter((field) => field.default !== null)
      .map((field) => [field.key, field.default]),
  );
  return {
    ...data,
    actionId,
    connectionId: defaultConnectionId(connections, data.providerId),
    input,
  };
}

export function compatibleConnections(
  connections: AppConnection[],
  providerId: string,
): AppConnection[] {
  return connections.filter(
    (connection) => connection.providerId === providerId,
  );
}

// Blank when zero or multiple connected accounts exist; unambiguous otherwise.
export function defaultConnectionId(
  connections: AppConnection[],
  providerId: string,
): string {
  const connected = compatibleConnections(connections, providerId).filter(
    (connection) => connection.status === "connected",
  );
  return connected.length === 1 ? connected[0].id : "";
}

export function unknownActionInputKeys(
  data: AppActionNodeData,
  descriptor: ActionDescriptor,
): string[] {
  const known = new Set(
    descriptor.fields.flatMap((field) =>
      field.kind === "resource_selector"
        ? [field.key, `${field.key}__display`]
        : [field.key],
    ),
  );
  return Object.keys(data.input).filter((key) => !known.has(key));
}

export function validateAppActionForm(
  data: AppActionNodeData,
  descriptor: ActionDescriptor | null,
): string[] {
  const errors: string[] = [];
  if (!data.providerId) errors.push("Choose a provider.");
  if (!data.actionId) errors.push("Choose an action.");
  if (!data.connectionId) errors.push("Choose a connected account.");
  if (!descriptor) {
    if (data.actionId) errors.push("This action is unavailable in this Alfred version.");
    return errors;
  }
  for (const field of descriptor.fields) {
    const value = data.input[field.key];
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
          : typeof value === "string" && value.length <= 32 * 1024;
    if (!valid) {
      errors.push(`${field.label} has an invalid value.`);
    }
  }
  return errors;
}

export type ResourceLoadResult =
  | { page: ActionResourcePage; error: null }
  | { page: null; error: string };

export async function loadActionResourceOptions(
  api: IntegrationsApi,
  request: Parameters<IntegrationsApi["listActionResources"]>[0],
): Promise<ResourceLoadResult> {
  try {
    return { page: await api.listActionResources(request), error: null };
  } catch (error) {
    return { page: null, error: normalizeIntegrationError(error).message };
  }
}
