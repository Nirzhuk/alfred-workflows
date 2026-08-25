import { describe, expect, test } from "bun:test";
import {
  loadSandboxManifest,
  parseSandboxManifest,
  SandboxManifestError,
  type SandboxManifest,
} from "./manifest";
import type { SandboxTestKeys } from "./secrets";
import {
  expiryConfigMismatches,
  verifyPolarSandbox,
} from "./verifier";

const TEST_KEYS: SandboxTestKeys = {
  supporter: "supporter-private-value",
};

const DAY_MS = 24 * 60 * 60 * 1000;

function manifestFixture(): SandboxManifest {
  return parseSandboxManifest({
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
        url: "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_license/redirect",
        label: "Alfred Supporter Checkout",
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
    /** Overrides the null expiry every perpetual supporter key is issued with. */
    expiresAt?: string | null;
  } = {},
) {
  const requests: CapturedRequest[] = [];
  const active = new Map<string, Set<string>>();
  let nextId = 1;
  let failedValidation = false;

  const benefitForKey = (_key: string) =>
    "11111111-1111-4111-8111-111111111111";

  const licenseForKey = (key: string) => ({
    organization_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    benefit_id: benefitForKey(key),
    status: "granted",
    limit_activations: 3,
    // Perpetual model: Polar issues keys with expires_at null. An override
    // simulates a benefit misconfigured with an expiration.
    expires_at:
      options.expiresAt === undefined ? null : options.expiresAt,
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
  test("runs the full three-activation contract once for the supporter class", async () => {
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
    // One class x ten cases: activate/validate 1-3, fourth rejected,
    // deactivate, replacement, cleanup.
    expect(output).toHaveLength(10);
    expect(output.every((line) => line.startsWith("PASS supporter."))).toBe(
      true,
    );
    expect(mock.requests).toHaveLength(13);
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
  });

  test("fails fast pre-network when the checkout link is absent", async () => {
    const shipped = await loadSandboxManifest();
    const manifest = parseSandboxManifest({
      ...shipped,
      checkoutLinks: {
        supporter: {
          ...shipped.checkoutLinks.supporter,
          url: null,
        },
      },
    });

    const mock = createMockPolar();
    const failures: Array<{ caseName: string; detail?: readonly string[] }> =
      [];

    const result = await verifyPolarSandbox({
      manifest,
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: (passed, caseName, detail) => {
        if (!passed) failures.push({ caseName, detail });
      },
    });

    expect(result.passed).toBe(false);
    expect(failures).toHaveLength(1);
    expect(failures[0].caseName).toBe("manifest.checkout.supporter");
    expect(failures[0].detail?.join(" ")).toContain(
      "collect the checkout link from the Polar dashboard",
    );
    expect(mock.requests).toHaveLength(0);
    expect(result.caseNames).toEqual(["manifest.checkout.supporter"]);
  });

  test("fails a key issued WITH an expiry", async () => {
    // The retired one-year rule asserted the opposite. Under the perpetual
    // supporter model a key must read expires_at: null; any expiry means the
    // Polar benefit is misconfigured with a license-key expiration.
    const mock = createMockPolar({
      expiresAt: new Date(Date.now() + 200 * DAY_MS).toISOString(),
    });
    const output: string[] = [];

    const result = await verifyPolarSandbox({
      manifest: manifestFixture(),
      keys: TEST_KEYS,
      fetcher: mock.fetcher,
      report: (passed, caseName) =>
        output.push(`${passed ? "PASS" : "FAIL"} ${caseName}`),
    });

    expect(result.passed).toBe(false);
    expect(output).toContain("FAIL supporter.activate-1");
  });

  test("passes when no expiry is recorded", () => {
    expect(expiryConfigMismatches(manifestFixture())).toEqual([]);
  });

  test("fails fast on ANY recorded expiry", async () => {
    // Supporter licences are perpetual: a recorded ttl or timeframe on the
    // benefit is a configuration error, and no live key can compensate, so
    // the verifier refuses before touching Polar.
    for (const expiry of [
      { ttl: 1, timeframe: "year" as const },
      { ttl: 1, timeframe: "month" as const },
      { ttl: null, timeframe: "year" as const },
      { ttl: 2, timeframe: null },
    ]) {
      const fixture = manifestFixture();
      const recorded = parseSandboxManifest({
        ...fixture,
        benefits: {
          ...fixture.benefits,
          supporter: { ...fixture.benefits.supporter, expiry },
        },
      });
      const mock = createMockPolar();
      const details: string[] = [];

      const result = await verifyPolarSandbox({
        manifest: recorded,
        keys: TEST_KEYS,
        fetcher: mock.fetcher,
        report: (passed, caseName, detail) => {
          if (!passed && detail) details.push(...detail);
        },
      });

      expect(result.passed).toBe(false);
      expect(mock.requests).toHaveLength(0);
      expect(details.join("\n")).toContain(
        "supporter licences are perpetual",
      );
    }
  });

  test("rejects any recorded expiry, naming the field", () => {
    // A recorded month-long window parses (it is structurally valid) but the
    // verifier refuses it: supporter licences are perpetual, so ANY recorded
    // expiry is a misconfiguration against the model.
    const fixture = manifestFixture();
    const monthTimeframe = parseSandboxManifest({
      ...fixture,
      benefits: {
        ...fixture.benefits,
        supporter: {
          ...fixture.benefits.supporter,
          expiry: { ttl: 1, timeframe: "month" },
        },
      },
    });
    expect(expiryConfigMismatches(monthTimeframe).join("\n")).toContain(
      'benefits.supporter.expiry.timeframe is "month"',
    );

    // A ttl below 1 never reaches the verifier: the manifest parser rejects
    // it up front, with a reason naming benefits.supporter.expiry.ttl.
    const zeroTtl = {
      ...manifestFixture(),
      benefits: {
        ...manifestFixture().benefits,
        supporter: {
          ...manifestFixture().benefits.supporter,
          expiry: { ttl: 0, timeframe: "year" },
        },
      },
    };
    let reason = "";
    try {
      parseSandboxManifest(zeroTtl);
    } catch (error) {
      reason = (error as SandboxManifestError).reason;
    }
    expect(reason).toContain("benefits.supporter.expiry.ttl");
  });

  test("fails a key with any expires_at, near or far", async () => {
    const soonExpiring = createMockPolar({
      expiresAt: new Date(Date.now() + DAY_MS).toISOString(),
    });
    const farFuture = createMockPolar({
      expiresAt: new Date(Date.now() + 3650 * DAY_MS).toISOString(),
    });

    for (const mock of [soonExpiring, farFuture]) {
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

    const deactivations = mock.requests.filter(
      (request) => request.path.endsWith("/deactivate"),
    );
    expect(deactivations).toHaveLength(1);
    expect([...mock.active.values()].every((ids) => ids.size === 0)).toBe(true);
  });
});
