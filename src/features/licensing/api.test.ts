import { describe, expect, test } from "bun:test";
import { createLicensingApi } from "./api";
import type { LicenseStatus } from "./types";

const safeStatus: LicenseStatus = {
  product: "individual",
  state: "active",
  maskedKey: "••••-CRET",
  benefitId: "11111111-1111-4111-8111-111111111111",
  activationLabel: "Test Device",
  currentDevice: true,
  updateDeadline: "2027-08-15T12:00:00Z",
  lastSuccessfulValidation: "2026-08-15T12:00:00Z",
  nextRefresh: "2026-08-22T12:00:00Z",
  offlineDeadline: "2026-09-14T12:00:00Z",
  errorCode: null,
};

describe("licensing API", () => {
  test("invokes the four licensing commands with only activation input", async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const fakeInvoke = async <T>(
      command: string,
      args?: Record<string, unknown>,
    ): Promise<T> => {
      calls.push({ command, args });
      return safeStatus as T;
    };
    const api = createLicensingApi(fakeInvoke);

    await api.getStatus();
    await api.activate("TEST-LICENSE-KEY-SECRET", "Test Device");
    await api.refresh();
    await api.deactivate();

    expect(calls).toEqual([
      { command: "get_license_status", args: undefined },
      {
        command: "activate_license",
        args: {
          licenseKey: "TEST-LICENSE-KEY-SECRET",
          deviceLabel: "Test Device",
        },
      },
      { command: "refresh_license", args: undefined },
      { command: "deactivate_license", args: undefined },
    ]);
  });

  test("status DTO exposes no stored secret or opaque reference", () => {
    const keys = Object.keys(safeStatus);
    expect(keys).not.toContain("licenseKey");
    expect(keys).not.toContain("activationId");
    expect(keys).not.toContain("credentialRef");
    expect(JSON.stringify(safeStatus)).not.toContain("TEST-LICENSE-KEY-SECRET");
  });
});
