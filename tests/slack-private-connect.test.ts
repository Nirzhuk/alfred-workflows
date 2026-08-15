import { describe, expect, test } from "bun:test";

describe("Slack private connection form", () => {
  test("uses a password field and explicitly clears secrets without browser persistence", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/slack-private-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain('type="password"');
    expect(source).toContain('setBotToken("")');
    expect(source).toContain('setAppToken("")');
    expect(source).toContain("connections:write");
    expect(source).toContain("app_mentions:read");
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("sessionStorage");
  });
});
