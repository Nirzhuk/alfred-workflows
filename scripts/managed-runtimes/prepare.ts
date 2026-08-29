import { chmod, copyFile, lstat, mkdir, readdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { randomUUID } from "node:crypto";

import {
  assertNoLatestOrFallback,
  buildRuntimePackageManifest,
  loadAllRuntimeSourceManifests,
  relativePackagePath,
  resolveRepositoryPath,
  sourceTarget,
  sha256Bytes,
  type ArtifactSource,
  type PackageFile,
  type RuntimeSourceManifest,
  type RuntimeTargetSource,
} from "./manifest";
import { downloadExact, requireRegularFile, verifyFile } from "./download";

const execFileAsync = promisify(execFile);
const DEFAULT_OUTPUT = resolveRepositoryPath("src-tauri/sidecars/managed-runtimes");
const REPOSITORY_ROOT = resolveRepositoryPath("");
const MAX_FILES = 4096;
const MAX_FILE_BYTES = 1024 * 1024 * 1024;

export type PrepareOptions = {
  target: string;
  output?: string;
  cache?: string;
  evidence?: string;
  codexSidecar?: string;
  codexLegalDir?: string;
  codexSbom?: string;
  offline?: boolean;
  release?: boolean;
  fetchImpl?: typeof fetch;
};

export class ManagedRuntimePrepareError extends Error {
  constructor(readonly code: string, message: string) {
    super(`${code}: ${message}`);
    this.name = "ManagedRuntimePrepareError";
  }
}

function fail(code: string, message: string): never {
  throw new ManagedRuntimePrepareError(code, message);
}

function targetVersionPath(output: string, manifest: RuntimeSourceManifest, target: string): string {
  return join(output, manifest.runtimeId, target, manifest.runtimeVersion);
}

async function assertNoSymlink(path: string, label: string): Promise<void> {
  const metadata = await lstat(path).catch(() => undefined);
  if (!metadata || !metadata.isFile()) fail("managed_runtime_input_missing", `${label}: ${path}`);
}

async function copyInput(source: string, destination: string, executable = false): Promise<void> {
  await assertNoSymlink(source, "input");
  const metadata = await lstat(source);
  if (metadata.size > MAX_FILE_BYTES) fail("managed_runtime_input_too_large", source);
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  if (executable) await chmod(destination, 0o755);
}

async function collectPackageFiles(root: string): Promise<PackageFile[]> {
  const files: PackageFile[] = [];
  async function walk(directory: string): Promise<void> {
    const entries = await readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
      if (files.length >= MAX_FILES) fail("managed_runtime_file_count_exceeded", root);
      const path = join(directory, entry.name);
      const metadata = await lstat(path);
      if (metadata.isSymbolicLink()) fail("managed_runtime_symlink_rejected", path);
      if (metadata.isDirectory()) {
        await walk(path);
      } else if (metadata.isFile()) {
        if (metadata.size > MAX_FILE_BYTES) fail("managed_runtime_input_too_large", path);
        const bytes = new Uint8Array(await readFile(path));
        files.push({
          relativePath: relativePackagePath(path, root),
          sha256: sha256Bytes(bytes),
          executable: (metadata.mode & 0o111) !== 0,
        });
      } else {
        fail("managed_runtime_input_invalid", path);
      }
    }
  }
  await walk(root);
  return files.sort((left, right) => left.relativePath.localeCompare(right.relativePath));
}

async function runExtractor(command: string, args: string[], label: string): Promise<void> {
  try {
    await execFileAsync(command, args, { maxBuffer: 1024 * 1024 });
  } catch (error) {
    const detail = error instanceof Error ? error.message.split("\n")[0] : "extractor failed";
    fail("managed_runtime_archive_invalid", `${label}: ${detail}`);
  }
}

async function extractArchive(archive: string, destination: string): Promise<void> {
  await mkdir(destination, { recursive: true });
  if (archive.endsWith(".zip") || archive.endsWith(".whl")) {
    await runExtractor("unzip", ["-q", archive, "-d", destination], basename(archive));
  } else if (archive.endsWith(".tar.gz")) {
    await runExtractor("tar", ["-xzf", archive, "-C", destination], basename(archive));
  } else {
    fail("managed_runtime_archive_invalid", `unsupported archive ${archive}`);
  }
}

