import { createHash } from "node:crypto";
import { lstat, readFile, readdir } from "node:fs/promises";
import { execFile } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import {
  assertNoLatestOrFallback,
  loadAllRuntimeSourceManifests,
  parseRuntimeSourceManifest,
  sourceTarget,
  type RuntimeSourceManifest,
  type RuntimeTargetSource,
} from "./manifest";
import { ManagedRuntimeManifestError } from "./manifest";
import { requireRegularFile, verifyFile } from "./download";
import { publisherHookPlan } from "./publisher-hooks";

const DEFAULT_ROOT = resolve(
  fileURLToPath(new URL("../../src-tauri/sidecars/managed-runtimes", import.meta.url)),
);
const execFileAsync = promisify(execFile);
const MAX_FILES = 4096;
const HASH = /^[0-9a-f]{64}$/;

export type PublisherEvidenceContext = {
  manifest: RuntimeSourceManifest;
  target: RuntimeTargetSource;
  evidenceRoot: string;
  packageRoot: string;
};

export type VerifyOptions = {
  root?: string;
  target: string;
  offline?: boolean;
  publisherHook?: (context: PublisherEvidenceContext) => Promise<void> | void;
};

export class ManagedRuntimeVerificationError extends Error {
  constructor(readonly code: string, message: string) {
    super(`${code}: ${message}`);
    this.name = "ManagedRuntimeVerificationError";
  }
}

function fail(code: string, message: string): never {
  throw new ManagedRuntimeVerificationError(code, message);
}

async function readJson(path: string, code: string): Promise<unknown> {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    fail(code, path);
  }
}

async function walkFiles(root: string): Promise<string[]> {
  const files: string[] = [];
  async function walk(directory: string): Promise<void> {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      const metadata = await lstat(path);
      if (metadata.isSymbolicLink()) fail("managed_runtime_symlink_rejected", path);
      if (metadata.isDirectory()) await walk(path);
      else if (metadata.isFile()) {
        files.push(path);
        if (files.length > MAX_FILES) fail("managed_runtime_file_count_exceeded", root);
      } else fail("managed_runtime_input_invalid", path);
    }
  }
  await walk(root);
  return files.sort();
}

async function verifyPackageFiles(
  packageRoot: string,
  packageIndex: Record<string, unknown>,
  source: RuntimeSourceManifest,
  target: RuntimeTargetSource,
): Promise<void> {
  const files = packageIndex.files;
  if (!Array.isArray(files) || files.length === 0 || files.length > MAX_FILES) {
    fail("managed_runtime_index_invalid", "package index files are missing or unbounded");
  }
  const expected = new Set<string>();
  for (const entry of files) {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) fail("managed_runtime_index_invalid", "package file entry is invalid");
    const value = entry as Record<string, unknown>;
    const relativePath = value.relativePath;
    const digest = value.sha256;
    if (typeof relativePath !== "string" || relativePath.startsWith("/") || relativePath.includes("\\") || relativePath.includes(":") || relativePath.includes("..") || relativePath.split("/").some((part) => part === "" || part === ".") || typeof digest !== "string" || !HASH.test(digest)) {
      fail("managed_runtime_index_invalid", "package file entry is unsafe");
    }
    if (expected.has(relativePath)) fail("managed_runtime_index_invalid", `duplicate ${relativePath}`);
    expected.add(relativePath);
    const path = join(packageRoot, relativePath);
    await verifyFile(path, digest);
    if (value.executable === true && process.platform !== "win32") {
      const metadata = await lstat(path);
      if ((metadata.mode & 0o111) === 0) fail("managed_runtime_executable_invalid", relativePath);
    }
  }
  const actual = new Set((await walkFiles(packageRoot)).map((path) => path.slice(packageRoot.length + 1).replaceAll("\\", "/")));
  if (actual.size !== expected.size || [...actual].some((path) => !expected.has(path))) {
    fail("managed_runtime_undeclared_file", "package contains a file absent from package-index.json");
  }
  if (!expected.has(target.package.executable)) fail("managed_runtime_executable_missing", target.package.executable);
  if (!expected.has(source.legal.licenseResourcePath) || !expected.has(source.legal.noticeResourcePath)) fail("managed_runtime_legal_missing", "license and notice are required");
}

