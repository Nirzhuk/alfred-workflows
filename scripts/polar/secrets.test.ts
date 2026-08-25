import { describe, expect, test } from "bun:test";
import {
  loadSandboxTestKeys,
  loadSandboxTestKeysFromFile,
  parseSandboxTestKeys,
  readSandboxTestKeysFromEnv,
  SandboxSecretsError,
  SECRET_ENV_VARS,
} from "./secrets";

const KEY_VALUE = "supporter-private-value";

function envFixture(): Record<string, string | undefined> {
  return {
    [SECRET_ENV_VARS.supporter]: KEY_VALUE,
  };
}

describe("sandbox secret input", () => {
  test("parses the supporter key without transforming its value", () => {
    const keys = parseSandboxTestKeys({ supporter: KEY_VALUE });

    expect(keys.supporter).toBe(KEY_VALUE);
  });

  test("still loads a secrets file written with the retired individual name", () => {
    // The supporter class deliberately reuses the previously individual-named
    // slot so an operator's existing secret setup keeps working.
    const keys = parseSandboxTestKeys({ individual: KEY_VALUE });
    expect(keys.supporter).toBe(KEY_VALUE);
  });

  test("rejects incomplete input and paths that are not ignored local files", async () => {
    expect(() => parseSandboxTestKeys({})).toThrow(SandboxSecretsError);

    await expect(
      loadSandboxTestKeysFromFile("scripts/polar/sandbox-secrets.json"),
    ).rejects.toBeInstanceOf(SandboxSecretsError);
  });

  test("reads the key from a secret runner's environment", () => {
    expect(readSandboxTestKeysFromEnv(envFixture())).toEqual({
      supporter: KEY_VALUE,
    });
  });

  test("falls through to the local file only when no variable is set", async () => {
    expect(readSandboxTestKeysFromEnv({})).toBeNull();
    expect(
      readSandboxTestKeysFromEnv({ UNRELATED: "x", EMPTY: "" }),
    ).toBeNull();

    // No environment and no readable file: it must fail, not pass blank.
    // Point at an absent *.local path rather than the default, because an
    // operator running these tests may have a real sandbox-secrets.json.local
    // on disk and this assertion must not depend on their machine.
    await expect(
      loadSandboxTestKeys({
        env: {},
        file: new URL("./absent-secrets.json.local", import.meta.url),
      }),
    ).rejects.toBeInstanceOf(SandboxSecretsError);
  });

  test("never puts a key value into an error message", () => {
    let message = "";
    try {
      parseSandboxTestKeys({ supporter: "short" });
    } catch (error) {
      message = String(error);
    }
    expect(message).toContain("supporter");
    expect(message).not.toContain("short");
  });
});
