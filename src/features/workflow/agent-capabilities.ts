import type { AgentHarness, AgentProviderId } from "./types";

export type CapabilityStatus = "disabled" | "beta" | "available" | "blocked";
export type GateStatus = "passed" | "failed" | "not_applicable";
export type DesktopPlatform = "macos" | "windows" | "linux";
export type DesktopBuildKind = "development" | "packaged";

export type AgentCapabilityGate = {
  gate: string;
  status: GateStatus;
  reason?: string;
};

export type AuthMethodGate = {
  authMethod: string;
  status: GateStatus;
  reason?: string;
};

export type PlatformGate = {
  platform: DesktopPlatform;
  status: GateStatus;
  reason?: string;
};

export type BuildGate = {
  buildKind: DesktopBuildKind;
  status: GateStatus;
  reason?: string;
};

export type PackagedRuntimeMetadata = {
  kind: string;
  included: boolean;
  resourcePath?: string;
  checksumStatus: string;
  sha256?: string;
  license: string;
  licenseResourcePath?: string;
  noticeResourcePath?: string;
  signingResourcePath?: string;
  rollbackResourcePath?: string;
  signingStatus: string;
  rollbackStatus: string;
  dataIndependent: boolean;
  automaticFallback: boolean;
};

export type AgentCapabilityEntry = {
  provider: AgentProviderId;
  harness: AgentHarness;
  runtimeVersion: string | null;
  platforms: DesktopPlatform[];
  buildKinds: DesktopBuildKind[];
  authMethods: string[];
  authMethodGates: AuthMethodGate[];
  platformGates: PlatformGate[];
  buildGates: BuildGate[];
  billingSource: string;
  credentialCustody: string;
  modelSource: string;
  usageSource: string;
  supportsTools: boolean;
  supportsApprovals: boolean;
  supportsResume: boolean;
  supportsCancellation: boolean;
  status: CapabilityStatus;
  blockReason: string | null;
  executionPermitted: boolean;
  gates: AgentCapabilityGate[];
  package?: PackagedRuntimeMetadata;
};

export type AgentCapabilityManifest = {
  schemaVersion: number;
  platform: DesktopPlatform;
  buildKind: DesktopBuildKind;
  entries: AgentCapabilityEntry[];
};

export function capabilityFor(
  manifest: AgentCapabilityManifest | null,
  provider: AgentProviderId,
  harness: AgentHarness,
): AgentCapabilityEntry | null {
  if (!manifestIsValid(manifest)) return null;
  return (
    manifest.entries.find(
      (entry) => entry.provider === provider && entry.harness === harness,
    ) ?? null
  );
}

export function manifestIsValid(
  manifest: AgentCapabilityManifest | null,
): manifest is AgentCapabilityManifest {
  if (!manifest || manifest.schemaVersion !== 1 || manifest.entries.length > 64) {
    return false;
  }
  const keys = new Set<string>();
  for (const entry of manifest.entries) {
    const key = `${entry.provider}:${entry.harness}`;
    if (keys.has(key)) return false;
    keys.add(key);
    if (
      (entry.status === "blocked" || entry.status === "disabled") &&
      !entry.blockReason
    ) {
      return false;
    }
    if (
      entry.harness === "alfred" &&
      (entry.status === "available" || entry.status === "beta") &&
      !entry.runtimeVersion
    ) {
      return false;
    }
    if (entry.package?.automaticFallback) return false;
    if (typeof entry.executionPermitted !== "boolean") return false;
    if (
      entry.platforms.some(
        (platform) =>
          entry.platformGates.filter((gate) => gate.platform === platform).length !== 1,
      ) ||
      entry.buildKinds.some(
        (buildKind) =>
          entry.buildGates.filter((gate) => gate.buildKind === buildKind).length !== 1,
      ) ||
      entry.authMethods.some(
        (authMethod) =>
          entry.authMethodGates.filter((gate) => gate.authMethod === authMethod)
            .length !== 1,
      )
    ) {
      return false;
    }
  }
  return true;
}

/** Consumes the backend decision; package trust is never reconstructed in JS. */
export function capabilityPermitsExecution(
  manifest: AgentCapabilityManifest | null,
  provider: AgentProviderId,
  harness: AgentHarness,
): boolean {
  const entry = capabilityFor(manifest, provider, harness);
  return entry?.executionPermitted === true;
}

export function capabilityStatusLabel(entry: AgentCapabilityEntry | null): string {
  if (!entry) return "disabled (missing manifest entry)";
  return entry.status;
}

export function capabilityReason(entry: AgentCapabilityEntry | null): string {
  const code = entry?.blockReason ?? "native_capability_manifest_entry_missing";
  return code.split("_").join(" ");
}

export function nativeProviderRetargetDisabled(
  manifest: AgentCapabilityManifest | null,
  currentProvider: AgentProviderId,
  candidateProvider: AgentProviderId,
  harness: AgentHarness,
): boolean {
  return (
    harness === "alfred" &&
    candidateProvider !== currentProvider &&
    !capabilityPermitsExecution(manifest, candidateProvider, "alfred")
  );
}
