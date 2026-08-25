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


/**
 * Supporter licences are PERPETUAL (model settled 2026-08, superseding the
 * one-year rule of Plan 007): the benefit is configured WITHOUT a
 * license-key expiration, and the manifest records none. A recorded ttl or
 * timeframe means the Polar product is misconfigured against that model.
 * Reasons name the offending field so the operator knows exactly which
 * dashboard setting to remove.
 */
export function expiryConfigMismatches(manifest: SandboxManifest): string[] {
  const problems: string[] = [];
  for (const kind of BENEFIT_CLASSES) {
    const { ttl, timeframe } = manifest.benefits[kind].expiry;
    // A recorded ttl below 1 cannot reach here: parsing already rejected it.
    if (ttl !== null) {
      problems.push(
        `benefits.${kind}.expiry.ttl is ${ttl} — supporter licences are perpetual; remove the license-key expiration from this benefit in Polar`,
      );
    }
    if (timeframe !== null) {
      problems.push(
        `benefits.${kind}.expiry.timeframe is "${timeframe}" — supporter licences are perpetual; remove the license-key expiration from this benefit in Polar`,
      );
    }
  }
  return problems;
}

/**
 * Names every mismatched field rather than collapsing five distinct checks
 * into one opaque failure. An operator seeing `FAIL supporter.activate-1`
 * with no detail cannot tell a wrong benefit ID from an expiry Polar should
 * not have issued at all.
 *
 * Only non-secret configuration is reported: organization and benefit IDs are
 * public by design, and status/limit/expiry are product configuration. The
 * license key and activation ID are never included.
 */
export function licenseMismatches(
  manifest: SandboxManifest,
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
  if (benefit !== manifest.benefits.supporter.id) {
    problems.push(
      `benefit_id is ${benefit}, manifest expects ${manifest.benefits.supporter.id} for benefits.supporter.id`,
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
  if (license.expires_at !== null) {
    problems.push(
      `expires_at is ${license.expires_at} — supporter licences are perpetual; remove the benefit's license-key expiration in Polar`,
    );
  }
  return problems;
}

function assertLicense(
  manifest: SandboxManifest,
  license: ActivationRead["license_key"],
): void {
  const problems = licenseMismatches(manifest, license);
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
        assertLicense(manifest, value.license_key);
        return value;
      });
      activations.push(activation);

      await stage(`${kind}.validate-${index}`, async () => {
        const license = await client.validate(key, activation.id);
        assertLicense(manifest, license);
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
      assertLicense(manifest, replacement.license_key);
      const license = await client.validate(key, replacement.id);
      assertLicense(manifest, license);
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
  // No live key can compensate for a misconfigured benefit, so a recorded
  // expiration on it fails before any network call is made.
  const configProblems = expiryConfigMismatches(options.manifest);
  if (configProblems.length > 0) {
    const caseName = "manifest.expiry";
    caseNames.push(caseName);
    options.report(false, caseName, configProblems);
    return { passed: false, caseNames };
  }
  // Same fail-closed principle for a checkout link that has not been created
  // in the Polar dashboard yet: nothing downstream could succeed, so name the
  // missing field instead of half-running against live endpoints.
  for (const kind of BENEFIT_CLASSES) {
    if (options.manifest.checkoutLinks[kind].url === null) {
      const caseName = `manifest.checkout.${kind}`;
      caseNames.push(caseName);
      options.report(false, caseName, [
        "collect the checkout link from the Polar dashboard",
      ]);
      return { passed: false, caseNames };
    }
  }
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
