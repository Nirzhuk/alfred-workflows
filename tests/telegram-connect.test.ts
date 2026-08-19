import { describe, expect, test } from "bun:test";

describe("Telegram personal notifications", () => {
  test("setup is dedicated-bot, password, deep-link, and mandatory-test based", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/telegram-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain('type="password"');
    expect(source).toContain("@BotFather");
    expect(source).toContain("fresh bot dedicated to Alfred");
    expect(source).toContain("webhook");
    expect(source).toContain("openTelegramLink(pairing.pairingUrl)");
    expect(source).toContain("https://t.me/BotFather");
    expect(source).toContain("setBotToken(\"\")");
    expect(source).toContain("cancel(sessionId)");
    expect(source).toContain("telegramOpened");
    expect(source).toContain("Finish pairing and send test");
    expect(source).toContain("maxLength={4096}");
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("sessionStorage");
    expect(source).not.toContain("Chat ID");
  });

  test("provider connection dispatch uses a registry and only allows Telegram deep links", async () => {
    const registry = await Bun.file(
      new URL("../src/features/integrations/provider-ui.ts", import.meta.url),
    ).text();
    const settings = await Bun.file(
      new URL(
        "../src/features/integrations/connected-apps-settings.tsx",
        import.meta.url,
      ),
    ).text();
    const capability = await Bun.file(
      new URL("../src-tauri/capabilities/default.json", import.meta.url),
    ).text();
    expect(registry).toContain("TelegramConnect");
    expect(registry).toContain(
      "telegram: { Dialog: TelegramConnect, supportsReconnect: false }",
    );
    expect(settings).toContain("PROVIDER_UI");
    expect(settings).toContain("activeConnect");
    expect(capability).toContain('"https://t.me/*"');
  });
});
