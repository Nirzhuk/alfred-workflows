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
      individual: {
        id: "11111111-1111-4111-8111-111111111111",
        label: "Alfred Sandbox License",
      },
      teams: {
        id: "22222222-2222-4222-8222-222222222222",
        label: "Alfred Sandbox Teams",
      },
    },
    checkoutLinks: {
      individual: {
        url: SANDBOX_CHECKOUT,
        label: "Alfred Sandbox License Checkout",
      },
    },
    customerPortal: {
      url: null,
      label: "Alfred Sandbox Customer Portal",
    },
  };
}

describe("sandbox manifest", () => {
  test("accepts the sandbox checkout shape Polar actually issues", () => {
    // Polar has no `buy.` host in sandbox: a sandbox checkout link is the
    // API's own redirect endpoint on sandbox-api.polar.sh.
    const manifest = parseSandboxManifest(validManifest());

    expect(manifest.environment).toBe("sandbox");
    expect(manifest.benefits.individual.id).toBe(
      "11111111-1111-4111-8111-111111111111",
    );
    expect(manifest.benefits.teams.id).toBe(
      "22222222-2222-4222-8222-222222222222",
    );
    expect(manifest.checkoutLinks.individual.url).toBe(SANDBOX_CHECKOUT);
    // A null portal is a valid recorded state, not a missing field.
    expect(manifest.customerPortal.url).toBeNull();
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

  test("still allows a portal that has not been collected yet", () => {
    // null records "not collected", so the rest of the manifest stays usable.
    expect(parseSandboxManifest(validManifest()).customerPortal.url).toBeNull();
  });

  test("fails closed for unconfigured, production, secret-bearing, or duplicate values", () => {
    const unconfigured = validManifest();
    unconfigured.organizationId = null;
    expect(() => parseSandboxManifest(unconfigured)).toThrow(
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
        .individual as Record<string, unknown>
    ).url =
      "https://token@sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect";
    expect(() => parseSandboxManifest(secretBearingLink)).toThrow(
      SandboxManifestError,
    );

    const duplicate = validManifest();
    (
      (duplicate.benefits as Record<string, unknown>).teams as Record<
        string,
        unknown
      >
    ).id = "11111111-1111-4111-8111-111111111111";
    expect(() => parseSandboxManifest(duplicate)).toThrow(
      SandboxManifestError,
    );
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
          .individual as Record<string, unknown>
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
          .individual as Record<string, unknown>
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
        .individual as Record<string, unknown>
    ).url =
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect?customer_session_token=super-secret";

    let reason = "";
    try {
      parseSandboxManifest(manifest);
    } catch (error) {
      reason = (error as SandboxManifestError).reason;
    }
    expect(reason).toContain("checkoutLinks.individual.url");
    expect(reason).not.toContain("super-secret");
  });

  test("rejects arbitrary public-link destinations", () => {
    const manifest = validManifest();
    (
      (manifest.checkoutLinks as Record<string, unknown>)
        .individual as Record<string, unknown>
    ).url = "https://example.com/checkout";
    expect(() => parseSandboxManifest(manifest)).toThrow(
      SandboxManifestError,
    );
  });

  test("rejects the retired three-benefit shape", () => {
    // Plan 007 Step 4: the manifest must refuse the old four-product
    // configuration outright rather than parse the two classes it recognises
    // and silently ignore the third.
    const legacy = validManifest();
    (legacy.benefits as Record<string, unknown>).companySeat = {
      id: "33333333-3333-4333-8333-333333333333",
      label: "Alfred Sandbox Company Seat",
    };

    let reason = "";
    try {
      parseSandboxManifest(legacy);
    } catch (error) {
      reason = (error as SandboxManifestError).reason;
    }
    expect(reason).toContain("benefits.companySeat");
    expect(() => parseSandboxManifest(legacy)).toThrow(SandboxManifestError);
  });

  test("requires both benefit IDs, with no optional class left", () => {
    // The Company seat benefit used to be allowed as `id: null`. Alfred Teams
    // is a shipped product with its own key benefit, so an unbound ID is an
    // incomplete configuration, not a complete one.
    for (const kind of ["individual", "teams"]) {
      const manifest = validManifest();
      (
        (manifest.benefits as Record<string, unknown>)[kind] as Record<
          string,
          unknown
        >
      ).id = null;
      expect(() => parseSandboxManifest(manifest)).toThrow(
        SandboxManifestError,
      );
    }
  });

  test("rejects a Teams checkout link, which the app never opens", () => {
    // Teams is sold on the marketing website. A recorded Teams link would look
    // bound while nothing in the app could ever open it.
    const manifest = validManifest();
    (manifest.checkoutLinks as Record<string, unknown>).teams = {
      url: SANDBOX_CHECKOUT,
      label: "Alfred Sandbox Teams Checkout",
    };
    expect(() => parseSandboxManifest(manifest)).toThrow(SandboxManifestError);
  });
});
