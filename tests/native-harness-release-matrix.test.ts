import { describe, expect, test } from "bun:test";
import {
  capabilityPermitsExecution,
  capabilityReason,
  manifestIsValid,
  nativeProviderRetargetDisabled,
  type AgentCapabilityEntry,
  type AgentCapabilityManifest,
  type DesktopBuildKind,
  type DesktopPlatform,
} from "../src/features/workflow/agent-capabilities";
import type { AgentProviderId } from "../src/features/workflow/types";

const fixture = (await Bun.file(
  new URL("./fixtures/native-harness-release-matrix.json", import.meta.url),
).json()) as {
  schemaVersion: number;
  cases: { platform: DesktopPlatform; buildKind: DesktopBuildKind }[];
  cliProviders: AgentProviderId[];
  enabledNativeProviders: AgentProviderId[];
};

function entry(
  provider: AgentProviderId,
  harness: "cli" | "alfred",
): AgentCapabilityEntry {
  const available = harness === "cli";
  return {
    provider,
    harness,
    runtimeVersion: null,
    platforms: ["macos", "windows", "linux"],
    buildKinds: ["development", "packaged"],
    authMethods: available ? ["provider_cli"] : [],
    authMethodGates: available
      ? [{ authMethod: "provider_cli", status: "passed" }]
      : [],
    platformGates: ["macos", "windows", "linux"].map((platform) => ({
      platform: platform as DesktopPlatform,
      status: available ? ("passed" as const) : ("failed" as const),
    })),
    buildGates: ["development", "packaged"].map((buildKind) => ({
      buildKind: buildKind as DesktopBuildKind,
      status: available ? ("passed" as const) : ("failed" as const),
    })),
    billingSource: available ? "provider_cli_account" : "unavailable",
    credentialCustody: available ? "provider_cli_managed" : "unavailable",
    modelSource: available ? "provider_cli" : "unavailable",
    usageSource: "unavailable",
    supportsTools: available,
    supportsApprovals: false,
    supportsResume: false,
    supportsCancellation: true,
    status: available ? "available" : "blocked",
    blockReason: available ? null : "fixture_native_gate_blocked",
    executionPermitted: available,
    gates: [
      {
        gate: "release",
        status: available ? "passed" : "failed",
      },
    ],
    package: {
      kind: "not_applicable",
      included: false,
      checksumStatus: "not_applicable",
      license: "user_installed_or_https",
      signingStatus: "not_applicable",
      rollbackStatus: "not_applicable",
      dataIndependent: true,
      automaticFallback: false,
    },
  };
}

describe("native harness release matrix", () => {
  test("keeps every CLI provider available and every native provider closed", () => {
    for (const releaseCase of fixture.cases) {
      const manifest: AgentCapabilityManifest = {
        schemaVersion: fixture.schemaVersion,
        ...releaseCase,
        entries: fixture.cliProviders.flatMap((provider) => [
          entry(provider, "cli"),
          entry(provider, "alfred"),
        ]),
      };
      expect(manifestIsValid(manifest)).toBe(true);
      for (const provider of fixture.cliProviders) {
        expect(capabilityPermitsExecution(manifest, provider, "cli")).toBe(true);
        expect(capabilityPermitsExecution(manifest, provider, "alfred")).toBe(
          fixture.enabledNativeProviders.includes(provider),
        );
      }
    }
  });

  test("missing entries disable native and return a safe reason", () => {
    const manifest: AgentCapabilityManifest = {
      schemaVersion: 1,
      platform: "macos",
      buildKind: "packaged",
      entries: [],
    };
    expect(capabilityPermitsExecution(manifest, "codex", "alfred")).toBe(false);
    expect(capabilityReason(null)).toBe(
      "native capability manifest entry missing",
    );

    const duplicate = entry("codex", "cli");
    manifest.entries = [duplicate, duplicate];
    expect(manifestIsValid(manifest)).toBe(false);
    expect(capabilityPermitsExecution(manifest, "codex", "cli")).toBe(false);
  });

  test("uses the backend decision and prevents blocked native retargeting", () => {
    const native = entry("cursor", "alfred");
    native.status = "available";
    native.blockReason = null;
    native.runtimeVersion = "fixture-1.0.0";
    native.authMethods = ["api_key"];
    native.authMethodGates = [{ authMethod: "api_key", status: "passed" }];
    native.platformGates = native.platformGates.map((gate) => ({
      ...gate,
      status: "passed",
    }));
    native.buildGates = native.buildGates.map((gate) => ({
      ...gate,
      status: "passed",
    }));
    native.gates = [{ gate: "release", status: "passed" }];
    native.package = {
      ...native.package!,
      kind: "sidecar",
      included: true,
      checksumStatus: "verified",
      signingStatus: "verified",
      rollbackStatus: "verified",
    };
    native.executionPermitted = false;
    const manifest: AgentCapabilityManifest = {
      schemaVersion: 1,
      platform: "macos",
      buildKind: "packaged",
      entries: [entry("codex", "alfred"), native],
    };

    expect(capabilityPermitsExecution(manifest, "cursor", "alfred")).toBe(false);
    expect(nativeProviderRetargetDisabled(manifest, "codex", "cursor", "alfred")).toBe(
      true,
    );
    expect(nativeProviderRetargetDisabled(manifest, "codex", "codex", "alfred")).toBe(
      false,
    );
    expect(nativeProviderRetargetDisabled(manifest, "codex", "cursor", "cli")).toBe(
      false,
    );
  });
});
