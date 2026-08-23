import { describe, expect, test } from "bun:test";
import {
  createPolarPublicLinks,
  parsePolarPublicLink,
  PolarPublicLinkError,
  type PolarPublicLinkConfig,
  readPolarPublicLinkConfig,
} from "./public-links";
import { readPolarLinkEnvironment } from "./public-link-rules";

const SANDBOX_DESKTOP_CHECKOUT =
  "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_desktopFixture/redirect";

const configured: PolarPublicLinkConfig = {
  desktopCheckout: "https://buy.polar.sh/polar_cl_desktopFixture",
  customerPortal: "https://polar.sh/alfred/portal",
};

describe("Polar public links", () => {
  test("opens only the fixed configured destination for each action", async () => {
    const opened: string[] = [];
    const links = createPolarPublicLinks(configured, async (url) => {
      opened.push(url);
    });

    await links.open("desktopCheckout");
    await links.open("customerPortal");

    expect(opened).toEqual([
      configured.desktopCheckout,
      configured.customerPortal,
    ]);
  });

  test("rejects non-HTTPS, unexpected hosts, paths, and redirect input", () => {
    const rejected = [
      "http://buy.polar.sh/polar_cl_fixture",
      "https://example.com/polar_cl_fixture",
      "https://buy.polar.sh/checkout/polar_cl_fixture",
      "https://buy.polar.sh/polar_cl_fixture?redirect=https://example.com",
      "https://buy.polar.sh/polar_cl_fixture#redirect",
      "https://user@buy.polar.sh/polar_cl_fixture",
      "https://buy.polar.sh:444/polar_cl_fixture",
    ];

    for (const value of rejected) {
      expect(parsePolarPublicLink("desktopCheckout", value)).toBeNull();
    }
    expect(
      parsePolarPublicLink("customerPortal", "https://polar.sh/account"),
    ).toBeNull();
  });

  test("reads all three links from the reviewed VITE_POLAR_* build variables", () => {
    expect(
      readPolarPublicLinkConfig({
        VITE_POLAR_DESKTOP_CHECKOUT_URL: configured.desktopCheckout,
        VITE_POLAR_CUSTOMER_PORTAL_URL: configured.customerPortal,
      }),
    ).toEqual(configured);

    // An unconfigured source build: no variables set at all.
    expect(readPolarPublicLinkConfig({})).toEqual({
      desktopCheckout: undefined,
      customerPortal: undefined,
    });
  });

  test("keeps a partially configured build honest per destination", async () => {
    // A half-filled .env must not make the unset destinations look available
    // or fail the ones that are bound.
    const links = createPolarPublicLinks(
      readPolarPublicLinkConfig({
        VITE_POLAR_DESKTOP_CHECKOUT_URL: configured.desktopCheckout,
        VITE_POLAR_CUSTOMER_PORTAL_URL: "   ",
      }),
      async () => {},
    );

    expect(links.isConfigured("desktopCheckout")).toBe(true);
    expect(links.isConfigured("customerPortal")).toBe(false);

    await links.open("desktopCheckout");
    await expect(links.open("customerPortal")).rejects.toEqual(
      new PolarPublicLinkError("not_configured"),
    );
  });

  test("rejects missing config and runtime URL destinations", async () => {
    const links = createPolarPublicLinks(
      {
        desktopCheckout: undefined,
        customerPortal: undefined,
      },
      async () => {},
    );

    expect(links.isConfigured("desktopCheckout")).toBe(false);
    await expect(links.open("desktopCheckout")).rejects.toEqual(
      new PolarPublicLinkError("not_configured"),
    );
    await expect(
      links.open("https://example.com/redirect" as never),
    ).rejects.toEqual(new PolarPublicLinkError("invalid_destination"));
  });

  test("reads the environment from the same .env value build.rs bakes in", () => {
    expect(readPolarLinkEnvironment("sandbox")).toBe("sandbox");
    expect(readPolarLinkEnvironment("  sandbox  ")).toBe("sandbox");
    expect(readPolarLinkEnvironment("production")).toBe("production");
    // An unset, blank, or unrecognised value gets the tighter allow-list.
    expect(readPolarLinkEnvironment(undefined)).toBe("production");
    expect(readPolarLinkEnvironment("")).toBe("production");
    expect(readPolarLinkEnvironment("Sandbox")).toBe("production");
    expect(readPolarLinkEnvironment("staging")).toBe("production");
  });

  test("a sandbox build accepts the sandbox checkout shape and nothing wider", () => {
    expect(
      parsePolarPublicLink(
        "desktopCheckout",
        SANDBOX_DESKTOP_CHECKOUT,
        "sandbox",
      ),
    ).toBe(SANDBOX_DESKTOP_CHECKOUT);

    // Right host, wrong path shape; right path shape, wrong host.
    const rejected = [
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture",
      "https://sandbox-api.polar.sh/polar_cl_fixture",
      "https://sandbox-api.polar.sh/v1/checkout-links/other_fixture/redirect",
      "https://sandbox-api.polar.sh/",
      "https://sandbox.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://sandbox-api.polar.sh.attacker.invalid/v1/checkout-links/polar_cl_fixture/redirect",
    ];
    for (const value of rejected) {
      expect(
        parsePolarPublicLink("desktopCheckout", value, "sandbox"),
      ).toBeNull();
    }
  });

  test("keeps every URL safety check on the sandbox shape too", () => {
    // https only, no username/password, no port, no query string, no
    // fragment. A URL carrying a customer_session_token — or any query
    // parameter at all — is a credential, not a public link.
    const unsafe = [
      "http://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://user@sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://user:pass@sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
      "https://sandbox-api.polar.sh:444/v1/checkout-links/polar_cl_fixture/redirect",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect?customer_session_token=redacted",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect?redirect=https://example.com",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect#token",
    ];
    for (const value of unsafe) {
      expect(
        parsePolarPublicLink("desktopCheckout", value, "sandbox"),
      ).toBeNull();
    }
  });

  test("rejects an ephemeral checkout session in either environment", () => {
    // `polar_c_` is a single-use checkout SESSION that expires; `polar_cl_` is
    // a durable checkout LINK. Baking a session into a build ships a Buy
    // button that dies on its own, so no environment may accept one. This
    // exact URL was pasted into a real .env, which is why it has a test.
    const sessions = [
      "https://sandbox.polar.sh/checkout/polar_c_QwcsVLPLIGBrfpNaMdf8pUy1G0t7qMATwhd2Y0j3Jn3",
      "https://sandbox-api.polar.sh/v1/checkout-links/polar_c_QwcsVLPL/redirect",
      "https://buy.polar.sh/polar_c_QwcsVLPL",
    ];
    for (const environment of ["sandbox", "production"] as const) {
      for (const value of sessions) {
        expect(
          parsePolarPublicLink("desktopCheckout", value, environment),
        ).toBeNull();
      }
    }
  });

  test("neither environment accepts the other's link shapes", () => {
    // A sandbox build must never open a live checkout link, and a production
    // build must never open a sandbox one. This is a per-environment
    // allow-list, not production widened to also permit sandbox.
    expect(
      parsePolarPublicLink(
        "desktopCheckout",
        configured.desktopCheckout,
        "sandbox",
      ),
    ).toBeNull();
    expect(
      parsePolarPublicLink(
        "desktopCheckout",
        SANDBOX_DESKTOP_CHECKOUT,
        "production",
      ),
    ).toBeNull();
    // The default with no environment argument is the tighter production set.
    expect(
      parsePolarPublicLink("desktopCheckout", SANDBOX_DESKTOP_CHECKOUT),
    ).toBeNull();
  });

  test("accepts the per-organization customer portal, never a global path", () => {
    // Polar's hosted portal is per-organization: /<org-slug>/portal. There is
    // no global /purchases page — that path 404s on both hosts.
    expect(
      parsePolarPublicLink(
        "customerPortal",
        "https://sandbox.polar.sh/alfred/portal",
        "sandbox",
      ),
    ).toBe("https://sandbox.polar.sh/alfred/portal");
    expect(
      parsePolarPublicLink("customerPortal", "https://polar.sh/alfred/portal"),
    ).toBe("https://polar.sh/alfred/portal");

    const rejected: [string, "sandbox" | "production"][] = [
      // The dead global path, on either host.
      ["https://polar.sh/purchases", "production"],
      ["https://sandbox.polar.sh/purchases", "sandbox"],
      // Wrong host for the environment.
      ["https://sandbox-api.polar.sh/alfred/portal", "sandbox"],
      ["https://sandbox.polar.sh/alfred/portal", "production"],
      ["https://polar.sh/alfred/portal", "sandbox"],
      ["https://polar.sh.attacker.invalid/alfred/portal", "production"],
      // The slug is exactly one segment and cannot be walked deeper.
      ["https://polar.sh/portal", "production"],
      ["https://polar.sh/alfred/portal/request", "production"],
      ["https://polar.sh/a/b/portal", "production"],
      ["https://polar.sh/alfred/portal/../admin", "production"],
    ];
    for (const [value, environment] of rejected) {
      expect(
        parsePolarPublicLink("customerPortal", value, environment),
      ).toBeNull();
    }
  });

  test("keeps every URL safety check on the portal shape", () => {
    const unsafe = [
      "http://sandbox.polar.sh/alfred/portal",
      "https://user:pass@sandbox.polar.sh/alfred/portal",
      "https://sandbox.polar.sh:444/alfred/portal",
      "https://sandbox.polar.sh/alfred/portal?customer_session_token=redacted",
      "https://sandbox.polar.sh/alfred/portal#token",
    ];
    for (const value of unsafe) {
      expect(
        parsePolarPublicLink("customerPortal", value, "sandbox"),
      ).toBeNull();
    }
  });

  test("opens sandbox links only when the build is bound to sandbox", async () => {
    const opened: string[] = [];
    const links = createPolarPublicLinks(
      {
        desktopCheckout: SANDBOX_DESKTOP_CHECKOUT,
        customerPortal: "https://sandbox.polar.sh/alfred/portal",
      },
      async (url) => {
        opened.push(url);
      },
      "sandbox",
    );

    expect(links.isConfigured("desktopCheckout")).toBe(true);
    expect(links.isConfigured("customerPortal")).toBe(true);

    await links.open("desktopCheckout");
    await links.open("customerPortal");
    expect(opened).toEqual([
      SANDBOX_DESKTOP_CHECKOUT,
      "https://sandbox.polar.sh/alfred/portal",
    ]);

    // Every one of those sandbox values is refused by a production build.
    const production = createPolarPublicLinks(
      {
        desktopCheckout: SANDBOX_DESKTOP_CHECKOUT,
        customerPortal: "https://sandbox.polar.sh/alfred/portal",
      },
      async () => {},
    );
    expect(production.isConfigured("desktopCheckout")).toBe(false);
    expect(production.isConfigured("customerPortal")).toBe(false);
  });
});
