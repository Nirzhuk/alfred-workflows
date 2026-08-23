import { describe, expect, test } from "bun:test";
import {
  formatLicenseDate,
  getLicenseBadge,
  getLicenseStatusBadge,
  getLicenseStatusNotice,
  LICENSE_PRODUCT_LABELS,
  LICENSE_STATE_PRESENTATIONS,
} from "./view-model";
import type { LicenseProduct, LicenseState, LicenseStatus } from "./types";

describe("license presentation", () => {
  test("defines customer guidance for every effective state", () => {
    const states: LicenseState[] = [
      "unlicensed",
      "active",
      "offlineGrace",
      "needsOnline",
      "expired",
      "revoked",
      "disabled",
      "deviceLimit",
      "secureStorageUnavailable",
      "notConfigured",
    ];

    expect(Object.keys(LICENSE_STATE_PRESENTATIONS).sort()).toEqual(
      [...states].sort(),
    );
    for (const state of states) {
      expect(LICENSE_STATE_PRESENTATIONS[state].title.length).toBeGreaterThan(0);
      expect(LICENSE_STATE_PRESENTATIONS[state].summary.length).toBeGreaterThan(
        0,
      );
    }
  });

  test("keeps product identity separate from effective state", () => {
    const products: LicenseProduct[] = ["none", "individual", "teams"];
    expect(Object.keys(LICENSE_PRODUCT_LABELS).sort()).toEqual(
      [...products].sort(),
    );
    expect(LICENSE_PRODUCT_LABELS.individual).toBe("Alfred License");
    expect(LICENSE_PRODUCT_LABELS.teams).toBe("Alfred Teams");
  });

  test("keeps the customer-facing status badge binary", () => {
    // A closed update window is not a lapsed license, so `expired` reads as
    // active here alongside the two connectivity-healthy states.
    const activeStates: LicenseState[] = ["active", "offlineGrace", "expired"];
    const inactiveStates = ALL_STATES.filter(
      (state) => !activeStates.includes(state),
    );

    for (const state of activeStates) {
      expect(getLicenseStatusBadge(state)).toEqual({
        label: "Active",
        tone: "success",
      });
    }
    for (const state of inactiveStates) {
      expect(getLicenseStatusBadge(state)).toEqual({
        label: "Not active",
        tone: "neutral",
      });
    }
  });

  test("formats valid dates by locale and rejects invalid input", () => {
    expect(formatLicenseDate("2026-08-15T12:00:00Z", "en-GB")).toContain(
      "15 Aug 2026",
    );
    expect(formatLicenseDate("not-a-date", "en-GB")).toBeNull();
  });

  test("allow-lists safe snapshot notices and ignores unknown detail", () => {
    const visibleCodes = [
      "invalid_license",
      "license_invalid",
      "unsupported_product",
      "polar_unavailable",
      "polar_connectivity",
      "polar_timeout",
      "polar_rate_limited",
      "polar_invalid_response",
      "polar_response_too_large",
    ];
    for (const code of visibleCodes) {
      const notice = getLicenseStatusNotice(code);
      expect(notice?.code).toBe(code);
      expect(notice?.message.length).toBeGreaterThan(0);
    }

    for (const code of [
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
    ]) {
      expect(getLicenseStatusNotice(code)).toBeNull();
    }
    expect(getLicenseStatusNotice("provider-secret-detail")).toBeNull();
    expect(getLicenseStatusNotice(null)).toBeNull();
  });
});

const ALL_PRODUCTS: LicenseProduct[] = ["none", "individual", "teams"];

const ALL_STATES: LicenseState[] = [
  "unlicensed",
  "active",
  "offlineGrace",
  "needsOnline",
  "expired",
  "revoked",
  "disabled",
  "deviceLimit",
  "secureStorageUnavailable",
  "notConfigured",
];

/** `expired` belongs here: the update window closed, the purchase did not. */
const LICENSED_STATES: LicenseState[] = [
  "active",
  "offlineGrace",
  "needsOnline",
  "expired",
];

function statusOf(product: LicenseProduct, state: LicenseState): LicenseStatus {
  return {
    product,
    state,
    maskedKey: null,
    benefitId: null,
    activationLabel: null,
    currentDevice: false,
    updateDeadline: null,
    inUpdateWindow: true,
    lastSuccessfulValidation: null,
    nextRefresh: null,
    offlineDeadline: null,
    errorCode: null,
  };
}

