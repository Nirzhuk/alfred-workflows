import { useEffect } from "react";
import { useLicenseStore, type LicenseStore } from "../../store";
import { getLicenseBadge } from "../../view-model";

/** The app-wide settings navigation event the workflow canvas already
 * listens for. */
export const OPEN_SETTINGS_EVENT = "alfred:open-settings";

export function openLicenseBillingSettings(target: EventTarget): void {
  target.dispatchEvent(
    new CustomEvent(OPEN_SETTINGS_EVENT, {
      detail: { section: "license-billing" },
    }),
  );
}

type Props = {
  store?: LicenseStore;
};

/** A quiet statement of fact next to the wordmark: is this a free Alfred or a
 * licensed one, and which license. It gates nothing. */
export function LicenseBadge({ store = useLicenseStore }: Props = {}) {
  const status = store((state) => state.status);
  const hasLoaded = store((state) => state.hasLoaded);
  const operation = store((state) => state.operation);
  const load = store((state) => state.load);

  useEffect(() => {
    if (!hasLoaded && operation === null) void load();
  }, [hasLoaded, load, operation]);

  // Nothing is claimed before the local status is read, so the tag appears
  // once rather than flipping from a guessed "Free" to the real answer.
  if (!hasLoaded) return null;

  const badge = getLicenseBadge(status);

  return (
    <button
      type="button"
      className={`titlebar-license is-${badge.tone}`}
      data-license-tier={badge.tier}
      title={`${badge.detail}. Open License & Billing settings.`}
      aria-label={`License: ${badge.label}. ${badge.detail}. Open License & Billing settings.`}
      onClick={() => openLicenseBillingSettings(window)}
    >
      <span className="titlebar-license-label">{badge.label}</span>
    </button>
  );
}
