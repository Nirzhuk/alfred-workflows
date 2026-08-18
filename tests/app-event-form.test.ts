import { describe, expect, test } from "bun:test";
import {
  compatibleEventConnections,
  emptyAppTriggerConfig,
  loadEventResourceOptions,
  selectAppEvent,
  selectAppEventProvider,
  validateAppEventForm,
} from "../src/features/integrations/app-event-form";
import type {
  AppConnection,
  AppEventDescriptor,
} from "../src/features/integrations/types";
import type { IntegrationsApi } from "../src/features/integrations/api";

const descriptor: AppEventDescriptor = {
  providerId: "slack",
  eventType: "slack.app_mention",
  label: "App mention",
  description: "A bot was mentioned.",
  requiredScopes: ["app_mentions:read"],
  deliveryModes: ["socket"],
  filterFields: [
    {
      key: "channel",
      label: "Channel",
      description: "Optional channel filter.",
      kind: "resource_selector",
      required: false,
      default: null,
      secret: false,
      optionSource: "conversations",
      options: [],
      supportsInterpolation: false,
    },
  ],
  fetchesResourceContent: false,
  descriptorVersion: 1,
  externalEventIdRequired: true,
  allowedAttributeKeys: ["channelId"],
  pollIntervalSeconds: 1,
  pendingCap: 100,
};

function connection(scopes: string[], providerId = "slack"): AppConnection {
  return {
    id: scopes.join("-") || "none",
    providerId,
    displayName: "Workspace",
    externalAccountId: null,
    externalTenantId: null,
    connectionMode: "private_bot",
    scopes,
    status: "connected",
    expiresAt: null,
    lastCheckedAt: null,
    lastErrorCode: null,
    createdAt: "now",
    updatedAt: "now",
  };
}

describe("connected app trigger configuration", () => {
  test("provider and event changes reset dependent values", () => {
    const configured = {
      ...emptyAppTriggerConfig(),
      providerId: "slack",
      eventType: descriptor.eventType,
      connectionId: "connection",
      filters: { channel: "C1" },
    };
    expect(selectAppEventProvider(configured, "github")).toMatchObject({
      providerId: "github",
      eventType: "",
      connectionId: "",
      filters: {},
    });
    expect(selectAppEvent(emptyAppTriggerConfig(), descriptor)).toMatchObject({
      eventType: descriptor.eventType,
      descriptorVersion: 1,
      connectionId: "",
    });
  });

  test("connections are filtered by provider and exact event scopes", () => {
    expect(
      compatibleEventConnections(
        [
          connection(["app_mentions:read"]),
          connection(["chat:write"]),
          connection(["app_mentions:read"], "github"),
        ],
        descriptor,
      ),
    ).toHaveLength(1);
  });

  test("missing descriptors and invalid filters fail safely", () => {
    const config = {
      ...emptyAppTriggerConfig(),
      providerId: "slack",
      eventType: descriptor.eventType,
      connectionId: "connection",
      filters: { channel: false },
    };
    expect(validateAppEventForm(config, null)).toContain(
      "This event is unavailable in this Alfred version.",
    );
    expect(validateAppEventForm(config, descriptor)).toContain(
      "Channel has an invalid value.",
    );
    expect(
      descriptor.filterFields.every((field) => field.secret === false),
    ).toBe(true);
  });

  test("resource selector loading normalizes provider failures", async () => {
    const api = {
      listEventResources: async () => ({
        items: [{ id: "C1", label: "Engineering" }],
        nextPageToken: null,
      }),
    } as IntegrationsApi;
    const loaded = await loadEventResourceOptions(api, {
      connectionId: "connection",
      providerId: "slack",
      eventType: descriptor.eventType,
      fieldKey: "channel",
      query: "eng",
    });
    expect(loaded.page?.items[0]?.id).toBe("C1");

    const failed = await loadEventResourceOptions(
      {
        listEventResources: async () => {
          throw { code: "rate_limited", message: "raw fixture" };
        },
      } as IntegrationsApi,
      {
        connectionId: "connection",
        providerId: "slack",
        eventType: descriptor.eventType,
        fieldKey: "channel",
        query: "",
      },
    );
    expect(failed.page).toBeNull();
    expect(failed.error).toContain("rate limiting");
    expect(failed.error).not.toContain("raw fixture");
  });
});
