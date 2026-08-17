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
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("sessionStorage");
  });
});
