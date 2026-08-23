import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import type { LicensingApi } from "../../api";
import { createPolarPublicLinks } from "../../public-links";
import { createLicenseStore } from "../../store";
import type { LicenseState, LicenseStatus } from "../../types";
import {
  getLicenseStatusBadge,
  LICENSE_STATE_PRESENTATIONS,
} from "../../view-model";
import {
  activateAndClearLicenseKey,
  defaultLicenseDeviceLabel,
  LICENSE_KEY_INPUT_ATTRIBUTES,
  LicenseSettings,
} from "./license-settings";

function status(
  state: LicenseState,
  overrides: Partial<LicenseStatus> = {},
): LicenseStatus {
  return {
    product: state === "unlicensed" ? "none" : "teams",
    state,
    maskedKey: state === "unlicensed" ? null : "••••-SAFE",
    benefitId: null,
    activationLabel: state === "unlicensed" ? null : "Alfred on macOS",
    currentDevice: [
      "active",
      "offlineGrace",
      "needsOnline",
      "expired",
      "revoked",
      "disabled",
    ].includes(state),
    updateDeadline: "2027-08-15T12:00:00Z",
    inUpdateWindow: true,
    lastSuccessfulValidation: "2026-08-15T12:00:00Z",
    nextRefresh: null,
    offlineDeadline: "2026-09-14T12:00:00Z",
    errorCode: null,
    ...overrides,
  };
}

function api(snapshot: LicenseStatus): LicensingApi {
  return {
    getStatus: async () => snapshot,
    activate: async () => snapshot,
    refresh: async () => snapshot,
    deactivate: async () => snapshot,
  };
}

const missingLinks = createPolarPublicLinks(
  {
    desktopCheckout: undefined,
    customerPortal: undefined,
  },
  async () => {},
);

function renderStatus(snapshot: LicenseStatus): string {
  const store = createLicenseStore(api(snapshot), {
    status: snapshot,
    hasLoaded: true,
  });
  return renderToStaticMarkup(
    <LicenseSettings store={store} links={missingLinks} platform="macos" />,
  );
}

function renderState(state: LicenseState): string {
  return renderStatus(status(state));
}

