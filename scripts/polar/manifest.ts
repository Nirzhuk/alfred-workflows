import {
  matchesPolarLinkRule,
  polarLinkRulesFor,
  type PolarLinkKind,
  polarLinkShapes,
} from "../../src/features/licensing/public-link-rules";

export const POLAR_SANDBOX_API_BASE = "https://sandbox-api.polar.sh";

/**
 * The two products of the approved model (Plan 007): Alfred License, sold to
 * one named user, and Alfred Teams, sold one-time per claimed seat. The
 * retired `desktopAnnual | desktopLifetime | companySeat` shape is rejected
 * rather than mapped, so a manifest written for the old four-product Polar
 * configuration cannot be bound by accident.
 */
export const BENEFIT_CLASSES = ["individual", "teams"] as const;

export type BenefitClass = (typeof BENEFIT_CLASSES)[number];

export type PublicResource = {
  readonly id: string;
  readonly label: string;
};

export type PublicLink = {
  readonly url: string;
  readonly label: string;
};

/**
 * A public link Alfred has no confirmed URL shape for yet. `url: null` records
 * "not available", which is a valid manifest state — the verifier only needs
 * the organization, the two benefit IDs, and the checkout link.
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
   * Alfred License only. Teams is sold on the marketing website, so Alfred has
   * no Teams checkout entry point and nothing here to record for one.
   */
  readonly checkoutLinks: {
    readonly individual: PublicLink;
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
      `${field}.${unknown} is not part of the two-product model (expected only ${allowed.join(", ")})`,
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

function parseResource(field: string, value: unknown): PublicResource {
  requiredRecord(field, value);
  return {
    id: requiredUuid(`${field}.id`, value.id),
    label: requiredLabel(`${field}.label`, value.label),
  };
}

function parseCheckoutLink(field: string, value: unknown): PublicLink {
  requiredRecord(field, value);
  return {
    url: requiredPublicUrl(`${field}.url`, value.url, "checkout"),
    label: requiredLabel(`${field}.label`, value.label),
  };
}

/**
 * The portal stays optional so a manifest can be filled in incrementally:
 * `null` records "not collected yet" rather than blocking every other value.
 * A recorded value is held to the sandbox portal rule like any other link.
 */
function parsePortalLink(field: string, value: unknown): OptionalPublicLink {
  requiredRecord(field, value);
  const label = requiredLabel(`${field}.label`, value.label);
  if (value.url === null || value.url === undefined) {
    return { url: null, label };
  }
  return {
    url: requiredPublicUrl(`${field}.url`, value.url, "portal"),
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
  rejectUnknownKeys("benefits", value.benefits, BENEFIT_CLASSES);
  rejectUnknownKeys("checkoutLinks", value.checkoutLinks, ["individual"]);

  const manifest: SandboxManifest = {
    version: 1,
    environment: "sandbox",
    organizationId: requiredUuid("organizationId", value.organizationId),
    benefits: {
      individual: parseResource(
        "benefits.individual",
        value.benefits.individual,
      ),
      teams: parseResource("benefits.teams", value.benefits.teams),
    },
    checkoutLinks: {
      individual: parseCheckoutLink(
        "checkoutLinks.individual",
        value.checkoutLinks.individual,
      ),
    },
    customerPortal: parsePortalLink("customerPortal", value.customerPortal),
  };

  const identifiers = [
    manifest.organizationId,
    ...BENEFIT_CLASSES.map((kind) => manifest.benefits[kind].id),
  ];
  if (new Set(identifiers).size !== identifiers.length) {
    throw new SandboxManifestError(
      "organizationId and the two benefit IDs must all differ",
    );
  }

  const labels = [
    ...BENEFIT_CLASSES.map((kind) => manifest.benefits[kind].label),
    manifest.checkoutLinks.individual.label,
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
