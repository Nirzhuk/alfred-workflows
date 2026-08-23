import { describe, expect, test } from "bun:test";
import {
  parseSandboxManifest,
  type SandboxManifest,
} from "./manifest";
import type { SandboxTestKeys } from "./secrets";
import { isOneYearExpiry, verifyPolarSandbox } from "./verifier";

const TEST_KEYS: SandboxTestKeys = {
  individual: "individual-private-value",
  teams: "teams-private-value",
};

const DAY_MS = 24 * 60 * 60 * 1000;

/** A plausible live expiry: issued a while ago, still inside its one year. */
function inWindowExpiry(): string {
  return new Date(Date.now() + 200 * DAY_MS).toISOString();
}

function manifestFixture(): SandboxManifest {
  return parseSandboxManifest({
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
        url: "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect",
        label: "Alfred Sandbox License Checkout",
      },
    },
    customerPortal: {
      // No confirmed sandbox portal URL shape yet; the verifier does not need one.
      url: null,
      label: "Alfred Sandbox Customer Portal",
    },
  });
}

type CapturedRequest = {
  readonly path: string;
  readonly headers: Headers;
  readonly body: Record<string, string>;
};

function createMockPolar(
  options: {
    failFirstValidation?: boolean;
    /** Overrides the one-year expiry every Alfred key is issued with. */
    expiresAt?: string | null;
  } = {},
) {
  const requests: CapturedRequest[] = [];
  const active = new Map<string, Set<string>>();
  let nextId = 1;
  let failedValidation = false;

  const benefitForKey = (key: string) =>
    key === TEST_KEYS.individual
      ? "11111111-1111-4111-8111-111111111111"
      : "22222222-2222-4222-8222-222222222222";

  const licenseForKey = (key: string) => ({
    organization_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    benefit_id: benefitForKey(key),
    status: "granted",
    limit_activations: 3,
    expires_at:
      options.expiresAt === undefined ? inWindowExpiry() : options.expiresAt,
  });

  const fetcher = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(
      input instanceof Request ? input.url : input instanceof URL ? input : input,
    );
    const body = JSON.parse(String(init?.body)) as Record<string, string>;
    requests.push({
      path: url.pathname,
      headers: new Headers(init?.headers),
      body,
    });

    const key = body.key;
    const activations = active.get(key) ?? new Set<string>();
    active.set(key, activations);

    if (url.pathname.endsWith("/activate")) {
      if (activations.size >= 3) return new Response(null, { status: 403 });
      const suffix = String(nextId).padStart(12, "0");
      nextId += 1;
      const id = `00000000-0000-4000-8000-${suffix}`;
      activations.add(id);
      return Response.json({
        id,
        label: body.label,
        license_key: licenseForKey(key),
      });
    }

    if (url.pathname.endsWith("/validate")) {
      if (options.failFirstValidation && !failedValidation) {
        failedValidation = true;
        return Response.json(
          { error: "private response", echoed: key },
          { status: 500 },
        );
      }
      if (!activations.has(body.activation_id)) {
        return new Response(null, { status: 404 });
      }
      return Response.json(licenseForKey(key));
    }

    if (url.pathname.endsWith("/deactivate")) {
      if (!activations.delete(body.activation_id)) {
        return new Response(null, { status: 404 });
      }
      return new Response(null, { status: 204 });
    }

    return new Response(null, { status: 404 });
  }) as typeof fetch;

  return { fetcher, requests, active };
}

