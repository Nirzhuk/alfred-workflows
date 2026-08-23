/** The two products Alfred sells. Both are one-time purchases. */
export type LicenseProduct = "none" | "individual" | "teams";

export type LicenseState =
  | "unlicensed"
  | "active"
  | "offlineGrace"
  | "needsOnline"
  | "expired"
  | "revoked"
  | "disabled"
  | "deviceLimit"
  | "secureStorageUnavailable"
  | "notConfigured";

/** Redacted licensing state. Secret keys, activation IDs, and credential
 * references are deliberately absent from this frontend contract. */
export type LicenseStatus = {
  product: LicenseProduct;
  state: LicenseState;
  maskedKey: string | null;
  benefitId: string | null;
  activationLabel: string | null;
  currentDevice: boolean;
  /** The last date whose builds this license covers. A date, never a key.
   * `null` is a license that carries no update window at all. */
  updateDeadline: string | null;
  /** Whether the running build was released on or before `updateDeadline`.
   * Rust owns this comparison; the UI never does date maths for it. */
  inUpdateWindow: boolean;
  lastSuccessfulValidation: string | null;
  nextRefresh: string | null;
  offlineDeadline: string | null;
  errorCode: string | null;
};

export type LicenseCommandError = {
  code: string;
  recoverable: boolean;
};
