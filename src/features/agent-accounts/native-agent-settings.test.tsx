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
        productId: "chatgpt_codex",
        productName: "ChatGPT Codex",
        harness: "alfred",
        authMethods: ["oauth_pkce", "device_code"],
        billingSource: "provider_subscription",
        billingOwner: "subscription_account",
        credentialCustody: "runtime_managed",
        managedRuntimeId: "codex_python_sdk",
        managedRuntimeVersion: "0.147.0",
        connectAvailable: false,
        gateCode: "native_provider_not_available",
      },
    ];
  const accounts: AgentAccount[] = [
      {
        id: "account_opaque",
        providerId: "codex",
        providerName: "Codex",
        productId: "chatgpt_codex",
        productName: "ChatGPT Codex",
        harness: "alfred",
        displayName: "Work account",
        externalAccountId: null,
        externalWorkspaceId: null,
        authMethod: "oauth_pkce",
        custodyMode: "runtime_managed",
        managedRuntimeId: "codex_python_sdk",
        managedRuntimeVersion: "0.147.0",
        scopes: [],
        billingSource: "provider_subscription",
        billingOwner: "subscription_account",
        entitlementState: "unknown",
        entitlementSource: "provider_unobserved",
        entitlementObservedAt: null,
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
  expect(markup).toContain("agent-mark-codex");
  expect(markup).toContain("agent-mark-glyph");
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
      productId: "cursor_cloud",
      productName: "Cursor Cloud",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      credentialCustody: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      connectAvailable: false,
      gateCode: "native_provider_not_available",
    },
  ];
  const accounts: AgentAccount[] = [
    {
      id: "account_cursor_gated",
      providerId: "cursor",
      providerName: "Cursor",
      productId: "cursor_cloud",
      productName: "Cursor Cloud",
      harness: "alfred",
      displayName: "Cloud repository",
      externalAccountId: null,
      externalWorkspaceId: null,
      authMethod: "api_key",
      custodyMode: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      scopes: [],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      entitlementState: "unknown",
      entitlementSource: "provider_unobserved",
      entitlementObservedAt: null,
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
    productId: "grok_api",
    productName: "Grok API",
    harness: "alfred",
    displayName: null,
    externalAccountId: null,
    externalWorkspaceId: null,
    authMethod: "api_key",
    custodyMode: "alfred_managed",
    managedRuntimeId: null,
    managedRuntimeVersion: null,
    scopes: [],
    billingSource: "provider_api",
    billingOwner: "credential_owner",
    entitlementState: "unknown",
    entitlementSource: "provider_unobserved",
    entitlementObservedAt: null,
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

  const zenMessage = disconnectMessageFor({
    ...apiKeyAccount,
    providerId: "opencode",
    providerName: "OpenCode",
    productId: "opencode_zen",
    productName: "OpenCode Zen",
    managedRuntimeId: "opencode_server",
    managedRuntimeVersion: "1.18.23",
    billingSource: "provider_payg",
  });
  expect(zenMessage).toContain("delete its locally stored OpenCode API key");
  expect(zenMessage).toContain("does not revoke or rotate");
  expect(zenMessage).not.toContain("OAuth");

  const runtimeMessage = disconnectMessageFor({
    ...apiKeyAccount,
    providerId: "opencode",
    providerName: "OpenCode",
    productId: "opencode_go",
    productName: "OpenCode Go",
    custodyMode: "runtime_managed",
    managedRuntimeId: "opencode_server",
    managedRuntimeVersion: "1.18.23",
    billingSource: "provider_subscription",
    billingOwner: "subscription_account",
  });
  expect(runtimeMessage).toContain("isolated provider runtime to sign out");
  expect(runtimeMessage).toContain("sessions may remain active");
  expect(cleanupMessageFor(apiKeyAccount)).toContain(
    "Revoke or rotate the key in the provider console",
  );
  expect(cleanupMessageFor({
    ...apiKeyAccount,
    providerId: "opencode",
    productId: "opencode_go",
    productName: "OpenCode Go",
    custodyMode: "runtime_managed",
    managedRuntimeId: "opencode_server",
    managedRuntimeVersion: "1.18.23",
    billingSource: "provider_subscription",
    billingOwner: "subscription_account",
  })).toContain("provider runtime could not finish sign-out");
});

test("shows the Grok API billing boundary without implying subscription reuse", () => {
  const providers: AgentProviderRegistration[] = [
    {
      providerId: "grok",
      providerName: "Grok",
      productId: "grok_api",
      productName: "Grok API",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      credentialCustody: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
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

test("offers the dedicated API-key intake for exactly Claude, Gemini, and Grok", () => {

  const providers: AgentProviderRegistration[] = [
    {
      providerId: "claude_code",
      providerName: "Claude",
      productId: "claude_api",
      productName: "Claude API",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      credentialCustody: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      connectAvailable: true,
      gateCode: "claude_live_api_key_smoke_missing",
    },
    {
      providerId: "gemini",
      providerName: "Gemini",
      productId: "gemini_api",
      productName: "Gemini API",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      credentialCustody: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      connectAvailable: true,
      gateCode: "gemini_live_api_key_smoke_missing",
    },
    {
      providerId: "grok",
      providerName: "Grok",
      productId: "grok_api",
      productName: "Grok API",
      harness: "alfred",
      authMethods: ["api_key"],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      credentialCustody: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      connectAvailable: true,
      gateCode: "grok_live_api_key_smoke_missing",
    },
  ];
  const accounts: AgentAccount[] = [
    {
      id: "account_claude",
      providerId: "claude_code",
      providerName: "Claude",
      productId: "claude_api",
      productName: "Claude API",
      harness: "alfred",
      displayName: "API key",
      externalAccountId: null,
      externalWorkspaceId: null,
      authMethod: "api_key",
      custodyMode: "alfred_managed",
      managedRuntimeId: null,
      managedRuntimeVersion: null,
      scopes: [],
      billingSource: "provider_api",
      billingOwner: "credential_owner",
      entitlementState: "unknown",
      entitlementSource: "provider_unobserved",
      entitlementObservedAt: null,
      status: "connected",
      expiresAt: null,
      lastCheckedAt: null,
      lastErrorCode: null,
      createdAt: "now",
      updatedAt: "now",
    },
  ];

  const markup = renderToStaticMarkup(
    <NativeAgentSettings snapshot={{ providers, accounts }} />,
  );
  expect(markup).toContain("Anthropic API key");
  expect(markup).toContain("Gemini API key");
  expect(markup).toContain("xAI API key");
  expect(markup).toContain("Reconnect");
  expect(markup).toContain("Native Claude runs remain blocked");
  expect(markup).toContain("Native Gemini runs remain blocked");
  expect(markup).toContain("Native Grok runs remain blocked");
  expect(markup).not.toContain(">Refresh<");
  expect(markup).not.toContain("sk-ant");
});
