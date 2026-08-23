import type { LicenseProduct, LicenseState, LicenseStatus } from "./types";

export type LicenseStatePresentation = {
  title: string;
  summary: string;
  tone: "neutral" | "success" | "warning" | "danger";
};

export type LicenseStatusNotice = {
  code: string;
  message: string;
};

export const LICENSE_PRODUCT_LABELS: Record<LicenseProduct, string> = {
  none: "No license",
  individual: "Alfred License",
  teams: "Alfred Teams",
};

export const LICENSE_STATE_PRESENTATIONS: Record<
  LicenseState,
  LicenseStatePresentation
> = {
  unlicensed: {
    title: "No license active",
    summary:
      "No license is active on this device. Activate one below if you already purchased Alfred.",
    tone: "neutral",
  },
  active: {
    title: "License active",
    summary: "This device is covered by your license.",
    tone: "success",
  },
  offlineGrace: {
    title: "License active offline",
    summary:
      "Alfred will retry automatically when you are online.",
    tone: "warning",
  },
  needsOnline: {
    title: "Online validation needed",
    summary: "Connect to the internet and refresh to confirm your license.",
    tone: "warning",
  },
  expired: {
    title: "Updates included until renewal",
    summary:
      "Your license stays active and every feature you paid for keeps working. Only newer releases fall outside your update window.",
    tone: "neutral",
  },
  revoked: {
    title: "License revoked",
    summary:
      "This license no longer works on this device. Manage it in Polar or use another key.",
    tone: "danger",
  },
  disabled: {
    title: "License disabled",
    summary:
      "This license is disabled. Check billing in Polar or use another key.",
    tone: "danger",
  },
  deviceLimit: {
    title: "Not active",
    summary: "No license is active on this device.",
    tone: "warning",
  },
  secureStorageUnavailable: {
    title: "Secure storage unavailable",
    summary:
      "Alfred cannot save a license on this device. Check your system keychain and try again.",
    tone: "danger",
  },
  notConfigured: {
    title: "Polar unavailable in this build",
    summary:
      "Licensing is not configured in this build. Local Alfred features remain usable.",
    tone: "warning",
  },
};

export type LicenseStatusBadge = {
  label: "Active" | "Not active";
  tone: "neutral" | "success";
};

const ACTIVE_STATUS_STATES: ReadonlySet<LicenseState> = new Set([
  "active",
  "offlineGrace",
  // A closed update window is not a lapsed license: the purchase still holds.
  "expired",
]);

export function getLicenseStatusBadge(state: LicenseState): LicenseStatusBadge {
  const isActive = ACTIVE_STATUS_STATES.has(state);
  return {
    label: isActive ? "Active" : "Not active",
    tone: isActive ? "success" : "neutral",
  };
}

export type LicenseBadgeTier = "free" | "licensed";

export type LicenseBadge = {
  /** The two classes a reader has to tell apart at a glance. */
  tier: LicenseBadgeTier;
  /** Short visible tag text. */
  label: string;
  /** The honest longer sentence, reused from the state presentation. */
  detail: string;
  tone: LicenseStatePresentation["tone"];
};

/** States in which a license key is currently recognized on this device. A
 * license that was revoked, disabled, never activated, or that this build
 * cannot read is not one of them: those are free builds that also have
 * something to say. `expired` *is* one of them — it means the update window
 * closed, not that the purchase lapsed. Alfred is GPL and fully functional
 * either way — the tag reports which of the two a build is, it never gates
 * anything. */
const LICENSED_BADGE_STATES: ReadonlySet<LicenseState> = new Set([
  "active",
  "offlineGrace",
  "needsOnline",
  "expired",
]);

/** Titlebar-length wording. On a licensed build this qualifies the product
 * label ("Alfred License - Offline"); on a free build it is the whole tag.
 * `active` adds nothing, so a healthy license reads as its product alone. */
