import { describe, expect, test } from "bun:test";
import {
  DOWNLOAD_LATEST_FAILED_MESSAGE,
  DOWNLOAD_LATEST_RELEASES_MESSAGE,
  LATEST_RELEASES_URL,
  openLatestDownload,
} from "./download-latest";

function collect(opener?: (url: string) => Promise<void>) {
  const opened: string[] = [];
  const notified: string[] = [];
  const run = () =>
    openLatestDownload({
      notify: (message) => notified.push(message),
      open: opener ??
        (async (url) => {
          opened.push(url);
        }),
    });
  return { opened, notified, run };
}

describe("download latest version action", () => {
  test("opens only the public GitHub releases page", async () => {
    const ctx = collect();
    await ctx.run();

    expect(ctx.opened).toEqual([LATEST_RELEASES_URL]);
    expect(ctx.notified).toEqual([DOWNLOAD_LATEST_RELEASES_MESSAGE]);
  });

  test("the releases message promises manual downloads, not automatic updates", () => {
    expect(DOWNLOAD_LATEST_RELEASES_MESSAGE).toContain(
      "Official builds update manually. Alfred does not install updates for you.",
    );
    expect(DOWNLOAD_LATEST_RELEASES_MESSAGE).toContain("unsigned beta");
  });

  test("the releases URL is a GitHub releases page allowed by the opener capability", () => {
    expect(LATEST_RELEASES_URL).toMatch(
      /^https:\/\/github\.com\/Nirzhuk\/alfred-workflows\/releases/,
    );
  });
  test("a browser failure explains the manual path and the source rebuild", async () => {
    const ctx = collect(async (url) => {
      ctx.opened.push(url);
      throw new Error("no browser");
    });
    await ctx.run();

    expect(ctx.notified).toEqual([
      DOWNLOAD_LATEST_RELEASES_MESSAGE,
      DOWNLOAD_LATEST_FAILED_MESSAGE,
    ]);
    expect(DOWNLOAD_LATEST_FAILED_MESSAGE).toContain(LATEST_RELEASES_URL);
    expect(DOWNLOAD_LATEST_FAILED_MESSAGE).toContain("bun run build");
  });
});
