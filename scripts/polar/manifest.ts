import {
  matchesPolarLinkRule,
  polarLinkRulesFor,
  type PolarLinkKind,
  polarLinkShapes,
} from "../../src/features/licensing/public-link-rules";

export const POLAR_SANDBOX_API_BASE = "https://sandbox-api.polar.sh";

/**
 * The single product of the settled supporter model (2026-08): Alfred
 * Supporter, a one-time purchase whose licence keys never expire. The retired
 * shapes — the two-product `individual | teams` model and the older
 * four-product `desktopAnnual | desktopLifetime | companySeat` one — are
 * rejected rather than mapped, so a manifest written for either old Polar
 * configuration cannot be bound by accident.
 */
export const BENEFIT_CLASSES = ["supporter"] as const;

/**
 * The license-key expiration Polar applies per benefit
 * (`BenefitLicenseKeyExpirationProperties`: `{"ttl": N, "timeframe":
 * "year" | "month" | "day"}`). Supporter licences are PERPETUAL (model
 * settled 2026-08, superseding Plan 007's one-year rule for this product), so
 * a correct Polar configuration records NO expiration at all.
 *
 * Parsing is structural only: it validates the shape of an explicitly
 * recorded expiry so a typo cannot slip through silently. Whether a recorded
 * value is allowed at all is the verifier's decision — any non-null ttl or
 * timeframe fails verification with "supporter licences are perpetual".
 */
export type BenefitExpiry = {
  readonly ttl: number | null;
  readonly timeframe: "year" | "month" | "day" | null;
};

export const EXPIRY_TIMEFRAMES = ["year", "month", "day"] as const;

export type BenefitClass = (typeof BENEFIT_CLASSES)[number];

export type PublicResource = {
  readonly id: string;
  readonly label: string;
  readonly expiry: BenefitExpiry;
};

/**
 * Benefit classes of the retired models — the two-product `individual` /
 * `teams` shape superseded by the supporter licence, plus the even older
 * four-product names. Their reappearance in a manifest is a configuration
 * error against the wrong plan, not an unknown field, so they get their own
 * rejection reason.
 */
export const LEGACY_BENEFIT_CLASSES = [
  "individual",
  "teams",
  "desktopAnnual",
  "desktopLifetime",
  "companySeat",
] as const;


/**
 * A public link Alfred has no confirmed URL for yet. `url: null` records
 * "not collected yet", which is a valid manifest state — the verifier only
 * needs the organization and the supporter benefit ID before it can start,
 * and fails fast on a still-null checkout link instead of parsing nothing.
 */

export type OptionalPublicLink = {
  readonly url: string | null;
  readonly label: string;
};

export type SandboxManifest = {
  readonly version: 1;
  readonly environment: "sandbox";
  readonly organizationId: string;
  readonly benefits: Readonly<Record<BenefitClass, PublicResource>>;
  /**
   * Alfred Supporter only. `url: null` records "not collected yet" until the
   * operator creates the checkout link in the Polar dashboard.
   */
  readonly checkoutLinks: {
    readonly supporter: OptionalPublicLink;
  };
  readonly customerPortal: OptionalPublicLink;
};

export class SandboxManifestError extends Error {
  constructor(
    /** Redaction-safe explanation naming the field, never its value. */
    public readonly reason: string,
  ) {
    super(`Polar sandbox manifest is unusable: ${reason}`);
    this.name = "SandboxManifestError";
  }
}

const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requiredRecord(
  field: string,
  value: unknown,
): asserts value is Record<string, unknown> {
  if (!isRecord(value)) throw new SandboxManifestError(`${field} is missing`);
}

/**
 * An unexpected member is a configuration error, not something to ignore. A
 * retired `benefits.companySeat` or an invented `checkoutLinks.teams` would
 * otherwise sit in the file looking bound while nothing ever read it.
 */
function rejectUnknownKeys(
  field: string,
  value: Record<string, unknown>,
  allowed: readonly string[],
): void {
  const unknown = Object.keys(value).find((key) => !allowed.includes(key));
  if (unknown !== undefined) {
    throw new SandboxManifestError(
      `${field}.${unknown} is not part of the supporter model (expected only ${allowed.join(", ")})`,
    );
  }
}

function requiredUuid(field: string, value: unknown): string {
  if (typeof value !== "string" || !UUID_V4.test(value)) {
    throw new SandboxManifestError(`${field} is not a UUID v4`);
  }
  return value.toLowerCase();
}

function requiredLabel(field: string, value: unknown): string {
  if (typeof value !== "string") {
    throw new SandboxManifestError(`${field} is missing`);
  }
  const label = value.trim();
  if (label.length === 0 || label.length > 80) {
    throw new SandboxManifestError(`${field} must be 1-80 characters`);
  }
  return label;
}

// This manifest describes a SANDBOX organization, so it is checked against the
// sandbox half of the shared allow-list in
// `src/features/licensing/public-link-rules.ts` — the same rules the frontend
// opener uses and the Tauri `opener:allow-open-url` scope mirrors. A production
// link recorded here would be accepted by neither, so it is rejected up front
// during configuration rather than silently refused at runtime.
function requiredPublicUrl(
  field: string,
  value: unknown,
  kind: PolarLinkKind,
): string {
  if (typeof value !== "string") {
    throw new SandboxManifestError(`${field} is missing`);
  }

  const rules = polarLinkRulesFor("sandbox", kind);

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new SandboxManifestError(`${field} is not a URL`);
  }
  if (!matchesPolarLinkRule(url, rules)) {
    throw new SandboxManifestError(
      `${field} must be exactly ${polarLinkShapes("sandbox", kind)}`,
    );
  }
  return url.href;
}
/**
 * `null` (or an absent member) records "no expiration configured", which is
 * the required state under the perpetual-supporter model. A recorded ttl is
 * a positive integer; a recorded timeframe is one of Polar's enum values —
 * both parse fine here and then fail verification.
 */
