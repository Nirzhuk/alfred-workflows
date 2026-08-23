import { invoke } from "@tauri-apps/api/core";
import type { LicenseStatus } from "./types";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export type LicensingApi = {
  getStatus: () => Promise<LicenseStatus>;
  activate: (licenseKey: string, deviceLabel: string) => Promise<LicenseStatus>;
  refresh: () => Promise<LicenseStatus>;
  deactivate: () => Promise<LicenseStatus>;
};

export function createLicensingApi(call: Invoke = invoke): LicensingApi {
  return {
    getStatus: () => call("get_license_status"),
    activate: (licenseKey, deviceLabel) =>
      call("activate_license", { licenseKey, deviceLabel }),
    refresh: () => call("refresh_license"),
    deactivate: () => call("deactivate_license"),
  };
}

export const licensingApi = createLicensingApi();
