import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * Official Alfred builds are published as public GitHub Release assets.
 * This action sends the user to the releases page; it never resolves or
 * downloads an installer itself, and Alfred has no automatic updater.
 */
export const LATEST_RELEASES_URL =
  "https://github.com/Nirzhuk/alfred-workflows/releases/latest";

export const DOWNLOAD_LATEST_RELEASES_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Official builds update manually. Alfred does not install updates for you.",
  "",
  "Your browser will open the GitHub releases page, where you can grab the",
  "latest installer for your platform.",
  "",
  "The macOS disk images are signed and notarized. The Windows installer is an",
  "unsigned beta, so Windows reports an unknown publisher and may show a",
  "SmartScreen warning.",
].join("\n");

export const DOWNLOAD_LATEST_FAILED_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Alfred could not open your browser. Visit this page yourself to reach the",
  "latest downloads:",
  "",
  `    ${LATEST_RELEASES_URL}`,
  "",
  "Alternatively, rebuild from source:",
  "",
  "    git pull",
  "    bun install --frozen-lockfile",
  "    bun run build",
  "",
  "See docs/building-from-source.md for the platform prerequisites.",
].join("\n");

type ExternalOpener = (url: string) => Promise<void>;

export type DownloadLatestDeps = {
  notify?: (message: string) => void;
  open?: ExternalOpener;
};

export async function openLatestDownload({
  notify = (message) => window.alert(message),
  open = openUrl,
}: DownloadLatestDeps = {}): Promise<void> {
  notify(DOWNLOAD_LATEST_RELEASES_MESSAGE);
  try {
    await open(LATEST_RELEASES_URL);
  } catch {
    notify(DOWNLOAD_LATEST_FAILED_MESSAGE);
  }
}
