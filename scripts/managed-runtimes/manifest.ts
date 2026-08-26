import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type VerificationScheme =
  | "apple_developer_id"
  | "windows_authenticode"
  | "platform_package_signature"
  | "sigstore_bundle";

export type ArtifactSource = {
  fileName: string;
  url: string;
  sha256: string;
  size?: number;
  extractedFileName?: string;
  extractedSha256?: string;
  extractedSize?: number;
};

export type RuntimeTargetSource = {
  target: string;
  artifact?: ArtifactSource;
  python?: ArtifactSource;
  cliWheel?: ArtifactSource;
  pydanticCoreWheel?: ArtifactSource;
  package: { executable: string };
  publisherVerification: {
    scheme: VerificationScheme;
    publisher: string;
    required: true;
    hook: string;
    manifestSignatureRequired?: boolean;
  };
};

export type LegalSource = {
  licenseExpression: string;
  licenseResourcePath: string;
  licenseSha256: string;
  noticeResourcePath: string;
  noticeSha256: string;
  licenseSource?: string;
  noticeSource?: string;
  inventory?: Array<{ name: string; expression: string; required: true }>;
};

export type RuntimeSourceManifest = {
  schemaVersion: 1;
  runtimeId: string;
  runtimeVersion: string;
  source: Record<string, unknown>;
  updatePolicy: {
    alfredManaged: true;
    selfUpdateAllowed: false;
    pathLookupAllowed: false;
    automaticFallback: false;
  };
  legal: LegalSource;
  packageLayout?: Record<string, string>;
  sdkSdist?: ArtifactSource;
  targets: RuntimeTargetSource[];
  wheels?: Array<ArtifactSource & { name: string; version: string; scope: string }>;
};

export type PackageFile = { relativePath: string; sha256: string; executable?: boolean };

export type RuntimePackageManifest = {
  schemaVersion: 1;
  contractVersion: 1;
  runtimeId: string;
  runtimeVersion: string;
  updatePolicy: {
    alfredManaged: true;
    selfUpdateAllowed: false;
    pathLookupAllowed: false;
  };
  targets: Array<{
    target: string;
    executable: { relativePath: string; sha256: string };
    resources: Array<{ relativePath: string; sha256: string }>;
    publisherVerification: {
      scheme: VerificationScheme;
      publisher: string;
      required: true;
    };
    licenseNotice: {
      licenseExpression: string;
      licenseResourcePath: string;
      noticeResourcePath: string;
    };
    rollback: { retainPreviousVerified: true; automaticFallback: false };
  }>;
};

const MANIFEST_DIRECTORY = fileURLToPath(new URL("./manifests", import.meta.url));
const HASH = /^[0-9a-f]{64}$/;
const SCHEMES: VerificationScheme[] = [
  "apple_developer_id",
  "windows_authenticode",
  "platform_package_signature",
  "sigstore_bundle",
];

export class ManagedRuntimeManifestError extends Error {
  constructor(readonly code: string, message: string) {
    super(`${code}: ${message}`);
    this.name = "ManagedRuntimeManifestError";
  }
}

function invalid(message: string): never {
  throw new ManagedRuntimeManifestError("managed_runtime_manifest_invalid", message);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 4096) {
    invalid(`${field} must be a bounded non-empty string`);
  }
  return value;
}

function requireHash(value: unknown, field: string): string {
  const hash = requireString(value, field);
  if (!HASH.test(hash)) invalid(`${field} must be a lowercase SHA-256 digest`);
  return hash;
}

function requireSafeRelativePath(value: unknown, field: string): string {
  const path = requireString(value, field).replaceAll("\\", "/");
  if (
    path.startsWith("/") ||
    path.includes("\0") ||
    path.includes(":") ||
    path.length > 512 ||
    path.split("/").some((part) => part === "" || part === "." || part === "..") ||
    path.split("/").some((part) => part.length > 128 || !/^[A-Za-z0-9._+-]+$/.test(part)) ||
    isAbsolute(path)
  ) {
    invalid(`${field} must be a safe relative path`);
  }
  return path;
}

function requireHttpsUrl(value: unknown, field: string): string {
  const raw = requireString(value, field);
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    invalid(`${field} must be an absolute URL`);
  }
  if (url.protocol !== "https:" || /latest|fallback|latest-version/i.test(raw)) {
    invalid(`${field} must be an exact HTTPS URL without latest/fallback semantics`);
  }
  return raw;
}