async function verifyEvidenceShape(path: string, expectedScheme: string, expectedPublisher: string): Promise<void> {
  await requireRegularFile(path, "publisher evidence");
  const bytes = new Uint8Array(await readFile(path));
  if (bytes.byteLength === 0 || bytes.byteLength > 1024 * 1024) fail("managed_runtime_publisher_evidence_invalid", path);
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    fail("managed_runtime_publisher_evidence_invalid", `${path} must be JSON evidence`);
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail("managed_runtime_publisher_evidence_invalid", path);
  const record = value as Record<string, unknown>;
  if (record.scheme !== expectedScheme || record.publisher !== expectedPublisher || typeof record.evidenceType !== "string" || typeof record.evidenceDigest !== "string" || !HASH.test(record.evidenceDigest)) {
    fail("managed_runtime_publisher_evidence_invalid", `${path} does not bind scheme/publisher/evidence digest`);
  }
  // A boolean is descriptive at most; accepting {verified:true} here would
  // let downloaded JSON mint the native package capability.
  if ("verified" in record) fail("managed_runtime_publisher_evidence_invalid", `${path} contains an untrusted verified flag`);
}

async function verifyPublisherEvidence(
  manifest: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  targetRoot: string,
  options: VerifyOptions,
): Promise<void> {
  const evidenceRoot = join(targetRoot, "publisher-evidence");
  const common = join(evidenceRoot, "publisher-verification.json");
  await verifyEvidenceShape(common, target.publisherVerification.scheme, target.publisherVerification.publisher);
  if (manifest.runtimeId === "claude_code_managed") {
    for (const name of ["manifest.json", "manifest.json.sig", "platform-signature.json"]) {
      await requireRegularFile(join(evidenceRoot, name), `Claude publisher evidence ${name}`);
    }
  }
  if (manifest.runtimeId === "codex_python_sdk") {
    for (const name of ["python.sigstore.json", "cli-wheel.sigstore.json", "pydantic-core.sigstore.json", "sdk-wheel.sigstore.json"]) {
      await requireRegularFile(join(evidenceRoot, name), `Codex publisher evidence ${name}`);
    }
  }
  if (!options.offline) fail("managed_runtime_online_verification_required", "publisher hooks must be explicitly run in offline mode or release mode");
  const plan = publisherHookPlan(target.publisherVerification.scheme, target.publisherVerification.hook);
  if (!options.publisherHook) fail("managed_runtime_publisher_hook_unavailable", target.publisherVerification.hook);
  await options.publisherHook({ manifest, target, evidenceRoot, packageRoot: join(targetRoot, "package") });
  if (!plan.tool || !plan.description) fail("managed_runtime_publisher_hook_unavailable", target.publisherVerification.hook);
}

