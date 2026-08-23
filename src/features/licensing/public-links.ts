import { openUrl } from "@tauri-apps/plugin-opener";
import {
  matchesPolarLinkRule,
  polarLinkRulesFor,
  type PolarLinkEnvironment,
  readPolarLinkEnvironment,
} from "./public-link-rules";

/**
  * Company plans are sold on the marketing website, not in the app: a Desktop
  * customer never gets a Company purchase path here. A Company *seat holder*
  * still activates their key in Alfred like any other licensee.
  */
export type PolarDestination = "desktopCheckout" | "customerPortal";

export type PolarPublicLinkConfig = Record<
  PolarDestination,
  string | undefined
>;

export type PolarPublicLinkEnvironment = {
  /** Baked in by `vite.config.ts` from the same `.env` value `build.rs` reads. */
  readonly ALFRED_POLAR_ENVIRONMENT?: string;
  readonly VITE_POLAR_DESKTOP_CHECKOUT_URL?: string;
  readonly VITE_POLAR_CUSTOMER_PORTAL_URL?: string;
};

export type PolarPublicLinks = {
  isConfigured: (destination: PolarDestination) => boolean;
  open: (destination: PolarDestination) => Promise<void>;
};

export class PolarPublicLinkError extends Error {
  constructor(
    public readonly code: "not_configured" | "invalid_destination",
  ) {
    super(
      code === "not_configured"
        ? "Licensing checkout is not configured in this build."
        : "This Polar destination is not allowed.",
    );
    this.name = "PolarPublicLinkError";
  }
}

type ExternalOpener = (url: string) => Promise<void>;

const DESTINATIONS: PolarDestination[] = ["desktopCheckout", "customerPortal"];

function isDestination(value: unknown): value is PolarDestination {
  return DESTINATIONS.includes(value as PolarDestination);
}

/**
 * Accepts a link only if it matches the allow-list for `environment`. The
 * default is `production`, the tighter of the two, so an unbound build never
 * inherits sandbox shapes.
 */
export function parsePolarPublicLink(
  destination: PolarDestination,
  input: string | undefined,
  environment: PolarLinkEnvironment = "production",
): string | null {
  const value = input?.trim();
  if (!value) return null;

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }

  const kind = destination === "customerPortal" ? "portal" : "checkout";
  return matchesPolarLinkRule(url, polarLinkRulesFor(environment, kind))
    ? url.href
    : null;
}

export function createPolarPublicLinks(
  config: PolarPublicLinkConfig,
  opener: ExternalOpener = openUrl,
  environment: PolarLinkEnvironment = "production",
): PolarPublicLinks {
  const resolved: Record<PolarDestination, string | null> = {
    desktopCheckout: parsePolarPublicLink(
      "desktopCheckout",
      config.desktopCheckout,
      environment,
    ),
    customerPortal: parsePolarPublicLink(
      "customerPortal",
      config.customerPortal,
      environment,
    ),
  };

  return {
    isConfigured: (destination) =>
      isDestination(destination) && resolved[destination] !== null,
    open: async (destination) => {
      if (!isDestination(destination)) {
        throw new PolarPublicLinkError("invalid_destination");
      }
      const url = resolved[destination];
      if (!url) throw new PolarPublicLinkError("not_configured");
      await opener(url);
    },
  };
}

export function readPolarPublicLinkConfig(
  env: PolarPublicLinkEnvironment,
): PolarPublicLinkConfig {
  return {
    desktopCheckout: env.VITE_POLAR_DESKTOP_CHECKOUT_URL,
    customerPortal: env.VITE_POLAR_CUSTOMER_PORTAL_URL,
  };
}

const buildEnv = import.meta.env as PolarPublicLinkEnvironment;

export const polarPublicLinks = createPolarPublicLinks(
  readPolarPublicLinkConfig(buildEnv),
  openUrl,
  readPolarLinkEnvironment(buildEnv.ALFRED_POLAR_ENVIRONMENT),
);
