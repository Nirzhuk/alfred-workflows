import {
  PolarPublicLinkError,
  polarPublicLinks,
  type PolarPublicLinks,
} from "./public-links";

/**
 * Alfred has no automatic updater. This action only sends the customer to
 * Polar's hosted customer portal, where they authenticate by email and Polar
 * issues their personal download links. Alfred never resolves or fetches a
 * signed installer URL itself.
 */
export const DOWNLOAD_LATEST_PORTAL_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Official builds update manually. Alfred does not install updates for you.",
  "",
  "Your browser will open Polar's customer portal. Sign in with the email you",
  "purchased with, and Polar shows the downloads your license covers.",
  "",
  "The macOS disk images are signed and notarized. The Windows installer is an",
  "unsigned beta, so Windows reports an unknown publisher and may show a",
  "SmartScreen warning.",
].join("\n");

export const DOWNLOAD_LATEST_SOURCE_MESSAGE = [
  "Download the latest Alfred",
  "",
  "This build has no official download channel configured, so it is a source or",
  "self-built copy. That build stays fully usable under GPL-3.0-or-later.",
  "",
  "To move it to the latest version, rebuild from source:",
  "",
  "    git pull",
  "    bun install --frozen-lockfile",
  "    bun run build",
  "",
  "See docs/building-from-source.md for the platform prerequisites.",
].join("\n");

export const DOWNLOAD_LATEST_FAILED_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Alfred could not open your browser. Open Polar's customer portal yourself and",
  "sign in with the email you purchased with to reach your downloads.",
].join("\n");

export type DownloadLatestDeps = {
  links?: PolarPublicLinks;
  notify?: (message: string) => void;
};

export async function openLatestDownload({
  links = polarPublicLinks,
  notify = (message) => window.alert(message),
}: DownloadLatestDeps = {}): Promise<void> {
  if (!links.isConfigured("customerPortal")) {
    notify(DOWNLOAD_LATEST_SOURCE_MESSAGE);
    return;
  }

  notify(DOWNLOAD_LATEST_PORTAL_MESSAGE);
  try {
    await links.open("customerPortal");
  } catch (error) {
    notify(
      error instanceof PolarPublicLinkError
        ? DOWNLOAD_LATEST_SOURCE_MESSAGE
        : DOWNLOAD_LATEST_FAILED_MESSAGE,
    );
  }
}
