import { describe, expect, test } from "bun:test";
import {
  loadSandboxTestKeys,
  loadSandboxTestKeysFromFile,
  parseSandboxTestKeys,
  readSandboxTestKeysFromEnv,
  SandboxSecretsError,
  SECRET_ENV_VARS,
} from "./secrets";

const FIXTURE = {
  individual: "individual-private-value",
  teams: "teams-private-value",
};

function envFixture(): Record<string, string | undefined> {
  return {
    [SECRET_ENV_VARS.individual]: FIXTURE.individual,
    [SECRET_ENV_VARS.teams]: FIXTURE.teams,
  };
}

describe("sandbox secret input", () => {
  test("parses both keys without transforming their values", () => {
    const keys = parseSandboxTestKeys(FIXTURE);

    expect(keys.individual).toBe(FIXTURE.individual);
    expect(keys.teams).toBe(FIXTURE.teams);
  });

  test("rejects incomplete input and paths that are not ignored local files", async () => {
    expect(() =>
      parseSandboxTestKeys({ individual: FIXTURE.individual }),
    ).toThrow(SandboxSecretsError);

    await expect(
      loadSandboxTestKeysFromFile("scripts/polar/sandbox-secrets.json"),
    ).rejects.toBeInstanceOf(SandboxSecretsError);
  });

  test("reads both keys from a secret runner's environment", () => {
    expect(readSandboxTestKeysFromEnv(envFixture())).toEqual(FIXTURE);
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

  test("refuses a half-configured secret runner and names the missing variable", () => {
    const partial = envFixture();
    delete partial[SECRET_ENV_VARS.teams];

    let reason = "";
    try {
      readSandboxTestKeysFromEnv(partial);
    } catch (error) {
      reason = (error as SandboxSecretsError).reason;
    }
    expect(reason).toContain(SECRET_ENV_VARS.teams);
    expect(reason).not.toContain("private-value");
  });

  test("never puts a key value into an error message", () => {
    const tooShort = { ...FIXTURE, individual: "short" };

    let message = "";
    try {
      parseSandboxTestKeys(tooShort);
    } catch (error) {
      message = String(error);
    }
    expect(message).toContain("individual");
    expect(message).not.toContain("short");
  });
});
