import { expect, test } from "bun:test";
import { usesAlfredManagedApiKey } from "../../types";

test("classifies local API keys from backend auth and custody metadata", () => {
  expect(usesAlfredManagedApiKey(["api_key"], "alfred_managed")).toBe(true);
  expect(usesAlfredManagedApiKey("api_key", "alfred_managed")).toBe(true);
  expect(usesAlfredManagedApiKey("api_key", "runtime_managed")).toBe(false);
  expect(usesAlfredManagedApiKey("device_code", "alfred_managed")).toBe(false);
});
