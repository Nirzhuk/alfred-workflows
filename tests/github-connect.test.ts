import { describe, expect, test } from "bun:test";

describe("GitHub connected app", () => {
  test("uses device flow, repository installation boundaries, and no pasted secret", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/github-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain("GitHub App device authorization");
    expect(source).toContain("repositories selected");
    expect(source).toContain("pairing.userCode");
    expect(source).toContain("pairing.verificationUri");
    expect(source).toContain("cancel(sessionId)");
    expect(source).toContain("No repository contents, administration, or code-push access");
    expect(source).not.toContain('type="password"');
    expect(source).not.toContain("clientSecret");
    expect(source).not.toContain("localStorage");
  });

  test("keeps GitHub browser access on the exact provider origin", async () => {
    const capability = await Bun.file(
      new URL("../src-tauri/capabilities/default.json", import.meta.url),
    ).text();
    const settings = await Bun.file(
      new URL(
        "../src/features/integrations/connected-apps-settings.tsx",
        import.meta.url,
      ),
    ).text();
    expect(capability).toContain('"https://github.com/*"');
    expect(settings).toContain("github: () => setGithubConnectOpen(true)");
    expect(settings).toContain("<GitHubConnect");
  });

  test("preserves the legacy gh-backed Git Host node", async () => {
    const runner = await Bun.file(
      new URL("../src-tauri/src/runner/mod.rs", import.meta.url),
    ).text();
    const types = await Bun.file(
      new URL("../src/features/workflow/types.ts", import.meta.url),
    ).text();
    expect(runner).toContain('"gitHost" =>');
    expect(types).toContain('kind: "gitHost"');
  });
});