export async function verifyPreparedRuntimeTarget(
  manifest: RuntimeSourceManifest,
  targetName: string,
  options: VerifyOptions,
): Promise<void> {
  assertNoLatestOrFallback(manifest);
  const target = sourceTarget(manifest, targetName);
  const root = resolve(options.root ?? DEFAULT_ROOT);
  const targetRoot = join(root, manifest.runtimeId, target.target, manifest.runtimeVersion);
  const packageIndexValue = await readJson(join(targetRoot, "package-index.json"), "managed_runtime_index_missing");
  if (typeof packageIndexValue !== "object" || packageIndexValue === null || Array.isArray(packageIndexValue)) fail("managed_runtime_index_invalid", "package index must be an object");
  const packageIndex = packageIndexValue as Record<string, unknown>;
  if (packageIndex.runtimeId !== manifest.runtimeId || packageIndex.runtimeVersion !== manifest.runtimeVersion || packageIndex.target !== target.target || packageIndex.packageRoot !== "package") fail("managed_runtime_index_invalid", "package index identity mismatch");
  const installedManifest = await readJson(join(targetRoot, "runtime-manifest.json"), "managed_runtime_manifest_missing");
  if (typeof installedManifest !== "object" || installedManifest === null || Array.isArray(installedManifest)) fail("managed_runtime_manifest_invalid", "runtime manifest must be an object");
  const installedIdentity = installedManifest as Record<string, unknown>;
  if (installedIdentity.schemaVersion !== 1 || installedIdentity.contractVersion !== 1 || installedIdentity.runtimeId !== manifest.runtimeId || installedIdentity.runtimeVersion !== manifest.runtimeVersion) fail("managed_runtime_manifest_invalid", "runtime manifest identity mismatch");
  const installedPolicy = installedIdentity.updatePolicy;
  if (!installedPolicy || typeof installedPolicy !== "object" || Array.isArray(installedPolicy) || (installedPolicy as Record<string, unknown>).alfredManaged !== true || (installedPolicy as Record<string, unknown>).selfUpdateAllowed !== false || (installedPolicy as Record<string, unknown>).pathLookupAllowed !== false) fail("managed_runtime_manifest_invalid", "runtime update policy mismatch");
  const runtimeTarget = (installedManifest as Record<string, unknown>).targets;
  if (!Array.isArray(runtimeTarget) || runtimeTarget.length !== 1 || (runtimeTarget[0] as Record<string, unknown>).target !== target.target) fail("managed_runtime_manifest_invalid", "runtime manifest target mismatch");
  const sourceManifestValue = await readJson(join(targetRoot, "source-manifest.json"), "managed_runtime_source_manifest_missing");
  const parsedSource = parseRuntimeSourceManifest(sourceManifestValue);
  if (JSON.stringify(parsedSource) !== JSON.stringify(manifest)) fail("managed_runtime_manifest_invalid", "source manifest does not match the code-owned source");
  await verifyPackageFiles(join(targetRoot, "package"), packageIndex, manifest, target);
  const packageFiles = packageIndex.files as Array<Record<string, unknown>>;
  const executable = packageFiles.find((entry) => entry.relativePath === target.package.executable);
  const expectedExecutable = target.artifact?.extractedSha256 ?? target.artifact?.sha256;
  if (expectedExecutable && executable?.sha256 !== expectedExecutable) fail("managed_runtime_digest_mismatch", target.package.executable);
  const runtimeTargetRecord = runtimeTarget[0] as Record<string, unknown>;
  const installedPublisher = runtimeTargetRecord.publisherVerification;
  if (!installedPublisher || typeof installedPublisher !== "object" || Array.isArray(installedPublisher) || (installedPublisher as Record<string, unknown>).scheme !== target.publisherVerification.scheme || (installedPublisher as Record<string, unknown>).publisher !== target.publisherVerification.publisher || (installedPublisher as Record<string, unknown>).required !== true) fail("managed_runtime_manifest_invalid", "runtime publisher requirement mismatch");
  const installedLicenseNotice = runtimeTargetRecord.licenseNotice;
  if (!installedLicenseNotice || typeof installedLicenseNotice !== "object" || Array.isArray(installedLicenseNotice) || (installedLicenseNotice as Record<string, unknown>).licenseExpression !== manifest.legal.licenseExpression || (installedLicenseNotice as Record<string, unknown>).licenseResourcePath !== manifest.legal.licenseResourcePath || (installedLicenseNotice as Record<string, unknown>).noticeResourcePath !== manifest.legal.noticeResourcePath) fail("managed_runtime_manifest_invalid", "runtime legal requirement mismatch");
  const installedRollback = runtimeTargetRecord.rollback;
  if (!installedRollback || typeof installedRollback !== "object" || Array.isArray(installedRollback) || (installedRollback as Record<string, unknown>).retainPreviousVerified !== true || (installedRollback as Record<string, unknown>).automaticFallback !== false) fail("managed_runtime_manifest_invalid", "runtime rollback policy mismatch");
  const runtimeExecutable = runtimeTargetRecord.executable;
  if (!runtimeExecutable || typeof runtimeExecutable !== "object" || Array.isArray(runtimeExecutable) || (runtimeExecutable as Record<string, unknown>).relativePath !== executable?.relativePath || (runtimeExecutable as Record<string, unknown>).sha256 !== executable?.sha256) fail("managed_runtime_manifest_invalid", "runtime executable does not match package index");
  const runtimeResources = runtimeTargetRecord.resources;
  if (!Array.isArray(runtimeResources) || runtimeResources.length !== packageFiles.length - 1) fail("managed_runtime_manifest_invalid", "runtime resources do not match package index");
  const resourcePairs = new Set(runtimeResources.map((entry) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) fail("managed_runtime_manifest_invalid", "runtime resource is invalid");
    const resource = entry as Record<string, unknown>;
    if (typeof resource.relativePath !== "string" || typeof resource.sha256 !== "string") fail("managed_runtime_manifest_invalid", "runtime resource is incomplete");
    return `${resource.relativePath}:${resource.sha256}`;
  }));
  for (const entry of packageFiles) {
    if (entry.relativePath !== executable?.relativePath && !resourcePairs.has(`${entry.relativePath}:${entry.sha256}`)) fail("managed_runtime_manifest_invalid", `runtime resource missing ${entry.relativePath}`);
  }
  await verifyFile(join(targetRoot, "package", manifest.legal.licenseResourcePath), manifest.legal.licenseSha256);
  await verifyFile(join(targetRoot, "package", manifest.legal.noticeResourcePath), manifest.legal.noticeSha256);
  await verifyPublisherEvidence(manifest, target, targetRoot, options);
}

