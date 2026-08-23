/**
 * The single allow-list every Polar public link is checked against, keyed on
 * the publisher's `ALFRED_POLAR_ENVIRONMENT`. Three places consume it:
 *
 *   1. `src/features/licensing/public-links.ts` — what the app will open;
 *   2. `scripts/polar/manifest.ts` — what an operator may record;
 *   3. `src-tauri/capabilities/default.json` — the Tauri
 *      `opener:allow-open-url` scope. Tauri capabilities are static JSON, so
 *      that one is kept in step by hand; every rule here has a matching entry
 *      there and nothing else.
 *
 * A production build accepts production shapes only and a sandbox build
 * accepts sandbox shapes only. Widening either to cover the other would let a
 * sandbox checkout link ship to paying customers, so the two sets never merge.
 */
export type PolarLinkEnvironment = "production" | "sandbox";

export type PolarLinkKind = "checkout" | "portal";

export type PolarLinkRule = {
  readonly hostname: string;
  readonly pathname: RegExp;
  /** Operator-facing description of the shape, used verbatim in errors. */
  readonly shape: string;
};

/** Polar's live checkout links: `https://buy.polar.sh/polar_cl_<id>`. */
/**
 * One path segment for the organization slug, then `/portal`. The slug class
 * excludes `/` so the rule cannot be walked into another path, and excludes
 * `.` so it cannot carry a host-looking segment.
 */
const ORG_PORTAL_PATH = /^\/[A-Za-z0-9][A-Za-z0-9_-]*\/portal$/;

const PRODUCTION_CHECKOUT: PolarLinkRule = {
  hostname: "buy.polar.sh",
  pathname: /^\/polar_cl_[A-Za-z0-9_-]+$/,
  shape: "https://buy.polar.sh/polar_cl_...",
};

/**
 * Polar's hosted customer portal is per-organization: `/<org-slug>/portal`,
 * which redirects to `/<org-slug>/portal/request` for the email sign-in code.
 * There is no global `/purchases` page — that path 404s and appears nowhere in
 * Polar's documentation.
 */
const PRODUCTION_PORTAL: PolarLinkRule = {
  hostname: "polar.sh",
  pathname: ORG_PORTAL_PATH,
  shape: "https://polar.sh/<org-slug>/portal",
};

/**
 * Sandbox has no `buy.` host. Polar issues a sandbox checkout link as the
 * API's own redirect endpoint, `/v1/checkout-links/<id>/redirect` on
 * `sandbox-api.polar.sh` — the same host `src-tauri/src/licensing/config.rs`
 * already allows for the sandbox API base.
 */
const SANDBOX_CHECKOUT: PolarLinkRule = {
  hostname: "sandbox-api.polar.sh",
  pathname: /^\/v1\/checkout-links\/polar_cl_[A-Za-z0-9_-]+\/redirect$/,
  shape: "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_.../redirect",
};

/**
 * The sandbox portal is the same per-organization path on the sandbox
 * dashboard host. Confirmed against the live sandbox organization:
 * `https://sandbox.polar.sh/<org-slug>/portal` returns 200 and redirects to
 * `/portal/request`, while `sandbox.polar.sh/purchases` 404s.
 */
const SANDBOX_PORTAL: PolarLinkRule = {
  hostname: "sandbox.polar.sh",
  pathname: ORG_PORTAL_PATH,
  shape: "https://sandbox.polar.sh/<org-slug>/portal",
};

export const POLAR_LINK_RULES: Readonly<
  Record<
    PolarLinkEnvironment,
    Readonly<Record<PolarLinkKind, readonly PolarLinkRule[]>>
  >
> = {
  production: {
    checkout: [PRODUCTION_CHECKOUT],
    portal: [PRODUCTION_PORTAL],
  },
  sandbox: {
    checkout: [SANDBOX_CHECKOUT],
    portal: [SANDBOX_PORTAL],
  },
};

export function polarLinkRulesFor(
  environment: PolarLinkEnvironment,
  kind: PolarLinkKind,
): readonly PolarLinkRule[] {
  return POLAR_LINK_RULES[environment][kind];
}

/** Every accepted shape for one environment and kind, for an error message. */
export function polarLinkShapes(
  environment: PolarLinkEnvironment,
  kind: PolarLinkKind,
): string {
  return polarLinkRulesFor(environment, kind)
    .map((rule) => rule.shape)
    .join(" or ");
}

/**
 * A public link carries no credential and no state: https only, no
 * username/password, no port, no query string, no fragment. A URL bearing a
 * `customer_session_token` — or any query parameter at all — is a credential
 * rather than a public link and is rejected here before the host is even
 * considered.
 */
export function matchesPolarLinkRule(
  url: URL,
  rules: readonly PolarLinkRule[],
): boolean {
  if (
    url.protocol !== "https:" ||
    url.username !== "" ||
    url.password !== "" ||
    url.port !== "" ||
    url.search !== "" ||
    url.hash !== ""
  ) {
    return false;
  }
  return rules.some(
    (rule) =>
      url.hostname === rule.hostname && rule.pathname.test(url.pathname),
  );
}

/**
 * Anything other than an explicit `sandbox` is treated as production, so an
 * unset or malformed build variable gets the tighter allow-list rather than
 * the looser one.
 */
export function readPolarLinkEnvironment(
  value: string | undefined,
): PolarLinkEnvironment {
  return value?.trim() === "sandbox" ? "sandbox" : "production";
}
