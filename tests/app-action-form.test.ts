import { describe, expect, test } from "bun:test";
import type { IntegrationsApi } from "../src/features/integrations/api";
import {
  compatibleConnections,
  defaultConnectionId,
  loadActionResourceOptions,
  selectAppAction,
  selectAppActionProvider,
  unknownActionInputKeys,
  validateAppActionForm,
} from "../src/features/integrations/app-action-form";
import type {
  ActionDescriptor,
  AppConnection,
} from "../src/features/integrations/types";
import {
  defaultAppActionNodeData,
  type AppActionNodeData,
} from "../src/features/workflow/types";

const descriptor: ActionDescriptor = {
  providerId: "slack",
  actionId: "slack.send_message",
  label: "Send message",
  description: "Send a message",
  fields: [
    {
      key: "channel",
      label: "Channel",
      description: "Destination",
      kind: "resource_selector",
      required: true,
      default: null,
      secret: false,
      optionSource: "channels",
      options: [],
      supportsInterpolation: false,
    },
    {
      key: "message",
      label: "Message",
      description: "Text",
      kind: "textarea",
      required: true,
      default: "{{output}}",
      secret: false,
      optionSource: null,
      options: [],
      supportsInterpolation: true,
    },
  ],
  requiredScopes: ["chat:write"],
  outputSchemaVersion: 1,
  outputIsUntrusted: false,
};

const connected: AppConnection = {
  id: "slack-connection",
  providerId: "slack",
  displayName: "Workspace",
  externalAccountId: null,
  externalTenantId: null,
  connectionMode: "native_oauth",
  scopes: ["chat:write"],
  status: "connected",
  expiresAt: null,
  lastCheckedAt: null,
  lastErrorCode: null,
  createdAt: "now",
  updatedAt: "now",
};

function api(
  listActionResources: IntegrationsApi["listActionResources"],
): IntegrationsApi {
  return {
    listProviders: async () => [],
    listConnections: async () => [],
    getConnection: async () => null,
    getUsage: async () => ({ workflows: [], schedules: [], triggers: [] }),
    disconnect: async () => {},
    listActionDescriptors: async () => [descriptor],
    listEventDescriptors: async () => [],
    listEventResources: async () => ({ items: [], nextPageToken: null }),
    connectSlackPrivate: async () => ({
      id: "connection",
      providerId: "slack",
      displayName: "Workspace",
      externalAccountId: null,
      externalTenantId: null,
      connectionMode: "private_bot",
      scopes: [],
      status: "connected",
      expiresAt: null,
      lastCheckedAt: null,
      lastErrorCode: null,
      createdAt: "now",
      updatedAt: "now",
    }),
    listActionResources,
  };
}