describe("license titlebar badge", () => {
  test("classifies every product and state combination", () => {
    const seen: string[] = [];
    for (const product of ALL_PRODUCTS) {
      for (const state of ALL_STATES) {
        const badge = getLicenseBadge(statusOf(product, state));
        const licensed = LICENSED_STATES.includes(state);

        expect([product, state, badge.tier]).toEqual([
          product,
          state,
          licensed ? "licensed" : "free",
        ]);
        expect([product, state, badge.label.length > 0]).toEqual([
          product,
          state,
          true,
        ]);
        expect([product, state, badge.detail]).toEqual([
          product,
          state,
          LICENSE_STATE_PRESENTATIONS[state].title,
        ]);
        expect(["neutral", "success", "warning", "danger"]).toContain(
          badge.tone,
        );
        seen.push(`${product}/${state}`);
      }
    }
    expect(seen.length).toBe(ALL_PRODUCTS.length * ALL_STATES.length);
  });

  test("names the license a licensed build actually holds", () => {
    for (const product of ["individual", "teams"] as const) {
      const badge = getLicenseBadge(statusOf(product, "active"));
      expect(badge.label).toBe(LICENSE_PRODUCT_LABELS[product]);
      expect(badge.tier).toBe("licensed");
      expect(badge.tone).toBe("success");
    }

    // "No license" would be a lie on a licensed state, so `none` degrades to a
    // truthful generic instead of the product label.
    expect(getLicenseBadge(statusOf("none", "active")).label).toBe("Licensed");
  });

  test("keeps a degraded license visibly apart from a healthy one", () => {
    for (const product of ALL_PRODUCTS) {
      const active = getLicenseBadge(statusOf(product, "active"));
      for (const state of ALL_STATES) {
        if (state === "active") continue;
        const badge = getLicenseBadge(statusOf(product, state));
        expect([product, state, badge.label === active.label]).toEqual([
          product,
          state,
          false,
        ]);
      }
    }
  });

  test("qualifies a license that is still granted but not fully validated", () => {
    expect(getLicenseBadge(statusOf("individual", "offlineGrace"))).toEqual({
      tier: "licensed",
      label: "Alfred License - Offline",
      detail: "License active offline",
      tone: "warning",
    });
    expect(getLicenseBadge(statusOf("teams", "needsOnline"))).toEqual({
      tier: "licensed",
      label: "Alfred Teams - Verify",
      detail: "Online validation needed",
      tone: "warning",
    });
  });

  /** Plan 007: a lapsed update window is not a lost license. The tag must
   * still name the product the customer bought, never demote them to a free
   * build for owning an older set of releases. */
  test("keeps a closed update window on the licensed side of the tag", () => {
    for (const product of ["individual", "teams"] as const) {
      const badge = getLicenseBadge(statusOf(product, "expired"));
      expect([product, badge.tier]).toEqual([product, "licensed"]);
      expect([product, badge.label]).toEqual([
        product,
        `${LICENSE_PRODUCT_LABELS[product]} - Updates ended`,
      ]);
      // Never a danger tone: nothing the customer paid for was taken away.
      expect([product, badge.tone]).toEqual([product, "neutral"]);
    }
    // The two verdicts that DO end entitlement stay on the free side.
    for (const state of ["revoked", "disabled"] as const) {
      expect([state, getLicenseBadge(statusOf("individual", state)).tier]).toEqual(
        [state, "free"],
      );
    }
  });

  test("states a lost or blocked license honestly instead of silently", () => {
    const expected: Partial<Record<LicenseState, { label: string; tone: string }>> = {
      revoked: { label: "Revoked", tone: "danger" },
      disabled: { label: "Disabled", tone: "danger" },
      deviceLimit: { label: "Not active", tone: "warning" },
      secureStorageUnavailable: { label: "Key unavailable", tone: "danger" },
    };
    for (const [state, want] of Object.entries(expected)) {
      const badge = getLicenseBadge(
        statusOf("individual", state as LicenseState),
      );
      expect([state, badge.tier]).toEqual([state, "free"]);
      expect([state, badge.label]).toEqual([state, want.label]);
      expect([state, badge.tone]).toEqual([state, want.tone]);
    }
  });

  test("reads a self-built Alfred as free without alarming the user", () => {
    // GPL self-built use is legitimate and fully functional. The tag states a
    // fact; the warning tone the settings page uses would read as a fault.
    for (const product of ALL_PRODUCTS) {
      const badge = getLicenseBadge(statusOf(product, "notConfigured"));
      expect([product, badge.tier]).toEqual([product, "free"]);
      expect([product, badge.label]).toEqual([product, "Free"]);
      expect([product, badge.tone]).toEqual([product, "neutral"]);
    }
    expect(LICENSE_STATE_PRESENTATIONS.notConfigured.tone).toBe("warning");

    // Indistinguishable from any other unlicensed build, by design.
    expect(getLicenseBadge(statusOf("none", "notConfigured")).label).toBe(
      getLicenseBadge(statusOf("none", "unlicensed")).label,
    );
  });

  test("treats an unread status as a plain free build", () => {
    expect(getLicenseBadge(null)).toEqual({
      tier: "free",
      label: "Free",
      detail: "No license active",
      tone: "neutral",
    });
  });
});
