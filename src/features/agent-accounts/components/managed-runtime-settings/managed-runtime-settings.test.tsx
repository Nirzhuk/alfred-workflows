import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ManagedRuntimeSettings, isSafeAuthorizationUrl } from "./managed-runtime-settings";
import type { ManagedRuntimeConnectionStatus, ManagedRuntimeProduct } from "../../managed-runtime-types";

const blockedProduct: ManagedRuntimeProduct = {
  providerId: "codex",
  productId: "chatgpt_codex",
  productName: "ChatGPT Codex",
  runtimeId: "codex_python_sdk",
  runtimeVersion: "0.147.0",
  installState: "blocked",
  connectionKind: "browser",
  connectAvailable: false,
  gateCodes: ["codex_python_sdk_sealed_package_unverified"],
  billingSource: "provider_subscription",
  custodyMode: "runtime_managed",
};

const blockedStatus: ManagedRuntimeConnectionStatus = {
  providerId: "codex",
  productId: "chatgpt_codex",
  installState: "blocked",
  connectionState: "disconnected",
  accountId: null,
  entitlementState: "unknown",
  lastErrorCode: null,
};

test("renders product-first blocked state without advertising a runtime action", () => {
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{ products: [blockedProduct], statuses: [blockedStatus] }}
    />,
  );
  expect(markup).toContain("Managed subscriptions");
  expect(markup).toContain("ChatGPT Codex");
  expect(markup).toContain("Blocked");
  expect(markup).toContain("The verified Codex runtime package is not available");
  expect(markup).not.toContain(">Connect<");
  expect(markup).not.toContain(">Install<");
  expect(markup).not.toContain("provider-token");
  expect(markup).not.toContain("apiKey");
});

test("accepts only HTTPS authorization URLs", () => {
  expect(isSafeAuthorizationUrl("https://chatgpt.com/auth")).toBe(true);
  expect(isSafeAuthorizationUrl("http://chatgpt.com/auth")).toBe(false);
  expect(isSafeAuthorizationUrl("javascript:alert(1)")).toBe(false);
});