export async function verifySourceManifests(): Promise<RuntimeSourceManifest[]> {
  const manifests = await loadAllRuntimeSourceManifests();
  for (const manifest of manifests) assertNoLatestOrFallback(manifest);
  return manifests;
}

export function fixturePublisherHook(): (context: PublisherEvidenceContext) => void {
  return ({ evidenceRoot, target }) => {
    // Tests and hermetic verification can provide this hook without invoking
    // a platform binary or making a network request.
    if (!target.publisherVerification.required || !evidenceRoot) throw new Error("publisher hook contract violated");
  };
}

async function environmentPublisherHook(context: PublisherEvidenceContext): Promise<void> {
  const command = process.env.ALFRED_RUNTIME_PUBLISHER_VERIFY;
  if (!command) fail("managed_runtime_publisher_hook_unavailable", context.target.publisherVerification.hook);
  try {
    await execFileAsync(command, [context.evidenceRoot, context.packageRoot], { maxBuffer: 1024 * 1024 });
  } catch (error) {
    fail("managed_runtime_publisher_verification_failed", error instanceof Error ? error.message.split("\n")[0] : "publisher hook failed");
  }
}

export function digestText(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

function parseArgs(args: string[]): VerifyOptions & { sourcesOnly?: boolean } {
  let target = "";
  const options: Partial<VerifyOptions> & { sourcesOnly?: boolean } = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const next = args[index + 1];
    if (arg === "--target") target = next ?? fail("managed_runtime_arguments_invalid", "--target requires a value");
    else if (arg === "--root") options.root = next ?? fail("managed_runtime_arguments_invalid", "--root requires a value");
    else if (arg === "--offline") options.offline = true;
    else if (arg === "--sources-only") options.sourcesOnly = true;
    else fail("managed_runtime_arguments_invalid", `unknown option ${arg}`);
    if (arg === "--target" || arg === "--root") index += 1;
  }
  if (!options.sourcesOnly && (!target || target === "latest" || /[/\\]/.test(target))) fail("managed_runtime_arguments_invalid", "an exact --target is required");
  return { target, ...options };
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  const manifests = await verifySourceManifests();
  if (options.sourcesOnly) {
    manifests.forEach((manifest) => console.log(`source-ok ${manifest.runtimeId} ${manifest.runtimeVersion}`));
    return;
  }
  for (const manifest of manifests) {
    await verifyPreparedRuntimeTarget(manifest, options.target, {
      ...options,
      offline: options.offline === true,
      publisherHook: environmentPublisherHook,
    });
    console.log(`verified ${manifest.runtimeId} ${manifest.runtimeVersion} ${options.target}`);
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
