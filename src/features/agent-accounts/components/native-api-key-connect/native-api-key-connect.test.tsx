import { expect, test } from "bun:test";
import { isNativeApiKeyProvider } from "./native-api-key-connect";

test("accepts only the approved native API-key providers", () => {
  expect(isNativeApiKeyProvider("claude_code")).toBe(true);
  expect(isNativeApiKeyProvider("gemini")).toBe(true);
  expect(isNativeApiKeyProvider("grok")).toBe(true);
  expect(isNativeApiKeyProvider("cursor")).toBe(false);
  expect(isNativeApiKeyProvider("codex")).toBe(false);
  expect(isNativeApiKeyProvider("toString")).toBe(false);
  expect(isNativeApiKeyProvider("__proto__")).toBe(false);
});

