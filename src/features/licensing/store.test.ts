import { describe, expect, test } from "bun:test";
import type { LicensingApi } from "./api";
import { createLicenseStore, mapLicenseError } from "./store";
import type { LicenseStatus } from "./types";

function status(overrides: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    product: "individual",
    state: "active",
    maskedKey: "••••-SAFE",
    benefitId: "11111111-1111-4111-8111-111111111111",
    activationLabel: "Alfred on macOS",
    currentDevice: true,
    updateDeadline: "2027-08-15T12:00:00Z",
    inUpdateWindow: true,
    lastSuccessfulValidation: "2026-08-15T12:00:00Z",
    nextRefresh: "2026-08-22T12:00:00Z",
    offlineDeadline: "2026-09-14T12:00:00Z",
    errorCode: null,
    ...overrides,
  };
}

function api(overrides: Partial<LicensingApi> = {}): LicensingApi {
  const safeStatus = status();
  return {
    getStatus: async () => safeStatus,
    activate: async () => safeStatus,
    refresh: async () => safeStatus,
    deactivate: async () => safeStatus,
    ...overrides,
  };
}

describe("license store", () => {
  test("reloads the safe snapshot after every mutation", async () => {
    const calls: string[] = [];
    const refreshed = status({ product: "individual" });
    const store = createLicenseStore(
      api({
        getStatus: async () => {
          calls.push("getStatus");
          return refreshed;
        },
        activate: async () => {
          calls.push("activate");
          return status();
        },
        refresh: async () => {
          calls.push("refresh");
          return status();
        },
        deactivate: async () => {
          calls.push("deactivate");
          return status({ state: "unlicensed", product: "none" });
        },
      }),
    );

    await store.getState().activate("secret-fixture", "Alfred on macOS");
    await store.getState().refresh();
    await store.getState().deactivate();

    expect(calls).toEqual([
      "activate",
      "getStatus",
      "refresh",
      "getStatus",
      "deactivate",
      "getStatus",
    ]);
    expect(store.getState().status).toEqual(refreshed);
    expect(JSON.stringify(store.getState())).not.toContain("secret-fixture");
  });

  test("allows only one mutation at a time", async () => {
    let finishActivation: ((value: LicenseStatus) => void) | undefined;
    let refreshCalls = 0;
    const store = createLicenseStore(
      api({
        activate: () =>
          new Promise((resolve) => {
            finishActivation = resolve;
          }),
        refresh: async () => {
          refreshCalls += 1;
          return status();
        },
      }),
    );

    const activation = store
      .getState()
      .activate("secret-fixture", "Alfred on Linux");
    expect(store.getState().operation).toBe("activate");
    expect(await store.getState().refresh()).toBe(false);
    expect(refreshCalls).toBe(0);

    finishActivation?.(status());
    expect(await activation).toBe(true);
    expect(store.getState().operation).toBeNull();
  });

  test("reports a returned activation failure instead of announcing success", async () => {
    const failed = status({
      product: "none",
      state: "deviceLimit",
      maskedKey: "••••-SAFE",
      benefitId: null,
      activationLabel: null,
      currentDevice: false,
      updateDeadline: null,
      inUpdateWindow: true,
      lastSuccessfulValidation: null,
      nextRefresh: null,
      offlineDeadline: null,
      errorCode: "device_limit",
    });
    const store = createLicenseStore(
      api({
        activate: async () => failed,
        getStatus: async () => failed,
      }),
    );

    expect(await store.getState().activate("secret-fixture", "Alfred on macOS")).toBe(
      false,
    );
    expect(store.getState().error).toEqual({
      code: "device_limit",
      message:
        "Polar could not activate this license. Check the license in Polar and try again.",
      recoverable: true,
    });
    expect(store.getState().announcement).toBe(
      "Polar could not activate this license. Check the license in Polar and try again.",
    );
    expect(store.getState().operation).toBeNull();
  });

  test("keeps the previous state when deactivation fails", async () => {
    const previous = status({ state: "offlineGrace" });
    const store = createLicenseStore(
      api({
        deactivate: async () => {
          throw { code: "network_error", recoverable: true };
        },
      }),
    );
    store.setState({ status: previous, hasLoaded: true });

    expect(await store.getState().deactivate()).toBe(false);
    expect(store.getState().status).toBe(previous);
    expect(store.getState().error).toEqual({
      code: "network_error",
      message:
        "Alfred could not reach Polar. Check your connection and try again.",
      recoverable: true,
    });
  });

  test("keeps a completed deactivation when the follow-up local read fails", async () => {
    const completed = status({
      product: "none",
      state: "unlicensed",
      maskedKey: null,
      benefitId: null,
      activationLabel: null,
      currentDevice: false,
      updateDeadline: null,
      inUpdateWindow: true,
      lastSuccessfulValidation: null,
      nextRefresh: null,
      offlineDeadline: null,
    });
    const store = createLicenseStore(
      api({
        getStatus: async () => {
          throw {
            code: "license_state_unavailable",
            recoverable: true,
            detail: "raw-local-detail-secret",
          };
        },
        deactivate: async () => completed,
      }),
      { status: status(), hasLoaded: true },
    );

    expect(await store.getState().deactivate()).toBe(true);
    expect(store.getState().status).toBe(completed);
    expect(store.getState().error).toEqual({
      code: "status_reload_failed",
      message:
        "License deactivated on this device. Alfred could not reload the follow-up local status, so the completed result is shown.",
      recoverable: true,
    });
    expect(store.getState().announcement).toContain(
      "License deactivated on this device.",
    );
    expect(JSON.stringify(store.getState())).not.toContain(
      "raw-local-detail-secret",
    );
  });

  test("does not expose unknown rejection details", () => {
    const mapped = mapLicenseError({
      code: "server_said_secret-fixture",
      recoverable: false,
      detail: "raw response",
    });
    expect(mapped).toEqual({
      code: "unknown",
      message: "Alfred could not update the license. Try again.",
      recoverable: false,
    });
    expect(JSON.stringify(mapped)).not.toContain("raw response");
    expect(JSON.stringify(mapped)).not.toContain("secret-fixture");
  });

  test("maps native Polar availability codes to safe guidance", () => {
    expect(
      mapLicenseError({ code: "polar_connectivity", recoverable: true }),
    ).toEqual({
      code: "polar_connectivity",
      message:
        "Alfred could not reach Polar. Check your connection and try again.",
      recoverable: true,
    });
    expect(
      mapLicenseError({ code: "polar_unavailable", recoverable: true }),
    ).toEqual({
      code: "polar_unavailable",
      message:
        "Polar is temporarily unavailable. Your saved license status is unchanged.",
      recoverable: true,
    });
  });
});
