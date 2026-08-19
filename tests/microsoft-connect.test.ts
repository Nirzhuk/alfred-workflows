import { describe, expect, test } from "bun:test";

describe("Microsoft connected app", () => {
  test("uses system-browser PKCE authorization, incremental scopes, and no pasted secret", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/microsoft-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain("authorization.authorizationUrl");
    expect(source).toContain("openUrl");
    expect(source).toContain("complete(sessionId)");
    expect(source).toContain("cancel(sessionId)");
    expect(source).toContain("Mail.Send");
    expect(source).toContain("Mail.ReadBasic");
    expect(source).toContain("Calendars.ReadWrite");
    expect(source).toContain("no reading of full");
    expect(source).toContain("while Alfred is open");
    expect(source).not.toContain('type="password"');
    expect(source).not.toContain("clientSecret");
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("Mail.ReadWrite");
  });

  test("keeps Microsoft browser access on the exact authorization origin", async () => {
    const capability = await Bun.file(
      new URL("../src-tauri/capabilities/default.json", import.meta.url),
    ).text();
    const settings = await Bun.file(
      new URL(
        "../src/features/integrations/connected-apps-settings.tsx",
        import.meta.url,
      ),
    ).text();
    expect(capability).toContain('"https://login.microsoftonline.com/*"');
    expect(settings).toContain("microsoft: () => {");
    expect(settings).toContain("<MicrosoftConnect");
  });

  test("requested mail scope stays Mail.ReadBasic and never requests Mail.Read", async () => {
    const source = await Bun.file(
      new URL("../src-tauri/src/integrations/microsoft.rs", import.meta.url),
    ).text();
    expect(source).toContain('pub const MAIL_READ_SCOPE: &str = "Mail.ReadBasic"');
    expect(source).toContain('pub const MAIL_SEND_SCOPE: &str = "Mail.Send"');
    expect(source).toContain(
      'pub const CALENDAR_SCOPE: &str = "Calendars.ReadWrite"',
    );
    expect(source).toContain("include_nonce: true");
    expect(source).toContain('MAIL_READ_SCOPE: &str = "Mail.ReadBasic"');
    expect(source).not.toContain('MAIL_READ_SCOPE: &str = "Mail.Read"');
  });

  test("tokens never land in SQLite, command payloads, or workflow output", async () => {
    const microsoft = await Bun.file(
      new URL("../src-tauri/src/integrations/microsoft.rs", import.meta.url),
    ).text();
    const commands = await Bun.file(
      new URL("../src-tauri/src/commands/integrations.rs", import.meta.url),
    ).text();
    expect(microsoft).toContain("Zeroizing");
    expect(microsoft).toContain("CredentialEnvelope");
    expect(commands).toContain("complete_microsoft_connection");
    expect(commands).not.toContain("refresh_token");
  });
});
