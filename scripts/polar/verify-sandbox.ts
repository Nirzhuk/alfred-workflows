import { loadSandboxManifest, SandboxManifestError } from "./manifest";
import {
  loadSandboxTestKeys,
  SandboxSecretsError,
  SECRET_ENV_VARS,
  SECRET_FILE_NAME,
} from "./secrets";
import { verifyPolarSandbox, type VerificationReporter } from "./verifier";

type Line = (text: string) => void;

function printResult(
  passed: boolean,
  caseName: string,
  detail?: readonly string[],
): void {
  console.log(`${passed ? "PASS" : "FAIL"} ${caseName}`);
  for (const line of detail ?? []) console.log(`  ${line}`);
}

const MANIFEST_HELP = [
  "  Fill every null in scripts/polar/sandbox-manifest.json with the PUBLIC",
  "  sandbox values: the organization ID, the Alfred License and Alfred Teams",
  "  benefit IDs, and the Alfred License sandbox checkout link shaped",
  "  https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_.../redirect.",
  "  customerPortal.url is https://sandbox.polar.sh/<org-slug>/portal, or null",
  "  if it has not been collected yet. Both benefit IDs are required and must",
  "  come from the CURRENT Polar products: the previously bound IDs are stale.",
  "  Teams is sold on the marketing website, so there is no Teams checkout.",
  "  See docs/polar-operator-handoff.md.",
];

const SECRETS_HELP = [
  "  Supply both sandbox TEST license keys one of two ways:",
  `    1. a secret runner exporting ${Object.values(SECRET_ENV_VARS).join(", ")}`,
  `    2. the git-ignored file scripts/polar/${SECRET_FILE_NAME}, a JSON`,
  '       object with "individual" and "teams".',
  "  Never pass a key as a command-line argument, and never commit one.",
];

/**
 * Configuration failures print the field that is wrong plus how to fix it.
 * `reason` on both error types is a field name, never a value, so this stays
 * inside the plan's redaction rule.
 */
function reportInputFailure(error: unknown, write: Line): void {
  if (error instanceof SandboxManifestError) {
    write(`FAIL verifier-input.manifest (${error.reason})`);
    for (const line of MANIFEST_HELP) write(line);
    return;
  }
  if (error instanceof SandboxSecretsError) {
    write(`FAIL verifier-input.secrets (${error.reason})`);
    for (const line of SECRETS_HELP) write(line);
    return;
  }
  write("FAIL verifier-input.unexpected");
}

export async function runSandboxVerifier(options: {
  readonly argv?: readonly string[];
  readonly report?: VerificationReporter;
  readonly write?: Line;
  /**
   * Loader seams so tests never read an operator's real
   * sandbox-secrets.json.local and never reach Polar's network. Production
   * callers omit both.
   */
  readonly loadManifest?: typeof loadSandboxManifest;
  readonly loadKeys?: typeof loadSandboxTestKeys;
} = {}): Promise<boolean> {
  const report = options.report ?? printResult;
  const write = options.write ?? ((text: string) => console.log(text));
  const argv = options.argv ?? process.argv.slice(2);

  // A license key on the command line lands in shell history and process
  // listings, so refuse to run at all rather than guess what was passed.
  if (argv.length > 0) {
    write("FAIL verifier-input.arguments (this command takes no arguments)");
    for (const line of SECRETS_HELP) write(line);
    return false;
  }

  let manifest;
  let keys;
  try {
    manifest = await (options.loadManifest ?? loadSandboxManifest)();
    keys = await (options.loadKeys ?? loadSandboxTestKeys)();
  } catch (error) {
    reportInputFailure(error, write);
    return false;
  }
  report(true, "verifier-input");

  const result = await verifyPolarSandbox({ manifest, keys, report });
  return result.passed;
}

if (import.meta.main) {
  process.exitCode = (await runSandboxVerifier()) ? 0 : 1;
}
