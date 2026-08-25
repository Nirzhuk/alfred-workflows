import { BENEFIT_CLASSES, type BenefitClass } from "./manifest";

export type SandboxTestKeys = Readonly<Record<BenefitClass, string>>;

/**
 * Environment variable that carries each test license key when the operator
 * runs the verifier through a secret runner (`op run --`, `gh secret`, etc.)
 * instead of the ignored local file. Names only ever appear in output; values
 * never do.
 *
 * The supporter class deliberately keeps the previously individual-named slot
 * so an operator's existing secret-runner invocation keeps working; the same
 * legacy-slot pattern holds on the Rust side
 * (`ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID`). See scripts/polar/README.md.
 */
export const SECRET_ENV_VARS: Readonly<Record<BenefitClass, string>> = {
  supporter: "POLAR_TEST_INDIVIDUAL_KEY",
};

export const SECRET_FILE_NAME = "sandbox-secrets.json.local";

export class SandboxSecretsError extends Error {
  constructor(
    /** Redaction-safe explanation. Must never interpolate a key value. */
    public readonly reason: string,
  ) {
    super(`Polar sandbox secret input is unusable: ${reason}`);
    this.name = "SandboxSecretsError";
  }
}

function requiredKey(kind: BenefitClass, value: unknown): string {
  if (typeof value !== "string") {
    throw new SandboxSecretsError(`${kind} is missing`);
  }
  const key = value.trim();
  if (key.length < 8 || key.length > 500) {
    throw new SandboxSecretsError(`${kind} is not a plausible license key`);
  }
  return key;
}

export function parseSandboxTestKeys(value: unknown): SandboxTestKeys {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new SandboxSecretsError("input is not a JSON object");
  }
  const record = value as Record<string, unknown>;
  // "individual" is accepted as an alias of "supporter" so a secrets file
  // written under the retired two-product naming still loads.
  const raw = ["supporter", "individual"]
    .map((name) => record[name])
    .find((candidate) => candidate !== undefined);
  return Object.fromEntries(
    BENEFIT_CLASSES.map((kind) => [kind, requiredKey(kind, raw)]),
  ) as SandboxTestKeys;
}

/**
 * Reads both keys from the secret runner's environment. Returns `null`
 * when none of the variables are set so the caller can fall back to the
 * ignored local file; throws when the operator set only some of them, because
 * a partial secret runner invocation must not silently half-run.
 */
export function readSandboxTestKeysFromEnv(
  env: Record<string, string | undefined> = process.env,
): SandboxTestKeys | null {
  const present = BENEFIT_CLASSES.filter((kind) => {
    const value = env[SECRET_ENV_VARS[kind]];
    return typeof value === "string" && value.trim().length > 0;
  });
  if (present.length === 0) return null;
  if (present.length !== BENEFIT_CLASSES.length) {
    const missing = BENEFIT_CLASSES.filter((kind) => !present.includes(kind))
      .map((kind) => SECRET_ENV_VARS[kind])
      .join(", ");
    throw new SandboxSecretsError(`environment is missing ${missing}`);
  }
  return parseSandboxTestKeys(
    Object.fromEntries(
      BENEFIT_CLASSES.map((kind) => [kind, env[SECRET_ENV_VARS[kind]]]),
    ),
  );
}

function assertIgnoredSecretPath(source: string | URL): void {
  const pathname = source instanceof URL ? source.pathname : source;
  if (!pathname.endsWith(".local")) {
    throw new SandboxSecretsError(
      "secret file must be a git-ignored *.local path",
    );
  }
}

export async function loadSandboxTestKeysFromFile(
  source: string | URL = new URL(`./${SECRET_FILE_NAME}`, import.meta.url),
): Promise<SandboxTestKeys> {
  assertIgnoredSecretPath(source);
  let contents: unknown;
  try {
    contents = await Bun.file(source).json();
  } catch {
    throw new SandboxSecretsError(
      `${SECRET_FILE_NAME} is missing or is not valid JSON`,
    );
  }
  return parseSandboxTestKeys(contents);
}

/** Environment first (secret runner), then the ignored local file. */
export async function loadSandboxTestKeys(
  options: {
    readonly env?: Record<string, string | undefined>;
    readonly file?: string | URL;
  } = {},
): Promise<SandboxTestKeys> {
  return (
    readSandboxTestKeysFromEnv(options.env ?? process.env) ??
    (await loadSandboxTestKeysFromFile(options.file))
  );
}