function validateArtifact(value: unknown, field: string): ArtifactSource {
  if (!isRecord(value)) invalid(`${field} must be an object`);
  const fileName = requireString(value.fileName, `${field}.fileName`);
  if (fileName.includes("/") || fileName.includes("\\")) {
    invalid(`${field}.fileName must be a basename`);
  }
  const url = requireHttpsUrl(value.url, `${field}.url`);
  if (decodeURIComponent(new URL(url).pathname).split("/").at(-1) !== fileName) {
    invalid(`${field}.fileName must match the URL basename`);
  }
  const artifact: ArtifactSource = {
    fileName,
    url,
    sha256: requireHash(value.sha256, `${field}.sha256`),
  };
  for (const [key, target] of [
    ["size", "size"],
    ["extractedSize", "extractedSize"],
  ] as const) {
    if (value[key] !== undefined && (!Number.isSafeInteger(value[key]) || Number(value[key]) <= 0)) {
      invalid(`${field}.${target} must be a positive safe integer`);
    }
    if (value[key] !== undefined) (artifact as Record<string, unknown>)[key] = value[key];
  }
  if (value.extractedFileName !== undefined) {
    const extractedFileName = requireString(value.extractedFileName, `${field}.extractedFileName`);
    if (extractedFileName.includes("/") || extractedFileName.includes("\\")) {
      invalid(`${field}.extractedFileName must be a basename`);
    }
    artifact.extractedFileName = extractedFileName;
    artifact.extractedSha256 = requireHash(value.extractedSha256, `${field}.extractedSha256`);
  }
  return artifact;
}

function validateTarget(value: unknown, index: number): RuntimeTargetSource {
  if (!isRecord(value)) invalid(`targets[${index}] must be an object`);
  const target = requireString(value.target, `targets[${index}].target`);
  if (target === "latest" || target.includes("/") || target.includes("\\")) {
    invalid(`targets[${index}].target must be an exact target`);
  }
  if (!isRecord(value.package)) invalid(`targets[${index}].package must be an object`);
  const executable = requireSafeRelativePath(
    value.package.executable,
    `targets[${index}].package.executable`,
  );
  if (!isRecord(value.publisherVerification)) {
    invalid(`targets[${index}].publisherVerification must be an object`);
  }
  const scheme = value.publisherVerification.scheme;
  if (typeof scheme !== "string" || !SCHEMES.includes(scheme as VerificationScheme)) {
    invalid(`targets[${index}] has an unsupported publisher verification scheme`);
  }
  if (value.publisherVerification.required !== true) {
    invalid(`targets[${index}] requires publisher verification`);
  }
  const publisherVerification = {
    scheme: scheme as VerificationScheme,
    publisher: requireString(value.publisherVerification.publisher, `targets[${index}].publisher`),
    required: true as const,
    hook: requireString(value.publisherVerification.hook, `targets[${index}].hook`),
    ...(value.publisherVerification.manifestSignatureRequired === undefined
      ? {}
      : typeof value.publisherVerification.manifestSignatureRequired === "boolean"
        ? { manifestSignatureRequired: value.publisherVerification.manifestSignatureRequired }
        : invalid(`targets[${index}].manifestSignatureRequired must be boolean`)),
  };
  const result: RuntimeTargetSource = { target, package: { executable }, publisherVerification };
  if (value.artifact !== undefined) result.artifact = validateArtifact(value.artifact, `targets[${index}].artifact`);
  if (value.python !== undefined) result.python = validateArtifact(value.python, `targets[${index}].python`);
  if (value.cliWheel !== undefined) result.cliWheel = validateArtifact(value.cliWheel, `targets[${index}].cliWheel`);
  if (value.pydanticCoreWheel !== undefined) {
    result.pydanticCoreWheel = validateArtifact(value.pydanticCoreWheel, `targets[${index}].pydanticCoreWheel`);
  }
  if (!result.artifact && (!result.python || !result.cliWheel)) {
    invalid(`targets[${index}] must declare an artifact or Python/CLI inputs`);
  }
  return result;
}

