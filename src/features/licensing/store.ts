import { create, type StoreApi, type UseBoundStore } from "zustand";
import { licensingApi, type LicensingApi } from "./api";
import type { LicenseCommandError, LicenseStatus } from "./types";

export type LicenseOperation =
  | "load"
  | "activate"
  | "refresh"
  | "deactivate";

export type LicenseUiError = {
  code: string;
  message: string;
  recoverable: boolean;
};

export type LicenseStoreState = {
  status: LicenseStatus | null;
  hasLoaded: boolean;
  operation: LicenseOperation | null;
  error: LicenseUiError | null;
  announcement: string;
  load: () => Promise<boolean>;
  activate: (licenseKey: string, deviceLabel: string) => Promise<boolean>;
  refresh: () => Promise<boolean>;
  deactivate: () => Promise<boolean>;
  clearError: () => void;
};

export type LicenseStore = UseBoundStore<StoreApi<LicenseStoreState>>;

type LicenseStoreInitialState = Partial<
  Pick<
    LicenseStoreState,
    "status" | "hasLoaded" | "operation" | "error" | "announcement"
  >
>;

const ERROR_MESSAGES: Record<string, string> = {
  device_limit:
    "Polar could not activate this license. Check the license in Polar and try again.",
  invalid_input: "Enter a license key and a device label.",
  invalid_license: "Polar did not recognize this license key.",
  activations_unsupported:
    "Polar could not validate this license setup. Check the license benefit in Polar and try again.",
  license_already_active:
    "A license is already active on this device. Deactivate it before using another key.",
  license_state_unavailable:
    "Alfred could not read the local license state. Try again.",
  network_error:
    "Alfred could not reach Polar. Check your connection and try again.",
  polar_connectivity:
    "Alfred could not reach Polar. Check your connection and try again.",
  polar_invalid_response:
    "Polar returned an invalid response. Wait a moment and try again.",
  polar_not_configured: "Polar licensing is not configured in this build.",
  polar_rate_limited:
    "Polar is receiving too many requests. Wait a moment and try again.",
  polar_response_too_large:
    "Polar returned an unexpected response. Wait a moment and try again.",
  polar_timeout: "Polar took too long to respond. Try again when you are online.",
  polar_unavailable:
    "Polar is temporarily unavailable. Your saved license status is unchanged.",
  request_failed:
    "Alfred could not reach Polar. Check your connection and try again.",
  secure_storage_invalid:
    "The saved license could not be read from secure storage. Deactivate it before trying another key.",
  secure_storage_unavailable:
    "Secure storage is unavailable. Alfred cannot safely save a license key on this device.",
  timeout: "Polar took too long to respond. Try again when you are online.",
};

function isLicenseCommandError(value: unknown): value is LicenseCommandError {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<LicenseCommandError>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.recoverable === "boolean"
  );
}

export function mapLicenseError(error: unknown): LicenseUiError {
  if (!isLicenseCommandError(error)) {
    return {
      code: "unknown",
      message: "Alfred could not update the license. Try again.",
      recoverable: true,
    };
  }

  return {
    code: Object.prototype.hasOwnProperty.call(ERROR_MESSAGES, error.code)
      ? error.code
      : "unknown",
    message:
      ERROR_MESSAGES[error.code] ??
      "Alfred could not update the license. Try again.",
    recoverable: error.recoverable,
  };
}

const OPERATION_MESSAGES: Record<
  Exclude<LicenseOperation, "load">,
  { pending: string; success: string }
> = {
  activate: {
    pending: "Activating license...",
    success: "License activated on this device.",
  },
  refresh: {
    pending: "Refreshing license...",
    success: "License status refreshed.",
  },
  deactivate: {
    pending: "Deactivating license...",
    success: "License deactivated on this device.",
  },
};

export function createLicenseStore(
  api: LicensingApi = licensingApi,
  initial: LicenseStoreInitialState = {},
): LicenseStore {
  return create<LicenseStoreState>((set, get) => {
    async function runMutation(
      operation: Exclude<LicenseOperation, "load">,
      command: () => Promise<LicenseStatus>,
    ): Promise<boolean> {
      if (get().operation !== null) return false;

      const messages = OPERATION_MESSAGES[operation];
      set({ operation, error: null, announcement: messages.pending });

      let commandStatus: LicenseStatus;
      try {
        commandStatus = await command();
      } catch (error) {
        const mapped = mapLicenseError(error);
        set({ error: mapped, announcement: mapped.message, operation: null });
        return false;
      }

      set({ status: commandStatus, hasLoaded: true });
      const commandFailure =
        operation === "activate" && commandStatus.errorCode
          ? mapLicenseError({
              code: commandStatus.errorCode,
              recoverable: true,
            })
          : null;

      try {
        const status = await api.getStatus();

        const returnedFailure =
          commandFailure ||
          (operation === "activate" && status.errorCode
            ? mapLicenseError({
                code: status.errorCode,
                recoverable: true,
              })
            : null);
        if (returnedFailure) {
          set({
            status,
            hasLoaded: true,
            error: returnedFailure,
            announcement: returnedFailure.message,
            operation: null,
          });
          return false;
        }

        set({ status, hasLoaded: true, announcement: messages.success });
        return true;
      } catch {
        if (commandFailure) {
          set({
            error: commandFailure,
            announcement: commandFailure.message,
            operation: null,
          });
          return false;
        }

        const warning: LicenseUiError = {
          code: "status_reload_failed",
          message: `${messages.success} Alfred could not reload the follow-up local status, so the completed result is shown.`,
          recoverable: true,
        };
        set({ error: warning, announcement: warning.message });
        return true;
      } finally {
        set({ operation: null });
      }
    }

    return {
      status: initial.status ?? null,
      hasLoaded: initial.hasLoaded ?? false,
      operation: initial.operation ?? null,
      error: initial.error ?? null,
      announcement: initial.announcement ?? "",

      load: async () => {
        if (get().operation !== null) return false;
        set({ operation: "load", error: null, announcement: "" });
        try {
          const status = await api.getStatus();
          set({ status, hasLoaded: true });
          return true;
        } catch (error) {
          const mapped = mapLicenseError(error);
          set({ error: mapped, hasLoaded: true, announcement: mapped.message });
          return false;
        } finally {
          set({ operation: null });
        }
      },

      activate: (licenseKey, deviceLabel) =>
        runMutation("activate", () => api.activate(licenseKey, deviceLabel)),
      refresh: () => runMutation("refresh", api.refresh),
      deactivate: () => runMutation("deactivate", api.deactivate),
      clearError: () => set({ error: null }),
    };
  });
}

export const useLicenseStore = createLicenseStore();