async function findNamedFile(root: string, name: string): Promise<string> {
  const found: string[] = [];
  async function walk(directory: string, depth: number): Promise<void> {
    if (depth > 8 || found.length > 1) return;
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) fail("managed_runtime_symlink_rejected", path);
      if (entry.isDirectory()) await walk(path, depth + 1);
      else if (entry.isFile() && entry.name === name) found.push(path);
    }
  }
  await walk(root, 0);
  if (found.length !== 1) fail("managed_runtime_archive_invalid", `${name} not found uniquely`);
  return found[0];
}

function cacheArtifactPath(cache: string, manifest: RuntimeSourceManifest, artifact: ArtifactSource): string {
  // Several target artifacts intentionally share a filename (for example
  // Claude's arm64 and x64 binaries). Include the pinned digest so one
  // target can never overwrite another target's cache entry.
  return join(cache, manifest.runtimeId, `${artifact.sha256}-${artifact.fileName}`);
}

async function acquire(
  manifest: RuntimeSourceManifest,
  artifact: ArtifactSource,
  options: PrepareOptions,
): Promise<string> {
  const cachePath = cacheArtifactPath(options.cache!, manifest, artifact);
  await downloadExact(artifact, cachePath, {
    offline: options.offline,
    fetchImpl: options.fetchImpl,
    maxBytes: MAX_FILE_BYTES,
  });
  return cachePath;
}

