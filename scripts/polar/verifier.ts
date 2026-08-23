import {
  BENEFIT_CLASSES,
  type BenefitClass,
  type SandboxManifest,
} from "./manifest";
import type { SandboxTestKeys } from "./secrets";
import {
  type ActivationRead,
  PolarPublicEndpointError,
  PolarPublicLicenseClient,
} from "./client";

export type VerificationReporter = (
  passed: boolean,
  caseName: string,
  /**
   * Non-secret field mismatches explaining a failure, so the operator can fix
   * the Polar configuration without guessing. Never contains a license key or
   * activation ID.
   */
  detail?: readonly string[],
) => void;

export type VerificationResult = {
  readonly passed: boolean;
  readonly caseNames: readonly string[];
};

const ONE_WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * Under the two-product model EVERY Alfred key carries a one-year expiry: the
 * update window is enforced by comparing the build's `ALFRED_RELEASE_DATE`
 * against this deadline (Plan 007). A key issued with no expiry, or with one
 * dated years out, means the Polar product is misconfigured — the opposite of
 * the retired rule, which required the lifetime key to have no expiry at all.
 *
 * The lower bound is only "still in the future". A test key issued months ago
 * legitimately expires in less than a year, and the license read carries no
 * issue date to measure the window from, so an exact one-year assertion would
 * fail correct configurations. A week of slack above one year absorbs clock
 * skew and Polar's own rounding.
 */
export function isOneYearExpiry(
  expiresAt: string | null,
  now: Date = new Date(),
): boolean {
  if (expiresAt === null) return false;
  const deadline = Date.parse(expiresAt);
  if (Number.isNaN(deadline)) return false;
  const latest = new Date(now);
  latest.setUTCFullYear(latest.getUTCFullYear() + 1);
  return deadline > now.getTime() && deadline <= latest.getTime() + ONE_WEEK_MS;
}

/**
 * Names every mismatched field rather than collapsing five distinct checks
 * into one opaque failure. An operator seeing `FAIL individual.activate-1`
 * with no detail cannot tell a wrong benefit ID from a missing expiry.
 *
 * Only non-secret configuration is reported: organization and benefit IDs are
 * public by design, and status/limit/expiry are product configuration. The
 * license key and activation ID are never included.
 */
export function licenseMismatches(
  manifest: SandboxManifest,
  kind: BenefitClass,
  license: ActivationRead["license_key"],
): string[] {
  const problems: string[] = [];
  const organization = license.organization_id.toLowerCase();
  if (organization !== manifest.organizationId) {
    problems.push(
      `organization_id is ${organization}, manifest expects ${manifest.organizationId}`,
    );
  }
  const benefit = license.benefit_id.toLowerCase();
  if (benefit !== manifest.benefits[kind].id) {
    problems.push(
      `benefit_id is ${benefit}, manifest expects ${manifest.benefits[kind].id} for benefits.${kind}.id`,
    );
  }
  if (license.status !== "granted") {
    problems.push(`status is ${license.status}, expected granted`);
  }
  if (license.limit_activations !== 3) {
    problems.push(
      `limit_activations is ${String(license.limit_activations)}, expected 3 — set the benefit's activation limit to 3 in Polar`,
    );
  }
  if (!isOneYearExpiry(license.expires_at)) {
    problems.push(
      license.expires_at === null
        ? "expires_at is null — every key must carry a one-year expiry; set the benefit's expiration to 1 year in Polar"
        : `expires_at is ${license.expires_at}, which is not within one year from now`,
    );
  }
  return problems;
}

function assertLicense(
  manifest: SandboxManifest,
  kind: BenefitClass,
  license: ActivationRead["license_key"],
): void {
  const problems = licenseMismatches(manifest, kind, license);
  if (problems.length > 0) throw new PolarPublicEndpointError(problems);
}

async function verifyClass(
  manifest: SandboxManifest,
  keys: SandboxTestKeys,
  kind: BenefitClass,
  client: PolarPublicLicenseClient,
  report: VerificationReporter,
  caseNames: string[],
): Promise<boolean> {
  const key = keys[kind];
  const active = new Set<string>();
  let passed = true;

  const stage = async <T>(caseName: string, action: () => Promise<T>) => {
    caseNames.push(caseName);
    try {
      const result = await action();
      report(true, caseName);
      return result;
    } catch (error) {
      passed = false;
      // Preserve why it failed. A bare rethrow here was discarding the exact
      // field mismatch the operator needs to fix their Polar configuration.
      const failure =
        error instanceof PolarPublicEndpointError
          ? error
          : new PolarPublicEndpointError();
      report(false, caseName, failure.detail);
      throw failure;
    }
  };

  try {
    const activations: ActivationRead[] = [];
    for (let index = 1; index <= 3; index += 1) {
      const activation = await stage(`${kind}.activate-${index}`, async () => {
        const expectedLabel = `${manifest.benefits[kind].label} device ${index}`;
        const value = await client.activate(
          key,
          expectedLabel,
        );
        active.add(value.id);
        if (value.label !== expectedLabel) throw new PolarPublicEndpointError();
        assertLicense(manifest, kind, value.license_key);
        return value;
      });
      activations.push(activation);

      await stage(`${kind}.validate-${index}`, async () => {
        const license = await client.validate(key, activation.id);
        assertLicense(manifest, kind, license);
      });
    }

    await stage(`${kind}.fourth-activation-rejected`, async () => {
      const attempt = await client.attemptLimitedActivation(
        key,
        `${manifest.benefits[kind].label} device 4`,
      );
      if (!attempt.limited) {
        active.add(attempt.activation.id);
        throw new PolarPublicEndpointError();
      }
    });

    await stage(`${kind}.deactivate-first`, async () => {
      await client.deactivate(key, activations[0].id);
      active.delete(activations[0].id);
    });

    await stage(`${kind}.replacement-activation`, async () => {
      const expectedLabel = `${manifest.benefits[kind].label} replacement`;
      const replacement = await client.activate(key, expectedLabel);
      active.add(replacement.id);
      if (replacement.label !== expectedLabel) {
        throw new PolarPublicEndpointError();
      }
      assertLicense(manifest, kind, replacement.license_key);
      const license = await client.validate(key, replacement.id);
      assertLicense(manifest, kind, license);
    });
  } catch {
    passed = false;
  } finally {
    const cleanupCase = `${kind}.cleanup`;
    caseNames.push(cleanupCase);
    let cleanupPassed = true;
    for (const activationId of active) {
      try {
        await client.deactivate(key, activationId);
      } catch {
        cleanupPassed = false;
      }
    }
    if (cleanupPassed) {
      active.clear();
      report(true, cleanupCase);
    } else {
      passed = false;
      report(false, cleanupCase);
    }
  }

  return passed;
}

export async function verifyPolarSandbox(options: {
  readonly manifest: SandboxManifest;
  readonly keys: SandboxTestKeys;
  readonly fetcher?: typeof fetch;
  readonly report: VerificationReporter;
}): Promise<VerificationResult> {
  const client = new PolarPublicLicenseClient(
    options.manifest.organizationId,
    options.fetcher,
  );
  const caseNames: string[] = [];
  let passed = true;

  for (const kind of BENEFIT_CLASSES) {
    if (
      !(await verifyClass(
        options.manifest,
        options.keys,
        kind,
        client,
        options.report,
        caseNames,
      ))
    ) {
      passed = false;
    }
  }

  return { passed, caseNames };
}