function parseBenefitExpiry(field: string, value: unknown): BenefitExpiry {
  if (value === null || value === undefined) {
    return { ttl: null, timeframe: null };
  }
  requiredRecord(field, value);
  rejectUnknownKeys(field, value, ["ttl", "timeframe"]);

  let ttl: number | null = null;
  if (value.ttl !== null && value.ttl !== undefined) {
    if (
      typeof value.ttl !== "number" ||
      !Number.isInteger(value.ttl) ||
      value.ttl < 1
    ) {
      throw new SandboxManifestError(
        `${field}.ttl must be null (not yet recorded) or an integer >= 1`,
      );
    }
    ttl = value.ttl;
  }

  let timeframe: BenefitExpiry["timeframe"] = null;
  if (value.timeframe !== null && value.timeframe !== undefined) {
    if (
      typeof value.timeframe !== "string" ||
      !(EXPIRY_TIMEFRAMES as readonly string[]).includes(value.timeframe)
    ) {
      throw new SandboxManifestError(
        `${field}.timeframe must be null (not yet recorded) or one of ${EXPIRY_TIMEFRAMES.join(", ")}`,
      );
    }
    timeframe = value.timeframe as BenefitExpiry["timeframe"];
  }

  return { ttl, timeframe };
}

function parseResource(field: string, value: unknown): PublicResource {
  requiredRecord(field, value);
  return {
    id: requiredUuid(`${field}.id`, value.id),
    label: requiredLabel(`${field}.label`, value.label),
    expiry: parseBenefitExpiry(`${field}.expiry`, value.expiry),
  };
}

/**
 * An optional public link: `null` records "not collected yet" so a manifest
 * can be filled in incrementally rather than blocking every other value. A
 * recorded value is held to the sandbox rule for its kind like any other
 * link; a still-null checkout link fails the verifier later, pre-network.
 */
function parseOptionalLink(
  field: string,
  value: unknown,
  kind: PolarLinkKind,
): OptionalPublicLink {
  requiredRecord(field, value);
  const label = requiredLabel(`${field}.label`, value.label);
  if (value.url === null || value.url === undefined) {
    return { url: null, label };
  }
  return {
    url: requiredPublicUrl(`${field}.url`, value.url, kind),
    label,
  };
}

export function parseSandboxManifest(value: unknown): SandboxManifest {
  requiredRecord("manifest", value);
  if (value.version !== 1) {
    throw new SandboxManifestError("version must be 1");
  }
  if (value.environment !== "sandbox") {
    throw new SandboxManifestError('environment must be "sandbox"');
  }

  requiredRecord("benefits", value.benefits);
  requiredRecord("checkoutLinks", value.checkoutLinks);
  // A retired class name is a configuration error against the wrong plan, not
  // a mere unknown key, so it is named as such before the generic check runs.
  const legacyClass = Object.keys(value.benefits).find((key) =>
    (LEGACY_BENEFIT_CLASSES as readonly string[]).includes(key),
  );
  if (legacyClass !== undefined) {
    throw new SandboxManifestError(
      `benefits.${legacyClass} is a retired benefit class (${LEGACY_BENEFIT_CLASSES.join(", ")}) — the supporter model has exactly one benefit (${BENEFIT_CLASSES.join(", ")})`,
    );
  }
  rejectUnknownKeys("benefits", value.benefits, BENEFIT_CLASSES);
  rejectUnknownKeys("checkoutLinks", value.checkoutLinks, BENEFIT_CLASSES);

  const manifest: SandboxManifest = {
    version: 1,
    environment: "sandbox",
    organizationId: requiredUuid("organizationId", value.organizationId),
    benefits: {
      supporter: parseResource("benefits.supporter", value.benefits.supporter),
    },
    checkoutLinks: {
      supporter: parseOptionalLink(
        "checkoutLinks.supporter",
        value.checkoutLinks.supporter,
        "checkout",
      ),
    },
    customerPortal: parseOptionalLink(
      "customerPortal",
      value.customerPortal,
      "portal",
    ),
  };

  const identifiers = [
    manifest.organizationId,
    ...BENEFIT_CLASSES.map((kind) => manifest.benefits[kind].id),
  ];
  if (new Set(identifiers).size !== identifiers.length) {
    throw new SandboxManifestError(
      "organizationId and the benefit ID must differ",
    );
  }

  const labels = [
    ...BENEFIT_CLASSES.map((kind) => manifest.benefits[kind].label),
    manifest.checkoutLinks.supporter.label,
    manifest.customerPortal.label,
  ];
  if (new Set(labels).size !== labels.length) {
    throw new SandboxManifestError("every label must be unique");
  }

  return manifest;
}

export async function loadSandboxManifest(
  source: string | URL = new URL("./sandbox-manifest.json", import.meta.url),
): Promise<SandboxManifest> {
  let contents: unknown;
  try {
    contents = await Bun.file(source).json();
  } catch {
    throw new SandboxManifestError(
      "sandbox-manifest.json is missing or is not valid JSON",
    );
  }
  return parseSandboxManifest(contents);
}