export function parseRuntimeSourceManifest(value: unknown): RuntimeSourceManifest {
  if (!isRecord(value)) invalid("manifest must be an object");
  if (value.schemaVersion !== 1) invalid("schemaVersion must be 1");
  const runtimeId = requireString(value.runtimeId, "runtimeId");
  if (!/^[a-z0-9][a-z0-9_-]{0,127}$/.test(runtimeId)) invalid("runtimeId must be a safe component");
  const runtimeVersion = requireString(value.runtimeVersion, "runtimeVersion");
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(runtimeVersion) || runtimeVersion === "latest" || /latest|fallback/i.test(runtimeVersion)) {
    invalid("runtimeVersion must be exact");
  }
  if (!isRecord(value.updatePolicy) || value.updatePolicy.alfredManaged !== true || value.updatePolicy.selfUpdateAllowed !== false || value.updatePolicy.pathLookupAllowed !== false || value.updatePolicy.automaticFallback !== false) {
    invalid("updatePolicy must be Alfred-managed with no self-update, PATH lookup, or fallback");
  }
  if (!isRecord(value.legal)) invalid("legal must be an object");
  const source = isRecord(value.source) ? value.source : invalid("source must be an object");
  let sdkSdist: ArtifactSource | undefined;
  if (source.sdkSdist !== undefined) sdkSdist = validateArtifact(source.sdkSdist, "source.sdkSdist");
  const legal: LegalSource = {
    licenseExpression: requireString(value.legal.licenseExpression, "legal.licenseExpression"),
    licenseResourcePath: requireSafeRelativePath(value.legal.licenseResourcePath, "legal.licenseResourcePath"),
    licenseSha256: requireHash(value.legal.licenseSha256, "legal.licenseSha256"),
    noticeResourcePath: requireSafeRelativePath(value.legal.noticeResourcePath, "legal.noticeResourcePath"),
    noticeSha256: requireHash(value.legal.noticeSha256, "legal.noticeSha256"),
  };
  if (legal.licenseResourcePath === legal.noticeResourcePath) invalid("license and notice paths must differ");
  for (const key of ["licenseSource", "noticeSource"] as const) {
    if (value.legal[key] !== undefined) legal[key] = requireSafeRelativePath(value.legal[key], `legal.${key}`);
  }
  if (value.legal.inventory !== undefined) {
    if (!Array.isArray(value.legal.inventory) || value.legal.inventory.length === 0) invalid("legal.inventory must be non-empty");
    const inventoryNames = new Set<string>();
    legal.inventory = value.legal.inventory.map((entry, index) => {
      if (!isRecord(entry) || entry.required !== true) invalid(`legal.inventory[${index}] is invalid`);
      const name = requireString(entry.name, `legal.inventory[${index}].name`);
      if (inventoryNames.has(name)) invalid(`duplicate legal.inventory name ${name}`);
      inventoryNames.add(name);
      return { name, expression: requireString(entry.expression, `legal.inventory[${index}].expression`), required: true as const };
    });
  }
  if (value.packageLayout !== undefined && !isRecord(value.packageLayout)) invalid("packageLayout must be an object");
  if (value.wheels !== undefined && !Array.isArray(value.wheels)) invalid("wheels must be an array");
  if (!Array.isArray(value.targets) || value.targets.length === 0 || value.targets.length > 32) invalid("targets must be bounded and non-empty");
  const targets = value.targets.map(validateTarget);
  const seen = new Set<string>();
  for (const target of targets) {
    const key = target.target.toLowerCase();
    if (seen.has(key)) invalid(`duplicate target ${target.target}`);
    seen.add(key);
  }
  return {
    schemaVersion: 1,
    runtimeId,
    runtimeVersion,
    source,
    updatePolicy: { alfredManaged: true, selfUpdateAllowed: false, pathLookupAllowed: false, automaticFallback: false },
    legal,
    ...(isRecord(value.packageLayout) ? { packageLayout: Object.fromEntries(Object.entries(value.packageLayout).map(([key, entry]) => [key, requireSafeRelativePath(entry, `packageLayout.${key}`)])) } : {}),
    ...(sdkSdist ? { sdkSdist } : {}),
    targets,
    ...(Array.isArray(value.wheels) ? { wheels: value.wheels.map((wheel, index) => {
      if (!isRecord(wheel)) invalid(`wheels[${index}] is invalid`);
      return Object.assign(validateArtifact(wheel, `wheels[${index}]`), {
        name: requireString(wheel.name, `wheels[${index}].name`),
        version: requireString(wheel.version, `wheels[${index}].version`),
        scope: requireString(wheel.scope, `wheels[${index}].scope`),
      });
    }) } : {}),
  };
}

export async function loadRuntimeSourceManifest(name: string): Promise<RuntimeSourceManifest> {
  if (!/^[a-z0-9][a-z0-9.-]+\.json$/.test(name)) invalid("manifest name is invalid");
  const path = resolve(MANIFEST_DIRECTORY, name);
  if (dirname(path) !== resolve(MANIFEST_DIRECTORY)) invalid("manifest path escaped source directory");
  let parsed: unknown;
  try {
    parsed = JSON.parse(await readFile(path, "utf8"));
  } catch {
    throw new ManagedRuntimeManifestError("managed_runtime_manifest_missing", `cannot read ${name}`);
  }
  return parseRuntimeSourceManifest(parsed);
}

