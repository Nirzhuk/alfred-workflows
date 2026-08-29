import { spawn, execFile } from "node:child_process";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { promisify } from "node:util";

import { downloadExact } from "./download.ts";
import {
  loadAllRuntimeSourceManifests,
  resolveRepositoryPath,
  sha256Bytes,
  sourceTarget,
} from "./manifest.ts";

const execFileAsync = promisify(execFile);
const TARGET = "aarch64-apple-darwin";
const COMMIT = "025a88adbd7ae4d448fc938b28d0446eb1753317";
const OUTPUT = resolveRepositoryPath("src-tauri/sidecars/managed-runtimes");
const CACHE = resolveRepositoryPath(".cache/managed-runtimes");
const SOURCE_JSON = resolveRepositoryPath("src-tauri/sidecars/codex-sdk/runtime-package.source.json");
const SIDECAR_SRC = resolveRepositoryPath("src-tauri/sidecars/codex-sdk");

const manifests = await loadAllRuntimeSourceManifests();
const codex = manifests.find((manifest) => manifest.runtimeId === "codex_python_sdk");
if (!codex) {
  throw new Error("codex_python_sdk source manifest is missing");
}
const target = sourceTarget(codex, TARGET);
if (!target.cliWheel) {
  throw new Error("Codex source is missing the pinned CLI wheel");
}

await mkdir(CACHE, { recursive: true });
const wheelPath = join(CACHE, `${target.cliWheel.sha256}-${target.cliWheel.fileName}`);
const licensePath = join(CACHE, `${codex.legal.licenseSha256}-LICENSE`);
const noticePath = join(CACHE, `${codex.legal.noticeSha256}-NOTICE`);
await downloadExact(target.cliWheel, wheelPath);
await downloadExact(
  {
    fileName: "LICENSE",
    url: `https://raw.githubusercontent.com/openai/codex/${COMMIT}/LICENSE`,
    sha256: codex.legal.licenseSha256,
    size: 10926,
  },
  licensePath,
);
await downloadExact(
  {
    fileName: "NOTICE",
    url: `https://raw.githubusercontent.com/openai/codex/${COMMIT}/NOTICE`,
    sha256: codex.legal.noticeSha256,
    size: 242,
  },
  noticePath,
);

const work = join(CACHE, `codex-local-${TARGET}`);
await execFileAsync("rm", ["-rf", work]);
await mkdir(join(work, "wheel"), { recursive: true });
await execFileAsync("unzip", ["-q", wheelPath, "-d", join(work, "wheel")]);
const cliBinary = await findNamedFile(join(work, "wheel"), "codex");

const sidecar = await buildSidecar();

const versionRoot = join(OUTPUT, codex.runtimeId, TARGET, codex.runtimeVersion);
const packageRoot = join(versionRoot, "package");
await execFileAsync("rm", ["-rf", versionRoot]);
await mkdir(join(packageRoot, "bin"), { recursive: true });
await mkdir(join(packageRoot, "libexec"), { recursive: true });
await mkdir(join(packageRoot, "legal", "openai-codex"), { recursive: true });

await copyVerified(sidecar, join(packageRoot, "bin/alfred-codex-sdk-sidecar"), true);
await copyVerified(cliBinary, join(packageRoot, "libexec/codex"), true);
await smokeSidecar(join(packageRoot, "bin/alfred-codex-sdk-sidecar"));
await copyVerified(licensePath, join(packageRoot, "legal/openai-codex/LICENSE"), false);
await copyVerified(noticePath, join(packageRoot, "legal/openai-codex/NOTICE"), false);

const sbom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: "urn:uuid:7d1c2e3a-9b44-4f0d-8c11-4b8f0d6a1e22",
  version: 1,
  metadata: {
    component: {
      type: "application",
      name: "alfred-codex-sdk-sidecar",
      version: "0.1.0",
    },
  },
  components: [
    {
      type: "library",
      name: "openai-codex",
      version: "0.147.0",
      purl: "pkg:pypi/openai-codex@0.147.0",
    },
    {
      type: "application",
      name: "openai-codex-cli-bin",
      version: "0.147.0",
      purl: "pkg:pypi/openai-codex-cli-bin@0.147.0",
    },
  ],
};
const sbomBytes = `${JSON.stringify(sbom, null, 2)}\n`;
if (sbomBytes.includes("source-component-expectation-not-a-sealed-target-sbom")) {
  throw new Error("refusing to write a source SBOM as the target package SBOM");
}
await writeFile(join(packageRoot, "legal/sbom.cdx.json"), sbomBytes);
await writeFile(join(versionRoot, "runtime-package.source.json"), await readFile(SOURCE_JSON));

