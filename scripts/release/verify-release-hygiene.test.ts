import { describe, expect, test } from "bun:test";

import {
  collect,
  scanArchitecture,
  scanSecrets,
  scanUpdater,
} from "./verify-release-hygiene";

const file = (path: string, text: string) => ({ path, text });

describe("architecture scan", () => {
  test("flags every obsolete commerce and update reference with its line", () => {
    const violations = scanArchitecture([
      file("docs/releasing.md", "intro\npaid through Stripe today\nend"),
      file("README.md", "assets live in CrabNebula\n"),
      file("docs/install.md", "Alfred ships manual Polar downloads\n"),
    ]);
    expect(violations.map((violation) => `${violation.file}:${violation.line}`)).toEqual([
      "docs/releasing.md:2",
      "README.md:1",
    ]);
  });

  test("passes a clean set of surfaces", () => {
    expect(scanArchitecture([file("README.md", "Polar owns checkout.\n")])).toEqual([]);
  });
});

describe("secret scan", () => {
  test("rejects a Polar server credential anywhere in source", () => {
    const violations = scanSecrets([
      file("src-tauri/src/lib.rs", 'let token = env("POLAR_ACCESS_TOKEN");'),
      file("src/config.ts", "export const polarWebhookSecret = process.env.X;"),
    ]);
    expect(violations).toHaveLength(2);
  });

  test("allows key handling only in the reviewed ephemeral set", () => {
    expect(
      scanSecrets([file("src-tauri/src/licensing/service.rs", "let license_key = key;")]),
    ).toEqual([]);
    expect(
      scanSecrets([file("src/features/history/panel.tsx", "const licenseKey = value;")]),
    ).toEqual([
      {
        file: "src/features/history/panel.tsx",
        line: 1,
        detail:
          "handles a license key outside the reviewed ephemeral/keychain-only set",
      },
    ]);
  });
});

describe("updater scan", () => {
  test("accepts the shipped manual-download configuration", () => {
    expect(
      scanUpdater([file(".github/workflows/release.yml", "          uploadUpdaterJson: false\n")]),
    ).toEqual([]);
  });

  test("ignores prose that only names the setting", () => {
    expect(
      scanUpdater([
        file(
          ".github/workflows/release.yml",
          "# so `uploadUpdaterJson` stays false and no updater plugin exists\n          uploadUpdaterJson: false\n",
        ),
      ]),
    ).toEqual([]);
  });

  test("rejects an enabled updater manifest, artifact, or plugin", () => {
    expect(
      scanUpdater([file(".github/workflows/release.yml", "uploadUpdaterJson: true\n")]),
    ).toHaveLength(2);
    expect(
      scanUpdater([
        file(".github/workflows/release.yml", "uploadUpdaterJson: false\ncreateUpdaterArtifacts: true\n"),
      ]),
    ).toHaveLength(1);
    expect(
      scanUpdater([
        file(".github/workflows/release.yml", "uploadUpdaterJson: false\n"),
        file("src-tauri/tauri.conf.json", '{ "plugins": { "updater": { "endpoints": [] } } }'),
      ]),
    ).toHaveLength(1);
    expect(
      scanUpdater([
        file(".github/workflows/release.yml", "uploadUpdaterJson: false\n"),
        file("src-tauri/Cargo.toml", 'tauri-plugin-updater = "2"\n'),
      ]),
    ).toHaveLength(1);
  });

  test("rejects a workflow that dropped the declaration entirely", () => {
    expect(scanUpdater([file(".github/workflows/release.yml", "name: release\n")])).toEqual([
      {
        file: ".github/workflows",
        line: 1,
        detail: "no `uploadUpdaterJson: false` declaration found",
      },
    ]);
  });
});

test("collect reads a named file and skips build output", async () => {
  const files = await collect(["package.json"]);
  expect(files.map((entry) => entry.path)).toEqual(["package.json"]);
  expect(files[0]?.text).toContain('"name": "alfred"');
});
