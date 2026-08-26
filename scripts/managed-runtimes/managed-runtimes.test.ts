import { createHash } from "node:crypto";
import { chmod, mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

import {
  buildRuntimePackageManifest,
  loadAllRuntimeSourceManifests,
  parseRuntimeSourceManifest,
  sha256Bytes,
  type RuntimeSourceManifest,
} from "./manifest";
import { downloadExact } from "./download";
import {
  digestText,
  fixturePublisherHook,
  verifyPreparedRuntimeTarget,
} from "./verify";

const digest = (value: string) => createHash("sha256").update(value).digest("hex");

describe("managed runtime source manifests", () => {
  test("pin the three provider versions and every supported target", async () => {
    const manifests = await loadAllRuntimeSourceManifests();
    expect(manifests.map((manifest) => `${manifest.runtimeId}@${manifest.runtimeVersion}`)).toEqual([
      "claude_code_managed@2.1.246",
      "opencode_server@1.18.23",
      "codex_python_sdk@0.147.0",
    ]);
    expect(manifests.map((manifest) => manifest.targets.length)).toEqual([8, 6, 8]);
    for (const manifest of manifests) {
      expect(manifest.runtimeVersion).not.toMatch(/latest|fallback/i);
      for (const target of manifest.targets) {
        const artifact = target.artifact ?? target.python;
        expect(artifact?.url).toMatch(/^https:\/\//);
        expect(artifact?.sha256).toMatch(/^[0-9a-f]{64}$/);
        expect(artifact?.url).not.toMatch(/latest|fallback/i);
      }
    }
  });

  test("rejects a latest URL and a missing publisher scheme", () => {
    const base = {
      schemaVersion: 1,
      runtimeId: "fixture_runtime",
      runtimeVersion: "1.0.0",
      source: {},
      updatePolicy: { alfredManaged: true, selfUpdateAllowed: false, pathLookupAllowed: false, automaticFallback: false },
      legal: { licenseExpression: "MIT", licenseResourcePath: "legal/LICENSE", licenseSha256: digest("license"), noticeResourcePath: "legal/NOTICE", noticeSha256: digest("notice") },
      targets: [{
        target: "x86_64-unknown-linux-gnu",
        artifact: { fileName: "tool", url: "https://example.test/latest/tool", sha256: digest("tool") },
        package: { executable: "bin/tool" },
        publisherVerification: { scheme: "platform_package_signature", publisher: "Fixture", required: true, hook: "fixture" },
      }],
    };
    expect(() => parseRuntimeSourceManifest(base)).toThrow("managed_runtime_manifest_invalid");
    expect(() => parseRuntimeSourceManifest({ ...base, targets: [{ ...base.targets[0], artifact: { fileName: "tool", url: "https://example.test/1.0.0/tool", sha256: digest("tool") }, publisherVerification: { ...base.targets[0].publisherVerification, scheme: "unknown" } }] })).toThrow("managed_runtime_manifest_invalid");
  });
});

describe("exact downloader", () => {
  test("uses only injected local-response bytes and verifies the pinned digest", async () => {
    const root = await mkdtemp(join(tmpdir(), "alfred-managed-runtime-download-"));
    const bytes = new TextEncoder().encode("fixture artifact");
    const artifact = { fileName: "fixture.bin", url: "https://example.test/1.0.0/fixture.bin", sha256: sha256Bytes(bytes), size: bytes.byteLength };
    let called = false;
    await downloadExact(artifact, join(root, "cache", artifact.fileName), {
      fetchImpl: async () => {
        called = true;
        return new Response(bytes, { status: 200 });
      },
    });
    expect(called).toBe(true);
    expect(await readFile(join(root, "cache", artifact.fileName))).toEqual(bytes);
  });

  test("offline mode never invokes fetch and fails on a missing cache entry", async () => {
    const root = await mkdtemp(join(tmpdir(), "alfred-managed-runtime-offline-"));
    let called = false;
    await expect(downloadExact({ fileName: "missing", url: "https://example.test/1.0.0/missing", sha256: digest("missing") }, join(root, "missing"), {
      offline: true,
      fetchImpl: async () => {
        called = true;
        return new Response("unexpected");
      },
    })).rejects.toThrow("managed_runtime_offline_input_missing");
    expect(called).toBe(false);
  });
});

describe("offline package verification", () => {
  test("verifies a complete local RuntimePackageStore input tree", async () => {
    const root = await mkdtemp(join(tmpdir(), "alfred-managed-runtime-package-"));
    const packageRoot = join(root, "fixture_runtime", "x86_64-unknown-linux-gnu", "1.0.0", "package");
    const targetRoot = packageRoot.slice(0, -"/package".length);
    await mkdir(join(packageRoot, "bin"), { recursive: true });
    await mkdir(join(packageRoot, "legal"), { recursive: true });
    await writeFile(join(packageRoot, "bin", "tool"), "tool");
    await chmod(join(packageRoot, "bin", "tool"), 0o755);
    await writeFile(join(packageRoot, "legal", "LICENSE"), "license");
    await writeFile(join(packageRoot, "legal", "NOTICE"), "notice");
    const source: RuntimeSourceManifest = parseRuntimeSourceManifest({
      schemaVersion: 1,
      runtimeId: "fixture_runtime",
      runtimeVersion: "1.0.0",
      source: {},
      updatePolicy: { alfredManaged: true, selfUpdateAllowed: false, pathLookupAllowed: false, automaticFallback: false },
      legal: { licenseExpression: "MIT", licenseResourcePath: "legal/LICENSE", licenseSha256: digest("license"), noticeResourcePath: "legal/NOTICE", noticeSha256: digest("notice") },
      targets: [{
        target: "x86_64-unknown-linux-gnu",
        artifact: { fileName: "tool", url: "https://example.test/1.0.0/tool", sha256: digest("tool") },
        package: { executable: "bin/tool" },
        publisherVerification: { scheme: "platform_package_signature", publisher: "Fixture", required: true, hook: "fixture" },
      }],
    });
    const target = source.targets[0];
    const files = [
      { relativePath: "bin/tool", sha256: digest("tool"), executable: true },
      { relativePath: "legal/LICENSE", sha256: digest("license") },
      { relativePath: "legal/NOTICE", sha256: digest("notice") },
    ];
    await writeFile(join(targetRoot, "runtime-manifest.json"), `${JSON.stringify(buildRuntimePackageManifest(source, target, files))}\n`);
    await writeFile(join(targetRoot, "source-manifest.json"), `${JSON.stringify(source)}\n`);
    await writeFile(join(targetRoot, "package-index.json"), `${JSON.stringify({ runtimeId: source.runtimeId, runtimeVersion: source.runtimeVersion, target: target.target, packageRoot: "package", files })}\n`);
    await mkdir(join(targetRoot, "publisher-evidence"), { recursive: true });
    await writeFile(join(targetRoot, "publisher-evidence", "publisher-verification.json"), `${JSON.stringify({ scheme: "platform_package_signature", publisher: "Fixture", evidenceType: "fixture-signature", evidenceDigest: digestText("fixture") })}\n`);
    await verifyPreparedRuntimeTarget(source, target.target, { root, target: target.target, offline: true, publisherHook: fixturePublisherHook() });
  });
});
