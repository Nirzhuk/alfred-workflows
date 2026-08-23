import { describe, expect, test } from "bun:test";
import { SandboxSecretsError, SECRET_ENV_VARS } from "./secrets";
import { runSandboxVerifier } from "./verify-sandbox";

function collect() {
  const lines: string[] = [];
  return {
    lines,
    write: (text: string) => lines.push(text),
    report: (passed: boolean, caseName: string) =>
      lines.push(`${passed ? "PASS" : "FAIL"} ${caseName}`),
  };
}

describe("verify:polar-sandbox entry point", () => {
  test("refuses command-line arguments so a key cannot reach shell history", async () => {
    const sink = collect();

    const passed = await runSandboxVerifier({
      argv: ["polar_lk_pretend_key"],
      write: sink.write,
      report: sink.report,
    });

    expect(passed).toBe(false);
    expect(sink.lines[0]).toBe(
      "FAIL verifier-input.arguments (this command takes no arguments)",
    );
    expect(sink.lines.join("\n")).not.toContain("polar_lk_pretend_key");
  });

  test("stops at the secrets gate without reaching the network", async () => {
    // Both benefit IDs are now bound in the committed manifest, so manifest
    // validation passes and the next gate is the missing TEST license keys.
    // The verifier must stop there and never reach Polar without secrets.
    // The "an unbound benefit ID fails closed" invariant is owned by
    // manifest.test.ts ("requires both benefit IDs, with no optional class
    // left"), which covers both classes against a fixture rather than against
    // the committed manifest, whose values legitimately change as the operator
    // binds them.
    const sink = collect();

    const passed = await runSandboxVerifier({
      argv: [],
      write: sink.write,
      report: sink.report,
      // Inject the absent-secrets case. Reading the default path would pick up
      // an operator's real sandbox-secrets.json.local and drive live
      // activations against Polar from a unit test.
      loadKeys: () => {
        throw new SandboxSecretsError(
          "sandbox-secrets.json.local is missing or is not valid JSON",
        );
      },
    });

    expect(passed).toBe(false);
    const rendered = sink.lines.join("\n");
    expect(rendered).toContain("FAIL verifier-input.secrets");
    // The manifest gate is behind us; only secrets are missing.
    expect(rendered).not.toContain("verifier-input.manifest");
    // It must not report success or reach the network.
    expect(rendered).not.toContain("PASS ");
  });

  test("names both supported secret sources in its remediation help", async () => {
    const sink = collect();

    await runSandboxVerifier({
      argv: ["--keys"],
      write: sink.write,
      report: sink.report,
    });

    const rendered = sink.lines.join("\n");
    for (const name of Object.values(SECRET_ENV_VARS)) {
      expect(rendered).toContain(name);
    }
    expect(rendered).toContain("sandbox-secrets.json.local");
  });
});
