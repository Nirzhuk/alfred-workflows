import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  AppLogo,
  APP_LOGOS,
} from "../src/features/integrations/app-logo";

const root = new URL("../", import.meta.url);
const appLogoSource = await Bun.file(
  new URL("src/features/integrations/app-logo.tsx", root),
).text();

async function catalogProviderIds(): Promise<string[]> {
  const catalog = await Bun.file(
    new URL("src-tauri/src/integrations/catalog.rs", root),
  ).text();
  return Array.from(catalog.matchAll(/provider\(\s*"([^"]+)"/g), (match) => match[1]!);
}

describe("AppLogo", () => {
  test("renders a local optimized logo for every catalog provider", async () => {
    const providerIds = await catalogProviderIds();

    expect(Object.keys(APP_LOGOS).sort()).toEqual(providerIds.sort());

    for (const providerId of providerIds) {
      const markup = renderToStaticMarkup(
        <AppLogo providerId={providerId} providerName={providerId} />,
      );
      expect(markup).toContain(`app-logo-${providerId}`);
      expect(markup).toContain("<img");
    }
  });

  test("falls back to an accessible initial for unknown providers", () => {
    const markup = renderToStaticMarkup(
      <AppLogo providerId="future_app" providerName="Future App" />,
    );

    expect(markup).toContain('aria-label="Future App logo"');
    expect(markup).toContain("is-fallback");
    expect(markup).toContain(">F<");
  });

  test("keeps provider artwork offline-safe and within the connection-logo budget", async () => {
    expect(appLogoSource).toContain("?no-inline");
    expect(appLogoSource).not.toMatch(/https?:\/\//);

    const assets = new Bun.Glob("*.svg");
    const assetDirectory = new URL("src/assets/apps/", root).pathname;
    for await (const assetName of assets.scan({ cwd: assetDirectory })) {
      const asset = Bun.file(`${assetDirectory}/${assetName}`);
      expect(asset.size).toBeLessThanOrEqual(5 * 1024);
      const svg = await asset.text();
      expect(svg).toStartWith("<svg");
      expect(svg).not.toMatch(
        /<(?:image|script)\b|(?:href|xlink:href)\s*=\s*["']https?:\/\//i,
      );
    }
  });

  test("uses a white surface only for marks that cannot be safely recolored", () => {
    expect(APP_LOGOS.github?.requiresSurface).toBe(true);
    expect(APP_LOGOS.linear?.requiresSurface).toBe(true);
    expect(APP_LOGOS.notion?.requiresSurface).toBe(true);
    expect(APP_LOGOS.sentry?.requiresSurface).toBeUndefined();
  });
});
