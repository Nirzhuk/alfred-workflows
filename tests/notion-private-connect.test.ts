import { describe, expect, test } from "bun:test";

describe("Notion private connection form", () => {
  test("keeps setup read-only and clears the token from React state", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/notion-private-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain('type="password"');
    expect(source).toContain('setIntegrationToken("")');
    expect(source).toContain("read-content capability only");
    expect(source).toContain("does not index your workspace");
    expect(source).toContain("Develop your own connections");
    expect(source).toContain("Read content only");
    expect(source).toContain("Share → Add connections");
    expect(source).toContain("Copy installation access token");
    expect(source).toContain("profile/integrations/internal");
    const capability = await Bun.file(
      new URL("../src-tauri/capabilities/default.json", import.meta.url),
    ).text();
    expect(capability).toContain(
      '"https://www.notion.so/profile/integrations/internal"',
    );
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("sessionStorage");
  });
});