describe("app action configuration", () => {
  test("provider and action changes default the connection when exactly one account is connected", () => {
    const configured: AppActionNodeData = {
      ...defaultAppActionNodeData(),
      providerId: "gmail",
      actionId: "gmail.send",
      connectionId: "old",
      input: { body: "old" },
    };
    const providerChanged = selectAppActionProvider(configured, "slack", [
      connected,
    ]);
    expect(providerChanged).toMatchObject({
      providerId: "slack",
      actionId: "",
      connectionId: connected.id,
      input: {},
    });

    const actionChanged = selectAppAction(providerChanged, descriptor, [
      connected,
    ]);
    expect(actionChanged.actionId).toBe("slack.send_message");
    expect(actionChanged.connectionId).toBe(connected.id);
    expect(actionChanged.input).toEqual({ message: "{{output}}" });
  });

  test("leaves the connection blank when zero or multiple accounts are connected", () => {
    const configured: AppActionNodeData = {
      ...defaultAppActionNodeData(),
      providerId: "gmail",
      actionId: "gmail.send",
      connectionId: "old",
      input: { body: "old" },
    };
    expect(
      selectAppActionProvider(configured, "slack", []).connectionId,
    ).toBe("");

    const second: AppConnection = { ...connected, id: "slack-connection-2" };
    expect(
      selectAppActionProvider(configured, "slack", [connected, second])
        .connectionId,
    ).toBe("");

    expect(defaultConnectionId([connected], "slack")).toBe(connected.id);
    expect(defaultConnectionId([connected], "gmail")).toBe("");
    expect(
      defaultConnectionId(
        [connected, { ...connected, status: "expired" }],
        "slack",
      ),
    ).toBe(connected.id);
  });

  test("filters connections by provider without hiding unhealthy metadata", () => {
    const rows = compatibleConnections(
      [
        connected,
        { ...connected, id: "expired", status: "expired" },
        { ...connected, id: "gmail", providerId: "gmail" },
      ],
      "slack",
    );
    expect(rows.map((row) => row.id)).toEqual(["slack-connection", "expired"]);
  });

  test("keeps unknown newer inputs and fails unknown descriptors safely", () => {
    const data: AppActionNodeData = {
      ...defaultAppActionNodeData(),
      providerId: "slack",
      actionId: descriptor.actionId,
      connectionId: connected.id,
      input: {
        channel: "C1",
        channel__display: "Engineering",
        message: "Hi",
        futureField: 42,
      },
    };
    expect(unknownActionInputKeys(data, descriptor)).toEqual(["futureField"]);
    expect(validateAppActionForm(data, null)).toContain(
      "This action is unavailable in this Alfred version.",
    );
    const graph = {
      nodes: [
        {
          id: "action-1",
          type: "appAction",
          position: { x: 10, y: 20 },
          data,
        },
      ],
      edges: [],
    };
    const reopened = JSON.parse(JSON.stringify(graph));
    expect(reopened.nodes[0].data).toEqual(data);
    expect(reopened.nodes[0].data.input.futureField).toBe(42);
  });

  test("models every supported descriptor field without a secret kind", () => {
    const supported = new Set([
      "text",
      "textarea",
      "boolean",
      "enum",
      "resource_selector",
    ]);
    expect(descriptor.fields.every((field) => supported.has(field.kind))).toBe(true);
    expect(descriptor.fields.every((field) => field.secret === false)).toBe(true);
    expect(supported.has("secret")).toBe(false);
  });

  test("validates descriptor field types before execution", () => {
    const enumDescriptor: ActionDescriptor = {
      ...descriptor,
      fields: [
        {
          key: "notify",
          label: "Notify",
          description: "",
          kind: "boolean",
          required: true,
          default: false,
          secret: false,
          optionSource: null,
          options: [],
          supportsInterpolation: false,
        },
        {
          key: "priority",
          label: "Priority",
          description: "",
          kind: "enum",
          required: true,
          default: "normal",
          secret: false,
          optionSource: null,
          options: [{ id: "normal", label: "Normal" }],
          supportsInterpolation: false,
        },
      ],
    };
    const invalid: AppActionNodeData = {
      ...defaultAppActionNodeData(),
      providerId: "slack",
      actionId: enumDescriptor.actionId,
      connectionId: connected.id,
      input: { notify: "yes", priority: "urgent" },
    };
    expect(validateAppActionForm(invalid, enumDescriptor)).toEqual([
      "Notify has an invalid value.",
      "Priority has an invalid value.",
    ]);
  });

  test("loads resource selectors and normalizes failures", async () => {
    const request = {
      connectionId: connected.id,
      providerId: "slack",
      actionId: descriptor.actionId,
      fieldKey: "channel",
      query: "eng",
    };
    const success = await loadActionResourceOptions(
      api(async () => ({
        items: [{ id: "C1", label: "Engineering" }],
        nextPageToken: "next",
      })),
      request,
    );
    expect(success.page?.items[0]?.label).toBe("Engineering");

    const failure = await loadActionResourceOptions(
      api(async () => {
        throw {
          code: "provider_unavailable",
          message: "Bearer secret fixture",
        };
      }),
      request,
    );
    expect(failure.page).toBeNull();
    expect(failure.error).toBe("The provider is temporarily unavailable.");
    expect(failure.error).not.toContain("secret fixture");
  });
});
