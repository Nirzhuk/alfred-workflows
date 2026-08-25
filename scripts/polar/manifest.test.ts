import { describe, expect, test } from "bun:test";
import {
  parseSandboxManifest,
  SandboxManifestError,
} from "./manifest";

const SANDBOX_CHECKOUT =
  "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect";

function validManifest(): Record<string, unknown> {
  return {
    version: 1,
    environment: "sandbox",
    organizationId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    benefits: {
      supporter: {
        id: "11111111-1111-4111-8111-111111111111",
        label: "Alfred Supporter",
      },
    },
    checkoutLinks: {
      supporter: {
        url: SANDBOX_CHECKOUT,
        label: "Alfred Supporter Checkout",
      },
    },
    customerPortal: {
      url: null,
      label: "Alfred Sandbox Customer Portal",
    },
  };
}

describe("sandbox manifest", () => {
  test("accepts a complete single-supporter manifest", () => {
    // The supporter model has exactly one benefit; a full-pass fixture carries
    // it plus a collected sandbox checkout link.
    const manifest = parseSandboxManifest(validManifest());

    expect(manifest.environment).toBe("sandbox");
    expect(manifest.benefits.supporter.id).toBe(
      "11111111-1111-4111-8111-111111111111",
    );
    expect(manifest.checkoutLinks.supporter.url).toBe(SANDBOX_CHECKOUT);
    // A null portal is a valid recorded state, not a missing field.
    expect(manifest.customerPortal.url).toBeNull();
  });

  test("accepts a checkout link that has not been collected yet", () => {
    // `url: null` records "not collected yet" so a half-filled manifest still
    // parses; the verifier fails fast on it later, pre-network.
    const uncollected = validManifest();
    (
      (uncollected.checkoutLinks as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).url = null;
    const manifest = parseSandboxManifest(uncollected);
    expect(manifest.checkoutLinks.supporter.url).toBeNull();
  });

  test("accepts the per-organization sandbox portal, never a global path", () => {
    // Confirmed live: sandbox.polar.sh/<org-slug>/portal returns 200, while
    // /purchases 404s on both the sandbox and production hosts.
    const good = validManifest();
    (good.customerPortal as Record<string, unknown>).url =
      "https://sandbox.polar.sh/alfred/portal";
    expect(parseSandboxManifest(good).customerPortal.url).toBe(
      "https://sandbox.polar.sh/alfred/portal",
    );

    const rejected = [
      "https://sandbox.polar.sh/purchases",
      "https://polar.sh/purchases",
      // Production host in a sandbox manifest.
      "https://polar.sh/alfred/portal",
      "https://sandbox-api.polar.sh/alfred/portal",
      // The slug is exactly one segment.
      "https://sandbox.polar.sh/portal",
      "https://sandbox.polar.sh/alfred/portal/request",
      "https://sandbox.polar.sh/a/b/portal",
      // Credential-bearing.
      "https://sandbox.polar.sh/alfred/portal?customer_session_token=redacted",
    ];
    for (const url of rejected) {
      const manifest = validManifest();
      (manifest.customerPortal as Record<string, unknown>).url = url;
      expect(() => parseSandboxManifest(manifest)).toThrow(
        SandboxManifestError,
      );
    }
  });

  test("fails closed for unconfigured, production, secret-bearing, or duplicate values", () => {
    const unconfigured = validManifest();
    unconfigured.organizationId = null;
    expect(() => parseSandboxManifest(unconfigured)).toThrow(
      SandboxManifestError,
    );

    const unboundBenefit = validManifest();
    (
      (unboundBenefit.benefits as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).id = null;
    expect(() => parseSandboxManifest(unboundBenefit)).toThrow(
      SandboxManifestError,
    );

    const production = validManifest();
    production.environment = "production";
    expect(() => parseSandboxManifest(production)).toThrow(
      SandboxManifestError,
    );

    const secretBearingLink = validManifest();
    (
      (secretBearingLink.checkoutLinks as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).url =
      "https://token@sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect";
    expect(() => parseSandboxManifest(secretBearingLink)).toThrow(
      SandboxManifestError,
    );

    // With one benefit, the only possible identifier collision is against the
    // organization itself.
    const duplicate = validManifest();
    (
      (duplicate.benefits as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let reason = "";
    try {
      parseSandboxManifest(duplicate);
    } catch (error) {
      reason = (error as SandboxManifestError).reason;
    }
    expect(reason).toContain("organizationId and the benefit ID must differ");
  });

  test("accepts only the sandbox link shape the desktop build may open", () => {
    // src/features/licensing/public-link-rules.ts is the one allow-list the
    // frontend opener and the Tauri opener:allow-open-url scope both follow. A
    // manifest value outside it would be recorded, bound, and then silently
    // refused at runtime.
    const rejectedCheckouts = [
      // Production shapes: a sandbox manifest must never carry a live link.
      "https://buy.polar.sh/polar_cl_company",
      "https://polar.sh/purchases",
      // Wrong host, right path.
      "https://sandbox.polar.sh/v1/checkout-links/polar_cl_company/redirect",
      "https://api.polar.sh/v1/checkout-links/polar_cl_company/redirect",
      "https://sandbox-api.polar.sh.attacker.invalid/v1/checkout-links/polar_cl_company/redirect",
      // Right host, wrong path.
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_company",
      "https://sandbox-api.polar.sh/polar_cl_company",
      "https://sandbox-api.polar.sh/v1/checkout-links/other_company/redirect",
      "https://sandbox.polar.sh/checkout/company",
    ];
    for (const url of rejectedCheckouts) {
      const manifest = validManifest();
      (
        (manifest.checkoutLinks as Record<string, unknown>)
          .supporter as Record<string, unknown>
      ).url = url;
      expect(() => parseSandboxManifest(manifest)).toThrow(
        SandboxManifestError,
      );
    }
  });

  test("keeps every URL safety check on the sandbox checkout shape", () => {
    // https only, no username/password, no port, no query string, no
    // fragment. A URL carrying a customer_session_token — or any query
    // parameter — is a credential, not a public link.
    const unsafe = [
      "http://sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect",
      "https://user@sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect",
      "https://user:pass@sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect",
      "https://sandbox-api.polar.sh:444/v1/checkout-links/polar_cl_company/redirect",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect?customer_session_token=redacted",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect?redirect=https://example.com",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_company/redirect#token",
    ];
    for (const url of unsafe) {
      const manifest = validManifest();
      (
        (manifest.checkoutLinks as Record<string, unknown>)
          .supporter as Record<string, unknown>
      ).url = url;
      expect(() => parseSandboxManifest(manifest)).toThrow(
        SandboxManifestError,
      );
    }
  });

  test("explains which field is wrong without echoing its value", () => {
    const manifest = validManifest();
    (
      (manifest.checkoutLinks as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).url =
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect?customer_session_token=super-secret";

    let reason = "";
    try {
      parseSandboxManifest(manifest);
    } catch (error) {
      reason = (error as SandboxManifestError).reason;
    }
    expect(reason).toContain("checkoutLinks.supporter.url");
    expect(reason).not.toContain("super-secret");
  });

  test("rejects arbitrary public-link destinations", () => {
    const manifest = validManifest();
    (
      (manifest.checkoutLinks as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).url = "https://example.com/checkout";
    expect(() => parseSandboxManifest(manifest)).toThrow(
      SandboxManifestError,
    );
  });

  test("rejects the retired individual and teams classes by name", () => {
    // The owner collapsed to a single supporter product on 2026-08-25. A
    // manifest still carrying either two-product class name is an error
    // against the wrong plan and gets its own rejection reason, not a generic
    // unknown-key error.
    for (const retiredClass of ["individual", "teams"]) {
      const retired = validManifest();
      (retired.benefits as Record<string, unknown>)[retiredClass] = {
        id: "33333333-3333-4333-8333-333333333333",
        label: `Alfred Sandbox ${retiredClass}`,
      };

      let reason = "";
      try {
        parseSandboxManifest(retired);
      } catch (error) {
        reason = (error as SandboxManifestError).reason;
      }
      expect(reason).toContain(`benefits.${retiredClass}`);
      expect(reason).toContain("retired");
      expect(reason).toContain("exactly one benefit");
    }

    // The same applies to checkout links invented under the old naming.
    const retiredCheckout = validManifest();
    (retiredCheckout.checkoutLinks as Record<string, unknown>).individual = {
      url: SANDBOX_CHECKOUT,
      label: "Alfred Sandbox License Checkout",
    };
    expect(() => parseSandboxManifest(retiredCheckout)).toThrow(
      SandboxManifestError,
    );
  });

  test("rejects every legacy four-product class with a reason naming it", () => {
    // The pre-two-product configuration is likewise refused outright rather
    // than parsed-and-ignored. Each retired name gets its own clear rejection.
    for (const legacyClass of [
      "desktopAnnual",
      "desktopLifetime",
      "companySeat",
    ]) {
      const legacy = validManifest();
      (legacy.benefits as Record<string, unknown>)[legacyClass] = {
        id: "33333333-3333-4333-8333-333333333333",
        label: `Alfred Sandbox ${legacyClass}`,
      };

      let reason = "";
      try {
        parseSandboxManifest(legacy);
      } catch (error) {
        reason = (error as SandboxManifestError).reason;
      }
      expect(reason).toContain(`benefits.${legacyClass}`);
      expect(reason).toContain("retired");
      expect(reason).toContain("exactly one benefit");
    }
  });

  test("rejects an expiry that is structurally invalid", () => {
    // Null (or absent) means "no expiration configured" and parses; anything
    // recorded must be a positive integer ttl with one of Polar's timeframes.
    // Whether a recorded value is allowed at all is the verifier's rule, not
    // the parser's — any recorded expiry then fails verification.
    const cases: Array<(benefit: Record<string, unknown>) => void> = [
      (benefit) => {
        benefit.expiry = { ttl: 0, timeframe: "year" };
      },
      (benefit) => {
        benefit.expiry = { ttl: 1.5, timeframe: "year" };
      },
      (benefit) => {
        benefit.expiry = { ttl: 1, timeframe: "decade" };
      },
      (benefit) => {
        benefit.expiry = { ttl: 1, timeframe: "year", graceDays: 7 };
      },
    ];
    for (const breakExpiry of cases) {
      const manifest = validManifest();
      breakExpiry(
        (manifest.benefits as Record<string, unknown>)
          .supporter as Record<string, unknown>,
      );
      expect(() => parseSandboxManifest(manifest)).toThrow(
        SandboxManifestError,
      );
    }
  });

  test("accepts an expiry recorded as null, and an absent one, at parse time", () => {
    // Null or absent means "no expiration configured", which is the required
    // state under the perpetual-supporter model. Parsing accepts it; a
    // RECORDED value is what fails verification, not the parser.
    expect(() => parseSandboxManifest(validManifest())).not.toThrow();
    const explicitNull = validManifest();
    (
      (explicitNull.benefits as Record<string, unknown>)
        .supporter as Record<string, unknown>
    ).expiry = null;
    expect(() => parseSandboxManifest(explicitNull)).not.toThrow();
  });
});