const hashes = {
  sidecar: await fileSha(join(packageRoot, "bin/alfred-codex-sdk-sidecar")),
  cli: await fileSha(join(packageRoot, "libexec/codex")),
  license: await fileSha(join(packageRoot, "legal/openai-codex/LICENSE")),
  notice: await fileSha(join(packageRoot, "legal/openai-codex/NOTICE")),
  sbom: await fileSha(join(packageRoot, "legal/sbom.cdx.json")),
};
await writeFile(join(versionRoot, "local-package-hashes.json"), `${JSON.stringify(hashes, null, 2)}\n`);
console.log({ versionRoot, hashes });


async function smokeSidecar(sidecarPath: string): Promise<void> {
  const home = await mkdtemp(join(tmpdir(), "alfred-codex-sidecar-smoke-"));
  try {
    await new Promise<void>((resolve, reject) => {
      const env = { ...process.env };
      for (const key of [
        "OPENAI_API_KEY",
        "OPENAI_ACCESS_TOKEN",
        "CODEX_ACCESS_TOKEN",
        "CODEX_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_BASE",
        "OPENAI_ORG_ID",
        "OPENAI_PROJECT_ID",
      ]) {
        delete env[key];
      }
      env.CODEX_HOME = home;
      const child = spawn(sidecarPath, [], {
        cwd: home,
        env,
        stdio: ["pipe", "pipe", "pipe"],
      });
      let stdout = "";
      let stderr = "";
      const timer = setTimeout(() => {
        child.kill("SIGKILL");
        reject(new Error(`sidecar did not become ready: ${stderr.slice(-800)}`));
      }, 30_000);
      child.stdout.on("data", (chunk: Buffer) => {
        stdout += chunk.toString("utf8");
        if (stdout.includes('"type":"ready"')) {
          child.stdin.write(
            '{"method":"shutdown","params":{},"protocolVersion":1,"requestId":"supervisor_shutdown"}\n',
          );
        }
      });
      child.stderr.on("data", (chunk: Buffer) => {
        stderr += chunk.toString("utf8");
      });
      child.on("error", (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.on("close", (code) => {
        clearTimeout(timer);
        if (stdout.includes('"type":"ready"') && code === 0) {
          resolve();
          return;
        }
        reject(
          new Error(
            `sidecar smoke failed code=${String(code)} stderr=${stderr.slice(-800)} stdout=${stdout.slice(0, 240)}`,
          ),
        );
      });
    });
  } finally {
    await rm(home, { recursive: true, force: true });
  }
}

async function buildSidecar(): Promise<string> {
  const venv = join(SIDECAR_SRC, ".venv");
  const python = join(venv, "bin", "python");
  await execFileAsync("uv", ["venv", "--clear", "--python", "3.11", venv], { cwd: SIDECAR_SRC });
  await execFileAsync(
    "uv",
    ["pip", "install", "--python", python, "-e", ".", "pyinstaller==6.16.0"],
    { cwd: SIDECAR_SRC },
  );
  const dist = join(SIDECAR_SRC, "dist");
  await execFileAsync("rm", ["-rf", join(SIDECAR_SRC, "build"), dist]);
  await execFileAsync(
    python,
    [
      "-m",
      "PyInstaller",
      "--noconfirm",
      "--clean",
      "--onefile",
      "--name",
      "alfred-codex-sdk-sidecar",
      "--paths",
      "src",
      "--hidden-import",
      "alfred_codex_sidecar",
      "--hidden-import",
      "openai_codex",
      "--copy-metadata",
      "openai-codex",
      "--copy-metadata",
      "openai-codex-cli-bin",
      "src/alfred_codex_sidecar/main.py",
    ],
    { cwd: SIDECAR_SRC },
  );
  const built = join(dist, "alfred-codex-sdk-sidecar");
  const { stat } = await import("node:fs/promises");
  const metadata = await stat(built);
  if (!metadata.isFile()) throw new Error(`${built} is not a file`);
  await chmod(built, 0o755);
  return built;
}

async function findNamedFile(root: string, name: string): Promise<string> {
  const found: string[] = [];
  const { readdir } = await import("node:fs/promises");
  async function walk(directory: string, depth: number): Promise<void> {
    if (depth > 8) return;
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await walk(path, depth + 1);
      else if (entry.isFile() && entry.name === name) found.push(path);
    }
  }
  await walk(root, 0);
  if (found.length !== 1) {
    throw new Error(`${name} was not unique under ${root}: ${found.join(", ")}`);
  }
  return found[0];
}

async function copyVerified(source: string, destination: string, executable: boolean): Promise<void> {
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  if (executable) await chmod(destination, 0o755);
}

async function fileSha(path: string): Promise<string> {
  return sha256Bytes(new Uint8Array(await readFile(path)));
}
