import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { downloadExact } from "./download.ts";
import { loadAllRuntimeSourceManifests, sha256Bytes, type RuntimeSourceManifest } from "./manifest.ts";
import { prepareRuntimeTarget } from "./prepare.ts";

const TARGET = "aarch64-apple-darwin";
const MANIFEST_URL = "https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json";
const SIGNATURE_URL = "https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json.sig";
const MAX_EVIDENCE_BYTES = 1024 * 1024;

type PublisherReleaseManifest = {
  version?: string;
  commit?: string;
  buildDate?: string;
  platforms?: Record<string, { binary?: string; checksum?: string; size?: number }>;
};

const manifests = await loadAllRuntimeSourceManifests();
const claude = manifests.find((manifest) => manifest.runtimeId === "claude_code_managed");
if (!claude) {
  throw new Error("claude_code_managed source manifest is missing");
}

const prepared = await prepareRuntimeTarget(claude, TARGET, { target: TARGET });
const evidenceRoot = join(prepared, "publisher-evidence");
await mkdir(evidenceRoot, { recursive: true });

const releaseManifest = await downloadExact(
  {
    fileName: "manifest.json",
    url: MANIFEST_URL,
    sha256: await pinAfterValidatingClaudeManifest(MANIFEST_URL, claude),
  },
  join(evidenceRoot, "manifest.json"),
  { maxBytes: MAX_EVIDENCE_BYTES },
);
const signature = await fetchHttps(SIGNATURE_URL, MAX_EVIDENCE_BYTES);
if (signature.byteLength === 0) {
  throw new Error("Claude release signature was empty");
}
await writeFile(join(evidenceRoot, "manifest.json.sig"), signature);

console.log({
  prepared,
  evidenceRoot,
  manifestSha256: sha256Bytes(releaseManifest),
  signatureBytes: signature.byteLength,
});

async function pinAfterValidatingClaudeManifest(
  url: string,
  source: RuntimeSourceManifest,
): Promise<string> {
  const bytes = await fetchHttps(url, MAX_EVIDENCE_BYTES);
  const parsed = JSON.parse(new TextDecoder().decode(bytes)) as PublisherReleaseManifest;
  const sourceMeta = source.source;
  const releaseCommit = typeof sourceMeta.releaseCommit === "string" ? sourceMeta.releaseCommit : "";
  const buildDate = typeof sourceMeta.buildDate === "string" ? sourceMeta.buildDate : "";
  if (
    parsed.version !== source.runtimeVersion
    || parsed.commit !== releaseCommit
    || parsed.buildDate !== buildDate
  ) {
    throw new Error("Claude release manifest did not match the pinned source");
  }
  const expectedPlatforms = new Map(
    source.targets.map((target) => {
      const artifact = target.artifact;
      if (!artifact) {
        throw new Error(`Claude target ${target.target} is missing an artifact pin`);
      }
      const platform = artifact.url.split("/").at(-2);
      if (!platform) {
        throw new Error(`Claude artifact URL is missing a platform segment: ${artifact.url}`);
      }
      return [platform, artifact];
    }),
  );
  if (!parsed.platforms || Object.keys(parsed.platforms).length !== expectedPlatforms.size) {
    throw new Error("Claude release manifest platform set did not match the pinned source");
  }
  for (const [platform, artifact] of expectedPlatforms) {
    const actual = parsed.platforms[platform];
    if (
      actual?.binary !== artifact.fileName
      || actual.checksum !== artifact.sha256
      || actual.size !== artifact.size
    ) {
      throw new Error(`Claude release manifest mismatched ${platform}`);
    }
  }
  return sha256Bytes(bytes);
}

async function fetchHttps(url: string, maxBytes: number): Promise<Uint8Array> {
  const parsed = new URL(url);
  if (parsed.protocol !== "https:" || /latest|fallback|latest-version/i.test(url)) {
    throw new Error(`refusing unpinned evidence URL: ${url}`);
  }
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  if (response.url) {
    const finalUrl = new URL(response.url);
    if (finalUrl.protocol !== "https:" || /latest|fallback|latest-version/i.test(response.url)) {
      throw new Error(`refusing unpinned redirected evidence URL: ${response.url}`);
    }
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength === 0 || bytes.byteLength > maxBytes) {
    throw new Error(`${url} exceeded the evidence size bound`);
  }
  return bytes;
}