async function addLegalResources(
  manifest: RuntimeSourceManifest,
  packageRoot: string,
  options: PrepareOptions,
): Promise<void> {
  const sourceLicense = manifest.legal.licenseSource
    ? resolveRepositoryPath(manifest.legal.licenseSource)
    : undefined;
  const sourceNotice = manifest.legal.noticeSource
    ? resolveRepositoryPath(manifest.legal.noticeSource)
    : undefined;
  const legalDir = options.codexLegalDir ? resolve(options.codexLegalDir) : undefined;
  const license = sourceLicense ?? (legalDir ? join(legalDir, manifest.legal.licenseResourcePath.replace(/^legal\//, "")) : undefined);
  const notice = sourceNotice ?? (legalDir ? join(legalDir, manifest.legal.noticeResourcePath.replace(/^legal\//, "")) : undefined);
  if (!license || !notice) fail("managed_runtime_legal_missing", `${manifest.runtimeId} legal input directory is required`);
  await copyInput(license, join(packageRoot, manifest.legal.licenseResourcePath));
  await copyInput(notice, join(packageRoot, manifest.legal.noticeResourcePath));
  await verifyFile(join(packageRoot, manifest.legal.licenseResourcePath), manifest.legal.licenseSha256);
  await verifyFile(join(packageRoot, manifest.legal.noticeResourcePath), manifest.legal.noticeSha256);
}

async function addCodexLegalInventory(
  manifest: RuntimeSourceManifest,
  packageRoot: string,
  options: PrepareOptions,
): Promise<void> {
  if (!options.codexLegalDir) fail("codex_legal_inventory_missing", "--codex-legal-dir is required");
  if (!manifest.legal.inventory?.length) fail("codex_legal_inventory_missing", "Codex legal inventory is empty");
  const inventoryPath = join(resolve(options.codexLegalDir), "third-party-notices.json");
  await requireRegularFile(inventoryPath, "Codex third-party notice inventory");
  let value: unknown;
  try {
    value = JSON.parse(await readFile(inventoryPath, "utf8"));
  } catch {
    fail("codex_legal_inventory_invalid", inventoryPath);
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) fail("codex_legal_inventory_invalid", inventoryPath);
  const components = (value as Record<string, unknown>).components;
  if (!Array.isArray(components) || components.length !== manifest.legal.inventory.length) fail("codex_legal_inventory_invalid", "inventory component count mismatch");
  const expectedComponents = new Map(manifest.legal.inventory.map((entry) => [entry.name, entry]));
  const seenNames = new Set<string>();
  const seenSlugs = new Set<string>();
  for (const component of components) {
    if (!component || typeof component !== "object" || Array.isArray(component)) fail("codex_legal_inventory_invalid", "component is invalid");
    const entry = component as Record<string, unknown>;
    if (typeof entry.name !== "string" || !expectedComponents.has(entry.name) || typeof entry.expression !== "string" || typeof entry.licensePath !== "string" || typeof entry.noticePath !== "string" || typeof entry.licenseSha256 !== "string" || typeof entry.noticeSha256 !== "string") fail("codex_legal_inventory_invalid", "component must bind name, expression, paths, and digests");
    if (seenNames.has(entry.name)) fail("codex_legal_inventory_invalid", `${entry.name} is duplicated`);
    seenNames.add(entry.name);
    if (entry.expression !== expectedComponents.get(entry.name)?.expression) fail("codex_legal_inventory_invalid", `${entry.name} license expression mismatch`);
    if (!/^[0-9a-f]{64}$/.test(entry.licenseSha256) || !/^[0-9a-f]{64}$/.test(entry.noticeSha256)) fail("codex_legal_inventory_invalid", `${entry.name} legal digest is invalid`);
    const license = resolve(options.codexLegalDir, entry.licensePath);
    const notice = resolve(options.codexLegalDir, entry.noticePath);
    const legalRoot = resolve(options.codexLegalDir);
    const licenseRelative = relative(legalRoot, license);
    const noticeRelative = relative(legalRoot, notice);
    if (licenseRelative.startsWith("..") || noticeRelative.startsWith("..")) fail("codex_legal_inventory_invalid", `${entry.name} legal path escaped input root`);
    const slug = entry.name.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    if (!slug || seenSlugs.has(slug)) fail("codex_legal_inventory_invalid", `${entry.name} legal destination collides`);
    seenSlugs.add(slug);
    await copyInput(license, join(packageRoot, "legal", "third-party", slug, "LICENSE"));
    await copyInput(notice, join(packageRoot, "legal", "third-party", slug, "NOTICE"));
    await verifyFile(join(packageRoot, "legal", "third-party", slug, "LICENSE"), entry.licenseSha256);
    await verifyFile(join(packageRoot, "legal", "third-party", slug, "NOTICE"), entry.noticeSha256);
  }
  await copyInput(inventoryPath, join(packageRoot, "legal", "third-party-notices.json"));
}

async function prepareClaude(
  manifest: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  packageRoot: string,
  options: PrepareOptions,
): Promise<void> {
  if (!target.artifact) fail("managed_runtime_manifest_invalid", "Claude target has no artifact");
  const artifact = await acquire(manifest, target.artifact, options);
  await copyInput(artifact, join(packageRoot, target.package.executable), true);
  await verifyFile(join(packageRoot, target.package.executable), target.artifact.sha256, target.artifact.size);
  await addLegalResources(manifest, packageRoot, options);
}

async function prepareOpenCode(
  manifest: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  packageRoot: string,
  temporaryRoot: string,
  options: PrepareOptions,
): Promise<void> {
  if (!target.artifact?.extractedFileName || !target.artifact.extractedSha256) {
    fail("managed_runtime_manifest_invalid", "OpenCode target is missing extraction pins");
  }
  const archive = await acquire(manifest, target.artifact, options);
  const extracted = join(temporaryRoot, "opencode-extracted");
  await extractArchive(archive, extracted);
  const executable = await findNamedFile(extracted, target.artifact.extractedFileName);
  await copyInput(executable, join(packageRoot, target.package.executable), true);
  await verifyFile(join(packageRoot, target.package.executable), target.artifact.extractedSha256, target.artifact.extractedSize);
  await addLegalResources(manifest, packageRoot, options);
}

async function prepareCodex(
  manifest: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  packageRoot: string,
  temporaryRoot: string,
  options: PrepareOptions,
): Promise<void> {
  const sidecar = options.codexSidecar;
  if (!sidecar) fail("codex_sidecar_executable_missing", "--codex-sidecar is required for Codex release preparation");
  await requireRegularFile(sidecar, "Codex sidecar executable");
  await copyInput(sidecar, join(packageRoot, target.package.executable), true);

  if (!target.python || !target.cliWheel || !target.pydanticCoreWheel) {
    fail("managed_runtime_manifest_invalid", "Codex target is missing Python or wheel pins");
  }
  // CPython and all locked wheels are consumed by the independently built
  // sidecar executable. Keep them as verified cache inputs rather than
  // expanding them into RuntimePackageStore resources: its manifest contract
  // intentionally bounds declared resources to 128 entries.
  if (!manifest.sdkSdist) fail("managed_runtime_manifest_invalid", "Codex source is missing the pinned SDK sdist");
  await acquire(manifest, manifest.sdkSdist, options);
  await acquire(manifest, target.python, options);
  for (const wheel of manifest.wheels ?? []) await acquire(manifest, wheel, options);
  await acquire(manifest, target.pydanticCoreWheel, options);
  const cliWheel = await acquire(manifest, target.cliWheel, options);
  const cliExtracted = join(temporaryRoot, "cli");
  await extractArchive(cliWheel, cliExtracted);
  const windowsTarget = target.target.includes("windows");
  const codexBinary = await findNamedFile(cliExtracted, windowsTarget ? "codex.exe" : "codex");
  await copyInput(codexBinary, join(packageRoot, "libexec", windowsTarget ? "codex.exe" : "codex"), true);
  await addLegalResources(manifest, packageRoot, options);
  await addCodexLegalInventory(manifest, packageRoot, options);
  if (!options.codexSbom) fail("codex_target_sbom_missing", "--codex-sbom is required for a sealed target package");
  await requireRegularFile(options.codexSbom, "Codex target SBOM");
  const sbomBytes = await readFile(options.codexSbom, "utf8");
  if (sbomBytes.includes("source-component-expectation-not-a-sealed-target-sbom")) fail("codex_target_sbom_invalid", "source SBOM cannot be used as final target SBOM");
  let sbom: unknown;
  try {
    sbom = JSON.parse(sbomBytes);
  } catch {
    fail("codex_target_sbom_invalid", "final target SBOM is not JSON");
  }
  if (!sbom || typeof sbom !== "object" || Array.isArray(sbom) || (sbom as Record<string, unknown>).bomFormat !== "CycloneDX" || typeof (sbom as Record<string, unknown>).specVersion !== "string" || !Array.isArray((sbom as Record<string, unknown>).components)) fail("codex_target_sbom_invalid", "final target SBOM is not a CycloneDX component inventory");
  await copyInput(options.codexSbom, join(packageRoot, manifest.packageLayout?.sbom ?? "legal/sbom.cdx.json"));
}

function requiredEvidence(manifest: RuntimeSourceManifest, target: RuntimeTargetSource): string[] {
  const evidence = ["publisher-verification.json"];
  if (manifest.runtimeId === "claude_code_managed") evidence.push("manifest.json", "manifest.json.sig", "platform-signature.json");
  if (manifest.runtimeId === "codex_python_sdk") evidence.push("python.sigstore.json", "cli-wheel.sigstore.json", "pydantic-core.sigstore.json", "sdk-wheel.sigstore.json");
  return evidence;
}

async function stageEvidence(
  manifest: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  stageRoot: string,
  options: PrepareOptions,
): Promise<void> {
  const evidenceDir = join(options.evidence!, manifest.runtimeId, target.target);
  const destination = join(stageRoot, "publisher-evidence");
  await mkdir(destination, { recursive: true });
  for (const name of requiredEvidence(manifest, target)) {
    const source = join(evidenceDir, name);
    await requireRegularFile(source, `${manifest.runtimeId} ${target.target} publisher evidence ${name}`);
    await copyInput(source, join(destination, name));
  }
}

async function writeJson(path: string, value: unknown): Promise<void> {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8" });
}

export async function prepareRuntimeTarget(
  manifest: RuntimeSourceManifest,
  targetName: string,
  options: PrepareOptions,
): Promise<string> {
  assertNoLatestOrFallback(manifest);
  const target = sourceTarget(manifest, targetName);
  const output = resolve(options.output ?? DEFAULT_OUTPUT);
  const cache = resolve(options.cache ?? process.env.ALFRED_RUNTIME_CACHE ?? join(REPOSITORY_ROOT, ".cache", "managed-runtimes"));
  const evidence = resolve(options.evidence ?? process.env.ALFRED_RUNTIME_EVIDENCE_DIR ?? join(cache, "evidence"));
  const effective = { ...options, output, cache, evidence };
  await mkdir(output, { recursive: true });
  const finalPath = targetVersionPath(output, manifest, targetName);
  const parent = dirname(finalPath);
  await mkdir(parent, { recursive: true });
  const stageRoot = join(parent, `.staging-${randomUUID()}`);
  const packageRoot = join(stageRoot, "package");
  const temporaryRoot = join(stageRoot, "work");
  await mkdir(packageRoot, { recursive: true });
  try {
    if (manifest.runtimeId === "claude_code_managed") await prepareClaude(manifest, target, packageRoot, effective);
    else if (manifest.runtimeId === "opencode_server") await prepareOpenCode(manifest, target, packageRoot, temporaryRoot, effective);
    else if (manifest.runtimeId === "codex_python_sdk") await prepareCodex(manifest, target, packageRoot, temporaryRoot, effective);
    else fail("managed_runtime_manifest_invalid", `unsupported runtime ${manifest.runtimeId}`);
    const files = await collectPackageFiles(packageRoot);
    const packageManifest = buildRuntimePackageManifest(manifest, target, files);
    await writeJson(join(stageRoot, "runtime-manifest.json"), packageManifest);
    await writeJson(join(stageRoot, "source-manifest.json"), manifest);
    await writeJson(join(stageRoot, "package-index.json"), {
      schemaVersion: 1,
      runtimeId: manifest.runtimeId,
      runtimeVersion: manifest.runtimeVersion,
      target: target.target,
      packageRoot: "package",
      runtimeManifest: "runtime-manifest.json",
      publisherEvidence: "publisher-evidence",
      files,
    });
    if (effective.release) await stageEvidence(manifest, target, stageRoot, effective);
    await rm(join(stageRoot, "work"), { recursive: true, force: true });
    await atomicReplaceDirectory(stageRoot, finalPath);
    return finalPath;
  } catch (error) {
    await rm(stageRoot, { recursive: true, force: true });
    throw error;
  }
}

async function atomicReplaceDirectory(stageRoot: string, finalPath: string): Promise<void> {
  const parent = dirname(finalPath);
  const recovery = join(parent, `.recovery-${randomUUID()}`);
  let movedExisting = false;
  try {
    try {
      await rename(finalPath, recovery);
      movedExisting = true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    try {
      await rename(stageRoot, finalPath);
    } catch (error) {
      if (movedExisting) await rename(recovery, finalPath).catch(() => undefined);
      throw error;
    }
    if (movedExisting) await rm(recovery, { recursive: true, force: true });
  } catch (error) {
    await rm(stageRoot, { recursive: true, force: true });
    throw new ManagedRuntimePrepareError(
      "managed_runtime_atomic_commit_failed",
      error instanceof Error ? error.message : "rename failed",
    );
  }
}

export function parsePrepareArgs(args: string[]): PrepareOptions {
  let target = "";
  const options: Partial<PrepareOptions> = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const next = args[index + 1];
    if (arg === "--target") target = next ?? fail("managed_runtime_arguments_invalid", "--target requires a value");
    else if (arg === "--output") options.output = next ?? fail("managed_runtime_arguments_invalid", "--output requires a value");
    else if (arg === "--cache") options.cache = next ?? fail("managed_runtime_arguments_invalid", "--cache requires a value");
    else if (arg === "--evidence") options.evidence = next ?? fail("managed_runtime_arguments_invalid", "--evidence requires a value");
    else if (arg === "--codex-sidecar") options.codexSidecar = next ?? fail("managed_runtime_arguments_invalid", "--codex-sidecar requires a value");
    else if (arg === "--codex-legal-dir") options.codexLegalDir = next ?? fail("managed_runtime_arguments_invalid", "--codex-legal-dir requires a value");
    else if (arg === "--codex-sbom") options.codexSbom = next ?? fail("managed_runtime_arguments_invalid", "--codex-sbom requires a value");
    else if (arg === "--offline") options.offline = true;
    else if (arg === "--release") options.release = true;
    else if (arg.startsWith("--")) fail("managed_runtime_arguments_invalid", `unknown option ${arg}`);
    else fail("managed_runtime_arguments_invalid", `unexpected argument ${arg}`);
    if (arg !== "--offline" && arg !== "--release") index += 1;
  }
  if (!target || target === "latest" || /[/\\]/.test(target)) fail("managed_runtime_arguments_invalid", "an exact --target is required");
  return { target, ...options } as PrepareOptions;
}

async function main(): Promise<void> {
  const options = parsePrepareArgs(process.argv.slice(2));
  const manifests = await loadAllRuntimeSourceManifests();
  for (const manifest of manifests) {
    await prepareRuntimeTarget(manifest, options.target, options);
    console.log(`prepared ${manifest.runtimeId} ${manifest.runtimeVersion} ${options.target}`);
  }
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
