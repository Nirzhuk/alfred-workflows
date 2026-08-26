import { createHash } from "node:crypto";
import { mkdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { randomUUID } from "node:crypto";

import { ManagedRuntimeManifestError, type ArtifactSource, sha256FileBytes } from "./manifest";

const MAX_DOWNLOAD_BYTES = 1024 * 1024 * 1024;

export type DownloadOptions = {
  offline?: boolean;
  fetchImpl?: typeof fetch;
  maxBytes?: number;
};

export class ManagedRuntimeDownloadError extends Error {
  constructor(readonly code: string, message: string) {
    super(`${code}: ${message}`);
    this.name = "ManagedRuntimeDownloadError";
  }
}

function fail(code: string, message: string): never {
  throw new ManagedRuntimeDownloadError(code, message);
}

function rejectUnpinnedUrl(url: string): void {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    fail("managed_runtime_url_invalid", url);
  }
  if (parsed.protocol !== "https:" || /latest|fallback|latest-version/i.test(url)) {
    fail("managed_runtime_url_unpinned", url);
  }
}

async function readBounded(response: Response, maxBytes: number): Promise<Uint8Array> {
  const declared = response.headers.get("content-length");
  if (declared && (!/^\d+$/.test(declared) || Number(declared) > maxBytes)) {
    fail("managed_runtime_download_too_large", "content-length exceeds bound");
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength > maxBytes) fail("managed_runtime_download_too_large", "response exceeds bound");
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    for (;;) {
      const item = await reader.read();
      if (item.done) break;
      total += item.value.byteLength;
      if (total > maxBytes) {
        await reader.cancel();
        fail("managed_runtime_download_too_large", "response exceeds bound");
      }
      chunks.push(item.value);
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

async function writeAtomic(path: string, bytes: Uint8Array): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const temporary = `${path}.tmp-${randomUUID()}`;
  try {
    await writeFile(temporary, bytes, { flag: "wx" });
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true }).catch(() => undefined);
  }
}

/**
 * Fetch exactly one code-owned artifact. Release hosts may redirect an exact
 * URL to immutable storage, but the final URL must remain pinned-looking and
 * the body must still match the code-owned SHA-256 and optional size.
 */
export async function downloadExact(
  artifact: ArtifactSource,
  destination: string,
  options: DownloadOptions = {},
): Promise<Uint8Array> {
  rejectUnpinnedUrl(artifact.url);
  const maxBytes = Math.min(options.maxBytes ?? MAX_DOWNLOAD_BYTES, MAX_DOWNLOAD_BYTES);
  const path = resolve(destination);
  if (options.offline) {
    let bytes: Uint8Array;
    try {
      bytes = new Uint8Array(await readFile(path));
    } catch {
      fail("managed_runtime_offline_input_missing", `${artifact.fileName} is not in the offline cache`);
    }
    if (bytes.byteLength > maxBytes) fail("managed_runtime_download_too_large", `${artifact.fileName} exceeds the offline bound`);
    sha256FileBytes(bytes, artifact.sha256, artifact.fileName);
    if (artifact.size !== undefined && bytes.byteLength !== artifact.size) {
      fail("managed_runtime_size_mismatch", artifact.fileName);
    }
    return bytes;
  }
  const fetchImpl = options.fetchImpl ?? fetch;
  let response: Response;
  try {
    response = await fetchImpl(artifact.url, { redirect: "follow" });
  } catch (error) {
    throw new ManagedRuntimeDownloadError(
      "managed_runtime_download_failed",
      error instanceof Error ? error.message : "fetch failed",
    );
  }
  if (!response.ok) fail("managed_runtime_download_failed", `${artifact.fileName} returned HTTP ${response.status}`);
  if (response.url) rejectUnpinnedUrl(response.url);
  const bytes = await readBounded(response, maxBytes);
  sha256FileBytes(bytes, artifact.sha256, artifact.fileName);
  if (artifact.size !== undefined && bytes.byteLength !== artifact.size) {
    fail("managed_runtime_size_mismatch", artifact.fileName);
  }
  await writeAtomic(path, bytes);
  return bytes;
}

export async function verifyFile(path: string, expectedSha256: string, expectedSize?: number): Promise<void> {
  let bytes: Uint8Array;
  try {
    bytes = new Uint8Array(await readFile(path));
  } catch {
    throw new ManagedRuntimeManifestError("managed_runtime_artifact_missing", path);
  }
  sha256FileBytes(bytes, expectedSha256, path);
  if (expectedSize !== undefined && bytes.byteLength !== expectedSize) {
    throw new ManagedRuntimeManifestError("managed_runtime_size_mismatch", path);
  }
}

export async function requireRegularFile(path: string, label: string): Promise<void> {
  try {
    const metadata = await stat(path);
    if (!metadata.isFile()) throw new Error("not a regular file");
  } catch {
    throw new ManagedRuntimeManifestError("managed_runtime_input_missing", `${label}: ${path}`);
  }
}

export function digestBytes(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}