describe("license settings", () => {
  test("renders distinct guidance for every license state", () => {
    for (const state of Object.keys(
      LICENSE_STATE_PRESENTATIONS,
    ) as LicenseState[]) {
      const markup = renderState(state);
      expect(markup).toContain(LICENSE_STATE_PRESENTATIONS[state].summary);
      expect(markup).toContain(`data-license-state="${state}"`);
      expect(markup).toContain(getLicenseStatusBadge(state).label);
    }
  });

  test("renders a concise plan summary with accessible exact dates", () => {
    const markup = renderState("offlineGrace");
    expect(markup).toContain("Current plan");
    expect(markup).toContain("Alfred Teams");
    expect(markup).toContain(">Active</span>");
    expect(markup).toContain('dateTime="2026-09-14T12:00:00Z"');
    expect(markup).toContain(
      "Offline access deadline: 2026-09-14T12:00:00Z",
    );
    expect(markup).toContain("Works offline until");
    expect(markup).not.toContain("Effective state");
    expect(markup).not.toContain("Last validated");
    expect(markup).not.toContain("TEST-LICENSE-KEY-SECRET");
  });

  test("keeps an inactive license state simple and provider-neutral", () => {
    const markup = renderStatus(
      status("deviceLimit", { product: "none", currentDevice: false }),
    );

    expect(markup).toContain("Current plan");
    expect(markup).toContain("No active license");
    expect(markup).toContain(">Not active</span>");
    expect(markup).toContain("No license is active on this device.");
    expect(markup).not.toContain("License found");
    expect(markup).not.toContain("License status");
    expect(markup).not.toContain("Device limit reached");
  });

  test("renders all licensed products independently of effective state", () => {
    const products = [
      ["individual", "Alfred License"],
      ["teams", "Alfred Teams"],
    ] as const;

    for (const [product, label] of products) {
      const markup = renderStatus(status("active", { product }));
      expect(markup).toContain("Current plan");
      expect(markup).toContain(">Active</span>");
      expect(markup).toContain(label);
    }
  });

  test("describes the deadline as the end of included updates, not of the license", () => {
    const markup = renderStatus(
      status("active", {
        product: "individual",
        updateDeadline: "2027-08-15T12:00:00Z",
      }),
    );

    expect(markup).toContain("Alfred License");
    expect(markup).toContain("Updates until");
    expect(markup).toContain("Updates available until");
    expect(markup).toContain("Your license never expires.");
    expect(markup).toContain("this build is inside that window");
    expect(markup).not.toContain("License expiry");
  });

  test("keeps provider refresh metadata out of the customer view", () => {
    const withNextCheck = renderStatus(
      status("active", {
        updateDeadline: null,
        nextRefresh: "2026-08-22T12:00:00Z",
      }),
    );
    expect(withNextCheck).not.toContain("Next check");
    expect(withNextCheck).not.toContain("2026-08-22T12:00:00Z");
    expect(withNextCheck).not.toContain("Last validated");

    const withoutNextCheck = renderStatus(
      status("active", { updateDeadline: null, nextRefresh: null }),
    );
    expect(withoutNextCheck).not.toContain("Next check");
    expect(withoutNextCheck).not.toContain("Expires");
  });

  test("shows the loading view before the first local status arrives", () => {
    const snapshot = status("active");
    const store = createLicenseStore(api(snapshot), { operation: "load" });
    const markup = renderToStaticMarkup(
      <LicenseSettings store={store} links={missingLinks} platform="macos" />,
    );
    expect(markup).toContain("Loading local license status...");
    expect(markup).not.toContain("Effective state");
  });

  test("shows visible feedback while a license activation is in progress", () => {
    const snapshot = status("unlicensed");
    const store = createLicenseStore(api(snapshot), {
      status: snapshot,
      hasLoaded: true,
      operation: "activate",
    });
    const markup = renderToStaticMarkup(
      <LicenseSettings store={store} links={missingLinks} platform="macos" />,
    );

    expect(markup).toContain("Activating...");
    expect(markup).toContain("Checking license...");
    expect(markup).toContain('id="license-key-feedback"');
  });

  test("shows safe transient DTO guidance without replacing saved state", () => {
    const messages = {
      polar_unavailable:
        "Polar is temporarily unavailable. The saved license status is unchanged, and Alfred will retry.",
      polar_connectivity:
        "Alfred could not reach Polar. The saved license status is unchanged, and Alfred will retry.",
      polar_timeout:
        "Polar took too long to respond. The saved license status is unchanged, and Alfred will retry.",
      polar_rate_limited:
        "Polar is receiving too many requests. The saved license status is unchanged, and Alfred will retry.",
    } as const;

    for (const [errorCode, message] of Object.entries(messages)) {
      const markup = renderStatus(status("offlineGrace", { errorCode }));
      expect(markup).toContain(">Active</span>");
      expect(markup).toContain(message);
      expect(markup).toContain("Works offline until");
      expect(markup).toContain("license-status-notice");
    }

    const unknown = renderStatus(
      status("active", { errorCode: "raw-provider-detail-secret" }),
    );
    expect(unknown).not.toContain("raw-provider-detail-secret");
  });

  test("shows safe activation outcome guidance returned in successful DTOs", () => {
    const outcomes = {
      invalid_license:
        "Polar did not recognize this license key. Check the key and try again.",
      unsupported_product:
        "This license does not include a supported Alfred product. Use an Alfred License or Alfred Teams key.",
      polar_invalid_response:
        "Alfred could not confirm the license status because Polar returned an invalid response. Try again.",
      polar_response_too_large:
        "Polar returned an unexpected response that Alfred could not safely process. Try again.",
    } as const;

    for (const [errorCode, message] of Object.entries(outcomes)) {
      const markup = renderStatus(
        status("unlicensed", {
          errorCode,
          maskedKey: "••••-SAFE",
          currentDevice: false,
        }),
      );
      expect(markup).toContain(">Not active</span>");
      expect(markup).toContain(message);
      expect(markup).toContain("Have a license key?");
    }

    const unknown = renderStatus(
      status("unlicensed", {
        errorCode: "raw-provider-secret-detail",
        maskedKey: "••••-SAFE",
      }),
    );
    expect(unknown).not.toContain("raw-provider-secret-detail");
  });

  test("offline and unconfigured states explain retry and local usability", () => {
    const offline = renderState("offlineGrace");
    expect(offline).toContain("will retry automatically");
    expect(offline).toContain("Works offline until");

    const notConfigured = renderState("notConfigured");
    expect(notConfigured).toContain("Local Alfred features remain usable");
  });

  test("offers device lifecycle actions and accurate deactivation wording", () => {
    const markup = renderState("active");
    expect(markup).toContain("Refresh");
    expect(markup).toContain("Deactivate this device");
    expect(markup).toContain("Manage billing");
  });

  test("allows replacement activation only when a failed license has no current device", () => {
    for (const state of ["expired", "revoked", "disabled"] as const) {
      const withoutCredential = renderStatus(
        status(state, { currentDevice: false }),
      );
      expect(withoutCredential).toContain("Have a license key?");
      expect(withoutCredential).not.toContain("Deactivate this device");

      const withCredential = renderStatus(
        status(state, { currentDevice: true }),
      );
      expect(withCredential).not.toContain("Have a license key?");
      expect(withCredential).toContain("Deactivate this device");
      expect(withCredential).toContain("Refresh");
    }

    expect(
      renderStatus(status("notConfigured", { currentDevice: false })),
    ).not.toContain("Have a license key?");
  });

  test("leaves license-key paste and password-manager custody available", () => {
    const markup = renderState("unlicensed");
    const licenseInput = markup.match(
      /<input[^>]*id="license-key-input"[^>]*>/,
    )?.[0];
    expect(licenseInput).toBeDefined();
    expect(licenseInput).not.toContain("autocomplete");
    expect(licenseInput).not.toContain("onpaste");
    expect("autoComplete" in LICENSE_KEY_INPUT_ATTRIBUTES).toBe(false);
    expect("onPaste" in LICENSE_KEY_INPUT_ATTRIBUTES).toBe(false);
  });

  test("disables unconfigured hosted destinations", () => {
    const markup = renderState("unlicensed");
    expect(markup).toContain("Buy Desktop");
    // Company plans are sold on the marketing website, so the app offers no
    // Company purchase path at all.
    expect(markup).not.toContain("Buy for a Company");
    expect(markup).toContain(
      "Licensing checkout is not configured in this build.",
    );
    expect(markup).toContain(
      "The Polar customer portal is not configured in this build.",
    );
    expect(markup.match(/disabled=""/g)?.length).toBeGreaterThanOrEqual(2);
  });

  test("uses non-identifying default labels for every platform", () => {
    expect(defaultLicenseDeviceLabel("macos")).toBe("Alfred on macOS");
    expect(defaultLicenseDeviceLabel("windows")).toBe("Alfred on Windows");
    expect(defaultLicenseDeviceLabel("linux")).toBe("Alfred on Linux");
    expect(defaultLicenseDeviceLabel("unknown")).toBe("Alfred on desktop");
  });

  test("clears the activation key after success and failure", async () => {
    for (const result of [true, false]) {
      let clearCalls = 0;
      const received: string[] = [];
      expect(
        await activateAndClearLicenseKey(
          "TEST-LICENSE-KEY-SECRET",
          "Alfred on macOS",
          async (key) => {
            received.push(key);
            return result;
          },
          () => {
            clearCalls += 1;
          },
        ),
      ).toBe(result);
      expect(received).toEqual(["TEST-LICENSE-KEY-SECRET"]);
      expect(clearCalls).toBe(1);
    }

    let clearCalls = 0;
    await expect(
      activateAndClearLicenseKey(
        "TEST-LICENSE-KEY-SECRET",
        "Alfred on macOS",
        async () => {
          throw new Error("offline");
        },
        () => {
          clearCalls += 1;
        },
      ),
    ).rejects.toThrow("offline");
    expect(clearCalls).toBe(1);
  });
});
