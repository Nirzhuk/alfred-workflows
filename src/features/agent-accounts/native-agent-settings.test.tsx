import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  cleanupMessageFor,
  disconnectMessageFor,
  NativeAgentSettings,
} from "./native-agent-settings";
import type { AgentAccount, AgentProviderRegistration } from "./types";

test("shows gated providers and safe lifecycle actions separately from Connected Apps", () => {
  const providers: AgentProviderRegistration[] = [
      {
        providerId: "codex",
        providerName: "Codex",
        harness: "alfred",
        authMethods: ["chatgpt_oauth", "chatgpt_device_code"],
        billingSource: "chatgpt_subscription",
        credentialCustody: "runtime_managed",
        connectAvailable: false,
        gateCode: "native_provider_not_available",
      },
    ];
  const accounts: AgentAccount[] = [
      {
        id: "account_opaque",
        providerId: "codex",
        providerName: "Codex",
        harness: "alfred",
        displayName: "Work account",
        externalAccountId: null,
        externalWorkspaceId: null,
        authMethod: "oauth_pkce",
        custodyMode: "alfred_managed",
        scopes: [],
        status: "error",
        expiresAt: null,
        lastCheckedAt: null,
        lastErrorCode: "provider_unavailable",
        createdAt: "now",
        updatedAt: "now",
      },
    ];

  const markup = renderToStaticMarkup(
    <NativeAgentSettings snapshot={{ providers: [...providers], accounts: [...accounts] }} />,
  );
  expect(markup).toContain("Native agent accounts");
  expect(markup).toContain("Alfred harness");
  expect(markup).toContain("Native account support is gated");
  expect(markup).not.toContain("Reconnect");
  expect(markup).not.toContain("Refresh");
  expect(markup).toContain("Disconnect");
  expect(markup).not.toContain("credentialRef");
  expect(markup).not.toContain("token");
});

test("shows truthful Cursor cloud disclosure without the frozen runtime labels", () => {
  const providers: AgentProviderRegistration[] = [
    {
      providerId: "cursor",
      providerName: "Cursor",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "cursor_cloud_agents_api",
      credentialCustody: "alfred_managed",
      connectAvailable: false,
      gateCode: "native_provider_not_available",
    },
  ];
  const accounts: AgentAccount[] = [
    {
      id: "account_cursor_gated",
      providerId: "cursor",
      providerName: "Cursor",
      harness: "alfred",
      displayName: "Cloud repository",
      externalAccountId: null,
      externalWorkspaceId: null,
      authMethod: "runtime",
      custodyMode: "runtime_managed",
      scopes: [],
      status: "error",
      expiresAt: null,
      lastCheckedAt: null,
      lastErrorCode: "native_provider_not_available",
      createdAt: "now",
      updatedAt: "now",
    },
  ];

  const markup = renderToStaticMarkup(
    <NativeAgentSettings snapshot={{ providers, accounts }} />,
  );
  expect(markup).toContain("Cursor API key");
  expect(markup).toContain("Cursor Cloud");
  expect(markup).toContain("remote repository");
  expect(markup).toContain("alfred managed");
  expect(markup).not.toContain("Provider runtime");
  expect(markup).not.toContain("Isolated runtime credential");
  expect(markup).not.toContain("Reconnect");
});

test("disconnect copy distinguishes local key deletion from provider revocation", () => {
  const apiKeyAccount: AgentAccount = {
    id: "account_grok",
    providerId: "grok",
    providerName: "Grok",
    harness: "alfred",
    displayName: null,
    externalAccountId: null,
    externalWorkspaceId: null,
    authMethod: "oauth_pkce",
    custodyMode: "alfred_managed",
    scopes: [],
    status: "connected",
    expiresAt: null,
    lastCheckedAt: null,
    lastErrorCode: null,
    createdAt: "now",
    updatedAt: "now",
  };
  const message = disconnectMessageFor(apiKeyAccount);
  expect(message).toContain("delete its locally stored Grok API key");
  expect(message).toContain("does not revoke or rotate");
  expect(message).toContain("provider console");

  const runtimeMessage = disconnectMessageFor({
    ...apiKeyAccount,
    providerId: "opencode",
    providerName: "OpenCode",
    custodyMode: "runtime_managed",
  });
  expect(runtimeMessage).toContain("isolated provider runtime to sign out");
  expect(runtimeMessage).toContain("sessions may remain active");
  expect(cleanupMessageFor(apiKeyAccount)).toContain(
    "Revoke or rotate the key in the provider console",
  );
  expect(cleanupMessageFor({
    ...apiKeyAccount,
    providerId: "opencode",
    custodyMode: "runtime_managed",
  })).toContain("provider runtime could not finish sign-out");
});

test("shows the Grok API billing boundary without implying subscription reuse", () => {
  const providers: AgentProviderRegistration[] = [
    {
      providerId: "grok",
      providerName: "Grok",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "xai_api_usage_based",
      credentialCustody: "alfred_managed",
      connectAvailable: false,
      gateCode: "native_provider_not_available",
    },
  ];

  const markup = renderToStaticMarkup(
    <NativeAgentSettings snapshot={{ providers, accounts: [] }} />,
  );
  expect(markup).toContain("xAI API key");
  expect(markup).toContain("billed to your xAI API team");
  expect(markup).toContain("Grok subscription");
  expect(markup).not.toContain("OAuth with PKCE");
});