const LICENSE_BADGE_QUALIFIERS: Record<LicenseState, string> = {
  unlicensed: "Free",
  // A self-built Alfred is a legitimate, fully functional Alfred. It reads as
  // free like any other unlicensed build — never as an error, never as a nag.
  notConfigured: "Free",
  active: "",
  offlineGrace: "Offline",
  needsOnline: "Verify",
  // Not "Expired": the license did not expire, its update window closed.
  expired: "Updates ended",
  revoked: "Revoked",
  disabled: "Disabled",
  deviceLimit: "Not active",
  secureStorageUnavailable: "Key unavailable",
};

/** The settings page can afford to warn about an unconfigured build; a tag
 * beside the wordmark cannot, so this one state drops to neutral. Every other
 * state keeps the tone the settings presentation already assigns it. */
const NEUTRAL_BADGE_STATES: ReadonlySet<LicenseState> = new Set([
  "notConfigured",
]);

export function getLicenseBadge(status: LicenseStatus | null): LicenseBadge {
  const state = status?.state ?? "unlicensed";
  const presentation = LICENSE_STATE_PRESENTATIONS[state];
  const qualifier = LICENSE_BADGE_QUALIFIERS[state];

  if (!LICENSED_BADGE_STATES.has(state)) {
    return {
      tier: "free",
      label: qualifier,
      detail: presentation.title,
      tone: NEUTRAL_BADGE_STATES.has(state) ? "neutral" : presentation.tone,
    };
  }

  // A licensed build names its license. `none` alongside a licensed state is
  // not expected from Rust, but "No license" would be a lie if it ever were.
  const product =
    status && status.product !== "none"
      ? LICENSE_PRODUCT_LABELS[status.product]
      : "Licensed";

  return {
    tier: "licensed",
    label: qualifier ? `${product} - ${qualifier}` : product,
    detail: presentation.title,
    tone: presentation.tone,
  };
}

const STATUS_NOTICE_MESSAGES: Record<string, string> = {
  invalid_license:
    "Polar did not recognize this license key. Check the key and try again.",
  license_invalid:
    "Polar did not confirm this license. Check it in Polar or try another key.",
  polar_connectivity:
    "Alfred could not reach Polar. The saved license status is unchanged, and Alfred will retry.",
  polar_invalid_response:
    "Alfred could not confirm the license status because Polar returned an invalid response. Try again.",
  polar_rate_limited:
    "Polar is receiving too many requests. The saved license status is unchanged, and Alfred will retry.",
  polar_response_too_large:
    "Polar returned an unexpected response that Alfred could not safely process. Try again.",
  polar_timeout:
    "Polar took too long to respond. The saved license status is unchanged, and Alfred will retry.",
  polar_unavailable:
    "Polar is temporarily unavailable. The saved license status is unchanged, and Alfred will retry.",
  unsupported_product:
    "This license does not include a supported Alfred product. Use an Alfred License or Alfred Teams key.",
};

// These stable Rust DTO codes are intentionally handled by their distinct
// effective-state presentation instead of a duplicate notice:
// - update_window_closed, license_revoked, license_disabled
// - device_limit, online_validation_required
// - secure_storage_unavailable, secure_storage_invalid
// - polar_config_incomplete, polar_environment_invalid,
//   polar_identifier_invalid, polar_api_base_invalid (notConfigured)
const STATE_PRESENTATION_ERROR_CODES = new Set([
  "update_window_closed",
  "license_revoked",
  "license_disabled",
  "device_limit",
  "online_validation_required",
  "secure_storage_unavailable",
  "secure_storage_invalid",
  "polar_config_incomplete",
  "polar_environment_invalid",
  "polar_identifier_invalid",
  "polar_api_base_invalid",
]);

export function getLicenseStatusNotice(
  errorCode: string | null,
): LicenseStatusNotice | null {
  if (!errorCode) return null;
  if (STATE_PRESENTATION_ERROR_CODES.has(errorCode)) return null;
  const message = STATUS_NOTICE_MESSAGES[errorCode];
  return message ? { code: errorCode, message } : null;
}

export function formatLicenseDate(
  value: string,
  locales?: Intl.LocalesArgument,
): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(locales, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