describe("Polar sandbox verifier", () => {
  test("checks both benefit classes and the three-activation contract without authorization", async () => {
    const mock = createMockPolar();
    const output: string[] = [];

    const result = await verifyPolarSandbox({
      manifest: manifestFixture(),
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: (passed, caseName) =>
        output.push(`${passed ? "PASS" : "FAIL"} ${caseName}`),
    });

    expect(result.passed).toBe(true);
    expect(output).toHaveLength(20);
    expect(output.every((line) => line.startsWith("PASS "))).toBe(true);
    expect(mock.requests).toHaveLength(26);
    expect(
      mock.requests.every((request) => !request.headers.has("authorization")),
    ).toBe(true);
    expect(
      mock.requests.every(
        (request) =>
          request.path ===
            "/v1/customer-portal/license-keys/activate" ||
          request.path ===
            "/v1/customer-portal/license-keys/validate" ||
          request.path ===
            "/v1/customer-portal/license-keys/deactivate",
      ),
    ).toBe(true);
    expect([...mock.active.values()].every((ids) => ids.size === 0)).toBe(true);
  });

  test("fails a key issued with no expiry, for either class", async () => {
    // The retired rule asserted the opposite: a lifetime key had to have NO
    // expiry. Under the two-product model a key without one cannot carry an
    // update deadline, so the Polar product is misconfigured.
    const mock = createMockPolar({ expiresAt: null });
    const output: string[] = [];

    const result = await verifyPolarSandbox({
      manifest: manifestFixture(),
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: (passed, caseName) =>
        output.push(`${passed ? "PASS" : "FAIL"} ${caseName}`),
    });

    expect(result.passed).toBe(false);
    expect(output).toContain("FAIL individual.activate-1");
    expect(output).toContain("FAIL teams.activate-1");
  });

  test("fails an expiry that is not roughly one year out", async () => {
    const perpetual = createMockPolar({
      expiresAt: new Date(Date.now() + 3650 * DAY_MS).toISOString(),
    });
    const alreadyLapsed = createMockPolar({
      expiresAt: new Date(Date.now() - DAY_MS).toISOString(),
    });

    for (const mock of [perpetual, alreadyLapsed]) {
      const result = await verifyPolarSandbox({
        manifest: manifestFixture(),
        keys: TEST_KEYS,
        fetcher: mock.fetcher,
        report: () => undefined,
      });
      expect(result.passed).toBe(false);
    }
  });

  test("redacts keys and response data from pass/fail-only output", async () => {
    const mock = createMockPolar({ failFirstValidation: true });
    const output: string[] = [];

    const result = await verifyPolarSandbox({
      manifest: manifestFixture(),
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: (passed, caseName) =>
        output.push(`${passed ? "PASS" : "FAIL"} ${caseName}`),
    });

    expect(result.passed).toBe(false);
    const rendered = output.join("\n");
    expect(rendered).not.toContain("private-value");
    expect(rendered).not.toContain("private response");
    expect(output.every((line) => /^(PASS|FAIL) [a-z0-9.-]+$/.test(line))).toBe(
      true,
    );
  });

  test("deactivates allocated instances in finally after a failed validation", async () => {
    const mock = createMockPolar({ failFirstValidation: true });

    await verifyPolarSandbox({
      manifest: manifestFixture(),
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: () => undefined,
    });

    const individualDeactivations = mock.requests.filter(
      (request) =>
        request.path.endsWith("/deactivate") &&
        request.body.key === TEST_KEYS.individual,
    );
    expect(individualDeactivations).toHaveLength(1);
    expect([...mock.active.values()].every((ids) => ids.size === 0)).toBe(true);
  });
});

describe("one-year expiry rule", () => {
  const now = new Date("2026-08-20T00:00:00Z");

  test("requires an expiry that exists, is a date, and is still ahead", () => {
    expect(isOneYearExpiry(null, now)).toBe(false);
    expect(isOneYearExpiry("not-a-date", now)).toBe(false);
    expect(isOneYearExpiry("2026-08-19T23:59:59Z", now)).toBe(false);
    // Exactly now is already lapsed, not in window.
    expect(isOneYearExpiry("2026-08-20T00:00:00Z", now)).toBe(false);
  });

  test("accepts one year out, and anything short of it", () => {
    expect(isOneYearExpiry("2027-08-20T00:00:00Z", now)).toBe(true);
    // A key issued months ago expires sooner than a year from today; the read
    // carries no issue date, so a shorter remaining window is still correct.
    expect(isOneYearExpiry("2026-09-20T00:00:00Z", now)).toBe(true);
    // One week of slack above a year absorbs clock skew and Polar's rounding.
    expect(isOneYearExpiry("2027-08-27T00:00:00Z", now)).toBe(true);
  });

  test("rejects a window materially longer than a year", () => {
    expect(isOneYearExpiry("2027-08-28T00:00:01Z", now)).toBe(false);
    expect(isOneYearExpiry("2036-08-20T00:00:00Z", now)).toBe(false);
  });

  test("handles a leap day without drifting a year", () => {
    const leapDay = new Date("2028-02-29T00:00:00Z");
    expect(isOneYearExpiry("2029-02-28T00:00:00Z", leapDay)).toBe(true);
    // JS rolls 2029-02-29 to 2029-03-01, which is the cap plus slack.
    expect(isOneYearExpiry("2029-03-01T00:00:00Z", leapDay)).toBe(true);
    expect(isOneYearExpiry("2029-04-01T00:00:00Z", leapDay)).toBe(false);
  });
});
