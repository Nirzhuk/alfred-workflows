import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import type { LicensingApi } from "../../api";
import { createLicenseStore } from "../../store";
import type { LicenseState, LicenseStatus } from "../../types";
import {
  LicenseBadge,
  OPEN_SETTINGS_EVENT,
  openLicenseBillingSettings,
} from "./license-badge";

function status(
  state: LicenseState,
  product: LicenseStatus["product"] = "individual",
): LicenseStatus {
  return {
    product,
    state,
    maskedKey: null,
    benefitId: null,
    activationLabel: null,
    currentDevice: state === "active",
    updateDeadline: null,
    inUpdateWindow: true,
    lastSuccessfulValidation: null,
    nextRefresh: null,
    offlineDeadline: null,
    errorCode: null,
  };
}

const idleApi: LicensingApi = {
  getStatus: async () => status("unlicensed", "none"),
  activate: async () => status("unlicensed", "none"),
  refresh: async () => status("unlicensed", "none"),
  deactivate: async () => status("unlicensed", "none"),
};

function render(snapshot: LicenseStatus | null, hasLoaded = true): string {
  const store = createLicenseStore(idleApi, { status: snapshot, hasLoaded });
  return renderToStaticMarkup(<LicenseBadge store={store} />);
}

describe("license titlebar tag", () => {
  test("stays out of the titlebar until the local status is read", () => {
    expect(render(null, false)).toBe("");
  });

  test("names the license and links it to its settings section", () => {
    const markup = render(status("active"));
    expect(markup).toContain("Alfred License");
    expect(markup).toContain('data-license-tier="licensed"');
    expect(markup).toContain("is-success");
    expect(markup).toContain('aria-label="License: Alfred License.');
    expect(markup).toContain("Open License &amp; Billing settings.");
  });

  test("reads a free build as free, quietly", () => {
    for (const state of ["unlicensed", "notConfigured"] as const) {
      const markup = render(status(state, "none"));
      expect([state, markup.includes(">Free<")]).toEqual([state, true]);
      expect([state, markup.includes('data-license-tier="free"')]).toEqual([
        state,
        true,
      ]);
      expect([state, markup.includes("is-neutral")]).toEqual([state, true]);
      expect([state, markup.includes("is-danger")]).toEqual([state, false]);
      expect([state, markup.includes("is-warning")]).toEqual([state, false]);
    }
  });

  test("never renders a revoked license the way it renders an active one", () => {
    expect(render(status("revoked"))).not.toBe(render(status("active")));
    expect(render(status("revoked"))).toContain(">Revoked<");
    expect(render(status("revoked"))).toContain('data-license-tier="free"');
  });

  test("distinguishes free from licensed without relying on color", () => {
    // The wording alone separates the two classes, so the tag survives a
    // color-blind reader and a forced high-contrast theme.
    const free = render(status("unlicensed", "none"));
    const licensed = render(status("active"));
    expect(free).toContain(">Free<");
    expect(licensed).toContain(">Alfred License<");
  });

  test("clicking routes to the existing License & Billing section", () => {
    const target = new EventTarget();
    let section: string | undefined;
    target.addEventListener(OPEN_SETTINGS_EVENT, (event) => {
      section = (event as CustomEvent<{ section?: string }>).detail?.section;
    });
    openLicenseBillingSettings(target);
    expect(section).toBe("license-billing");
  });
});