export async function loadAllRuntimeSourceManifests(): Promise<RuntimeSourceManifest[]> {
  return Promise.all([
    loadRuntimeSourceManifest("claude-code-2.1.246.json"),
    loadRuntimeSourceManifest("opencode-1.18.23.json"),
    loadRuntimeSourceManifest("codex-python-sdk-0.147.0.json"),
  ]);
}

export function sourceTarget(manifest: RuntimeSourceManifest, target: string): RuntimeTargetSource {
  if (target === "latest" || /[/\\]/.test(target)) invalid("target must be exact");
  const selected = manifest.targets.find((entry) => entry.target === target);
  if (!selected) throw new ManagedRuntimeManifestError("managed_runtime_target_unsupported", `${manifest.runtimeId} has no ${target}`);
  return selected;
}

export function sha256Bytes(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

export function sha256FileBytes(bytes: Uint8Array, expected: string, label: string): void {
  if (sha256Bytes(bytes) !== expected) {
    throw new ManagedRuntimeManifestError("managed_runtime_digest_mismatch", `${label} did not match its pinned SHA-256`);
  }
}

/** Build the JSON shape consumed by RuntimePackageStore's native verifier. */
export function buildRuntimePackageManifest(
  source: RuntimeSourceManifest,
  target: RuntimeTargetSource,
  files: PackageFile[],
): RuntimePackageManifest {
  if (files.length === 0 || files.length > 128) {
    throw new ManagedRuntimeManifestError("managed_runtime_resource_count_exceeded", "RuntimePackageStore permits at most 128 declared files");
  }
  const executable = files.find((entry) => entry.relativePath === target.package.executable);
  if (!executable) throw new ManagedRuntimeManifestError("managed_runtime_executable_missing", target.package.executable);
  const license = files.find((entry) => entry.relativePath === source.legal.licenseResourcePath);
  const notice = files.find((entry) => entry.relativePath === source.legal.noticeResourcePath);
  if (!license || !notice) throw new ManagedRuntimeManifestError("managed_runtime_legal_missing", "license and notice are required");
  return {
    schemaVersion: 1,
    contractVersion: 1,
    runtimeId: source.runtimeId,
    runtimeVersion: source.runtimeVersion,
    updatePolicy: { alfredManaged: true, selfUpdateAllowed: false, pathLookupAllowed: false },
    targets: [{
      target: target.target,
      executable: { relativePath: executable.relativePath, sha256: executable.sha256 },
      resources: files.filter((entry) => entry.relativePath !== executable.relativePath).map((entry) => ({ relativePath: entry.relativePath, sha256: entry.sha256 })),
      // PublisherVerificationScheme uses serde(rename_all = "snake_case")
      // in the Rust package store; preserve the source spelling exactly.
      publisherVerification: { scheme: target.publisherVerification.scheme, publisher: target.publisherVerification.publisher, required: true },
      licenseNotice: { licenseExpression: source.legal.licenseExpression, licenseResourcePath: source.legal.licenseResourcePath, noticeResourcePath: source.legal.noticeResourcePath },
      rollback: { retainPreviousVerified: true, automaticFallback: false },
    }],
  };
}

export function assertNoLatestOrFallback(value: unknown, path = "manifest"): void {
  if (typeof value === "string" && /latest|fallback/i.test(value)) {
    throw new ManagedRuntimeManifestError("managed_runtime_unpinned", `${path} contains latest/fallback`);
  }
  if (Array.isArray(value)) value.forEach((entry, index) => assertNoLatestOrFallback(entry, `${path}[${index}]`));
  else if (isRecord(value)) Object.entries(value).forEach(([key, entry]) => assertNoLatestOrFallback(entry, `${path}.${key}`));
}

export function resolveRepositoryPath(path: string): string {
  const root = resolve(MANIFEST_DIRECTORY, "../..", "..");
  const resolved = resolve(root, path);
  const escaped = relative(root, resolved);
  if (escaped.startsWith("..") || isAbsolute(escaped)) {
    throw new ManagedRuntimeManifestError("managed_runtime_path_unsafe", path);
  }
  return resolved;
}

export function relativePackagePath(path: string, root: string): string {
  const value = relative(root, path).replaceAll("\\", "/");
  return requireSafeRelativePath(value, "package path");
}
