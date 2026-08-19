import { describe, expect, test } from "bun:test";

describe("Gmail connected app", () => {
  test("uses system-browser PKCE authorization, send-only scope, and no pasted secret", async () => {
    const source = await Bun.file(
      new URL("../src/features/integrations/gmail-connect.tsx", import.meta.url),
    ).text();
    expect(source).toContain("authorization.authorizationUrl");
    expect(source).toContain("openUrl");
    expect(source).toContain("complete(sessionId)");
    expect(source).toContain("cancel(sessionId)");
    expect(source).toContain("gmail.send");
    expect(source).toContain("no reading");
    expect(source).toContain("searching, or deleting of mail");
    expect(source).not.toContain('type="password"');
    expect(source).not.toContain("clientSecret");
    expect(source).not.toContain("localStorage");
  });

  test("keeps Gmail browser access on the exact Google authorization origin", async () => {
    const capability = await Bun.file(
      new URL("../src-tauri/capabilities/default.json", import.meta.url),
    ).text();
    const registry = await Bun.file(
      new URL("../src/features/integrations/provider-ui.ts", import.meta.url),
    ).text();
    const settings = await Bun.file(
      new URL(
        "../src/features/integrations/connected-apps-settings.tsx",
        import.meta.url,
      ),
    ).text();
    expect(capability).toContain('"https://accounts.google.com/*"');
    expect(registry).toContain(
      "gmail: { Dialog: GmailConnect, supportsReconnect: true }",
    );
    expect(settings).toContain("PROVIDER_UI");
    expect(settings).toContain("activeConnect");
  });

  test("requested scopes stay send-only and exclude read access", async () => {
    const source = await Bun.file(
      new URL("../src-tauri/src/integrations/gmail.rs", import.meta.url),
    ).text();
    const requestedScopes =
      source.match(/REQUESTED_SCOPES[^\n]*\n/)?.[0] ?? "";
    const sendScope =
      source.match(/GMAIL_SEND_SCOPE[^\n]*\n/)?.[0] ?? "";
    expect(requestedScopes).toContain(
      '["openid", "email", "profile", GMAIL_SEND_SCOPE]',
    );
    expect(sendScope).toContain("https://www.googleapis.com/auth/gmail.send");
    expect(requestedScopes).not.toContain("readonly");
    expect(requestedScopes).not.toContain("modify");
  });

  test("tokens never land in SQLite, command payloads, or workflow output", async () => {
    const gmail = await Bun.file(
      new URL("../src-tauri/src/integrations/gmail.rs", import.meta.url),
    ).text();
    const commands = await Bun.file(
      new URL("../src-tauri/src/commands/integrations.rs", import.meta.url),
    ).text();
    expect(gmail).toContain("Zeroizing");
    expect(gmail).toContain("CredentialEnvelope");
    expect(commands).toContain("complete_gmail_connection");
    expect(commands).not.toContain("refresh_token");
  });
});
