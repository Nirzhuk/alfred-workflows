import { describe, expect, test } from "bun:test";
import {
  DOWNLOAD_LATEST_FAILED_MESSAGE,
  DOWNLOAD_LATEST_PORTAL_MESSAGE,
  DOWNLOAD_LATEST_SOURCE_MESSAGE,
  openLatestDownload,
} from "./download-latest";
import {
  createPolarPublicLinks,
  type PolarPublicLinkConfig,
} from "./public-links";

const configured: PolarPublicLinkConfig = {
  desktopCheckout: "https://buy.polar.sh/polar_cl_desktopFixture",
  customerPortal: "https://polar.sh/alfred/portal",
};

const unconfigured: PolarPublicLinkConfig = {
  desktopCheckout: undefined,
  customerPortal: undefined,
};

function collect(config: PolarPublicLinkConfig, opener?: (url: string) => Promise<void>) {
  const opened: string[] = [];
  const notified: string[] = [];
  const links = createPolarPublicLinks(
    config,
    opener ??
      (async (url) => {
        opened.push(url);
      }),
  );
  return {
    opened,
    notified,
    run: () =>
      openLatestDownload({
        links,
        notify: (message) => notified.push(message),
      }),
  };
}

describe("download latest version action", () => {
  test("a configured build opens only the Polar customer portal", async () => {
    const ctx = collect(configured);

    await ctx.run();

    expect(ctx.opened).toEqual(["https://polar.sh/alfred/portal"]);
    expect(ctx.notified).toEqual([DOWNLOAD_LATEST_PORTAL_MESSAGE]);
  });

  test("the portal message promises manual downloads, not automatic updates", () => {
    expect(DOWNLOAD_LATEST_PORTAL_MESSAGE).toContain("update manually");
    expect(DOWNLOAD_LATEST_PORTAL_MESSAGE).toContain(
      "does not install updates for you",
    );
    expect(DOWNLOAD_LATEST_PORTAL_MESSAGE).toContain("Sign in with the email");
    expect(DOWNLOAD_LATEST_PORTAL_MESSAGE).toContain("unsigned beta");
    expect(DOWNLOAD_LATEST_PORTAL_MESSAGE).not.toContain("automatic");
  });

  test("an unconfigured build shows build instructions and opens nothing", async () => {
    const ctx = collect(unconfigured);

    await ctx.run();

    expect(ctx.opened).toEqual([]);
    expect(ctx.notified).toEqual([DOWNLOAD_LATEST_SOURCE_MESSAGE]);
    expect(DOWNLOAD_LATEST_SOURCE_MESSAGE).toContain("bun run build");
    expect(DOWNLOAD_LATEST_SOURCE_MESSAGE).toContain("GPL-3.0-or-later");
  });

  test("a rejected portal URL falls back to build instructions", async () => {
    const ctx = collect({
      ...unconfigured,
      customerPortal: "https://polar.sh/alfred/portal?redirect=https://example.com",
    });

    await ctx.run();

    expect(ctx.opened).toEqual([]);
    expect(ctx.notified).toEqual([DOWNLOAD_LATEST_SOURCE_MESSAGE]);
  });

  test("a browser failure explains the manual portal path", async () => {
    const ctx = collect(configured, async () => {
      throw new Error("no system browser");
    });

    await ctx.run();

    expect(ctx.notified).toEqual([
      DOWNLOAD_LATEST_PORTAL_MESSAGE,
      DOWNLOAD_LATEST_FAILED_MESSAGE,
    ]);
  });
});
