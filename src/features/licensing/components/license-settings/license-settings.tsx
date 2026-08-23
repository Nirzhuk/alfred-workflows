import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import {
  detectDesktopPlatform,
  type DesktopPlatform,
} from "../../../../platform";
import {
  polarPublicLinks,
  PolarPublicLinkError,
  type PolarDestination,
  type PolarPublicLinks,
} from "../../public-links";
import {
  useLicenseStore,
  type LicenseOperation,
  type LicenseStore,
} from "../../store";
import type { LicenseState, LicenseStatus } from "../../types";
import {
  formatLicenseDate,
  getLicenseStatusBadge,
  getLicenseStatusNotice,
  LICENSE_PRODUCT_LABELS,
  LICENSE_STATE_PRESENTATIONS,
} from "../../view-model";

type Props = {
  store?: LicenseStore;
  links?: PolarPublicLinks;
  platform?: DesktopPlatform;
};

type LicenseDateProps = {
  label: string;
  value: string;
};

const ACTIVATION_STATES = new Set<LicenseState>([
  "unlicensed",
  "deviceLimit",
  "secureStorageUnavailable",
  "expired",
  "revoked",
  "disabled",
]);

export const LICENSE_KEY_INPUT_ATTRIBUTES = {
  type: "password",
  name: "licenseKey",
  spellCheck: false,
} as const;

export function defaultLicenseDeviceLabel(platform: DesktopPlatform): string {
  const platformName: Record<DesktopPlatform, string> = {
    macos: "macOS",
    windows: "Windows",
    linux: "Linux",
    unknown: "desktop",
  };
  return `Alfred on ${platformName[platform]}`;
}

export async function activateAndClearLicenseKey(
  licenseKey: string,
  deviceLabel: string,
  activate: (key: string, label: string) => Promise<boolean>,
  clear: () => void,
): Promise<boolean> {
  try {
    return await activate(licenseKey, deviceLabel);
  } finally {
    clear();
  }
}

function LicenseDate({ label, value }: LicenseDateProps) {
  const formatted = formatLicenseDate(value);
  if (!formatted) return <span>Unavailable</span>;

  return (
    <time dateTime={value} title={value}>
      <span aria-hidden="true">{formatted}</span>
      <span className="sr-only">
        {label}: {value}
      </span>
    </time>
  );
}

function operationLabel(operation: LicenseOperation | null): string | null {
  switch (operation) {
    case "load":
      return "Loading local license status...";
    case "activate":
      return "Activating license...";
    case "refresh":
      return "Refreshing license...";
    case "deactivate":
      return "Deactivating license...";
    default:
      return null;
  }
}

function LicenseStatusCard({
  status,
  actions,
}: {
  status: LicenseStatus;
  actions?: ReactNode;
}) {
  const presentation = LICENSE_STATE_PRESENTATIONS[status.state];
  const statusBadge = getLicenseStatusBadge(status.state);
  const statusNotice = getLicenseStatusNotice(status.errorCode);
  const plan =
    status.product === "none"
      ? status.state === "unlicensed"
        ? "Free"
        : "No active license"
      : LICENSE_PRODUCT_LABELS[status.product];
  const showUpdateDeadline =
    status.product !== "none" && Boolean(status.updateDeadline);
  const showOfflineDeadline =
    status.state === "offlineGrace" && Boolean(status.offlineDeadline);

  return (
    <section
      className="settings-section license-overview-section"
      aria-labelledby="license-overview-heading"
    >
      <div className="license-overview-card">
        <div className="license-overview-top">
          <div>
            <p className="license-overview-label">Current plan</p>
            <h2 id="license-overview-heading">{plan}</h2>
            <p className="license-overview-copy">{presentation.summary}</p>
          </div>
          <span
            className={`license-status-badge is-${statusBadge.tone}`}
            data-license-state={status.state}
          >
            {statusBadge.label}
          </span>
        </div>
        {showUpdateDeadline || showOfflineDeadline ? (
          <dl className="license-overview-details">
            {showUpdateDeadline ? (
              <div>
                <dt>Updates until</dt>
                <dd>
                  <LicenseDate
                    label="Updates available until"
                    value={status.updateDeadline!}
                  />
                </dd>
              </div>
            ) : null}
            {showOfflineDeadline ? (
              <div>
                <dt>Works offline until</dt>
                <dd>
                  <LicenseDate
                    label="Offline access deadline"
                    value={status.offlineDeadline!}
                  />
                </dd>
              </div>
            ) : null}
          </dl>
        ) : null}
        {showUpdateDeadline ? (
          <p className="license-overview-details-note">
            {status.inUpdateWindow
              ? "Your license never expires. This date only bounds which releases it covers, and this build is inside that window."
              : "Your license never expires, but this release came out after your update window closed. An earlier build keeps every feature you paid for."}
          </p>
        ) : null}
        {actions ? (
          <div className="license-overview-actions">{actions}</div>
        ) : null}
      </div>
      {statusNotice ? (
        <div
          className="license-status-notice license-overview-notice"
          role="status"
          aria-live="polite"
        >
          {statusNotice.message}
        </div>
      ) : null}
    </section>
  );
}

export function LicenseSettings({
  store = useLicenseStore,
  links = polarPublicLinks,
  platform = detectDesktopPlatform(),
}: Props) {
  const status = store((state) => state.status);
  const hasLoaded = store((state) => state.hasLoaded);
  const operation = store((state) => state.operation);
  const error = store((state) => state.error);
  const announcement = store((state) => state.announcement);
  const load = store((state) => state.load);
  const activate = store((state) => state.activate);
  const refresh = store((state) => state.refresh);
  const deactivate = store((state) => state.deactivate);
  const clearError = store((state) => state.clearError);

  const [licenseKey, setLicenseKey] = useState("");
  const [deviceLabel, setDeviceLabel] = useState(() =>
    defaultLicenseDeviceLabel(platform),
  );
  const [confirmingDeactivation, setConfirmingDeactivation] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);
  const deactivateButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!hasLoaded && operation === null) void load();
  }, [hasLoaded, load, operation]);

  const busy = operation !== null;
  const canActivate = Boolean(
    status && ACTIVATION_STATES.has(status.state) && !status.currentDevice,
  );
  const canManageActiveLicense = status?.currentDevice === true;
  const pendingLabel = operationLabel(operation);
  const hasLicenseKey = licenseKey.trim().length > 0;
  const showLicenseKeyFeedback = operation === "activate" || hasLicenseKey;

  const returnFocus = () => {
    window.requestAnimationFrame(() => {
      const target =
        deactivateButtonRef.current ??
        document.getElementById("license-overview-heading");
      target?.focus();
    });
  };

  const handleActivation = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    clearError();
    const key = licenseKey.trim();
    const label = deviceLabel.trim();
    if (!key || !label || busy) return;

    await activateAndClearLicenseKey(key, label, activate, () =>
      setLicenseKey(""),
    );
  };

  const openDestination = async (destination: PolarDestination) => {
    setLinkError(null);
    try {
      await links.open(destination);
    } catch (openError) {
      setLinkError(
        openError instanceof PolarPublicLinkError
          ? openError.message
          : "Alfred could not open Polar in your browser.",
      );
    }
  };

  const closeDeactivation = () => {
    setConfirmingDeactivation(false);
    returnFocus();
  };

  const confirmDeactivation = async () => {
    setConfirmingDeactivation(false);
    await deactivate();
    returnFocus();
  };

  return (
    <div className="license-settings">
      <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {announcement || pendingLabel}
      </p>

      {!hasLoaded && operation === "load" ? (
        <section
          className="settings-section"
          aria-labelledby="license-loading-heading"
        >
          <h2 id="license-loading-heading">Your license</h2>
          <div className="settings-card license-loading-card" role="status">
            Loading license...
          </div>
        </section>
      ) : null}

      {status ? (
        <LicenseStatusCard
          status={status}
          actions={
            canManageActiveLicense ? (
              <>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy}
                  onClick={() => void refresh()}
                >
                  {operation === "refresh" ? "Refreshing..." : "Refresh"}
                </button>
                <button
                  ref={deactivateButtonRef}
                  type="button"
                  className="ghost danger"
                  disabled={busy}
                  onClick={() => setConfirmingDeactivation(true)}
                >
                  {operation === "deactivate"
                    ? "Deactivating..."
                    : "Deactivate this device"}
                </button>
              </>
            ) : null
          }
        />
      ) : null}

      {error ? (
        <div
          className={`license-message ${
            error.code === "status_reload_failed" ? "is-warning" : "is-error"
          }`}
          role="alert"
        >
          <span>{error.message}</span>
          {error.recoverable ? (
            <button
              type="button"
              className="ghost"
              onClick={status ? clearError : () => void load()}
            >
              {status ? "Dismiss" : "Retry"}
            </button>
          ) : null}
        </div>
      ) : null}

      {status && canActivate ? (
        <section
          className="settings-section"
          aria-labelledby="license-activation-heading"
        >
          <div className="settings-section-heading license-section-heading">
            <div>
              <h2 id="license-activation-heading">Have a license key?</h2>
              <p className="settings-section-copy">
                Paste it below to use Alfred on this device.
              </p>
            </div>
          </div>
          <form
            className="settings-card license-activation-form"
            onSubmit={handleActivation}
          >
            <div className="license-field">
              <label className="settings-label" htmlFor="license-key-input">
                License key
              </label>
              <input
                id="license-key-input"
                {...LICENSE_KEY_INPUT_ATTRIBUTES}
                value={licenseKey}
                placeholder="Paste your license key"
                aria-describedby={`license-key-hint${showLicenseKeyFeedback ? " license-key-feedback" : ""}`}
                disabled={busy}
                onChange={(event) => setLicenseKey(event.currentTarget.value)}
              />
              <span className="settings-hint" id="license-key-hint">
                Your key is cleared after activation.
              </span>
              {showLicenseKeyFeedback ? (
                <span
                  className={`license-key-feedback ${
                    operation === "activate" ? "is-pending" : "is-ready"
                  }`}
                  id="license-key-feedback"
                  role="status"
                  aria-live="polite"
                >
                  {operation === "activate"
                    ? "Checking license..."
                    : "Key ready to activate."}
                </span>
              ) : null}
            </div>
            <details className="license-advanced-details">
              <summary>Choose a device name</summary>
              <div className="license-field">
                <label className="settings-label" htmlFor="device-label-input">
                  Device name
                </label>
                <input
                  id="device-label-input"
                  type="text"
                  name="deviceLabel"
                  value={deviceLabel}
                  maxLength={100}
                  aria-describedby="device-label-hint"
                  disabled={busy}
                  onChange={(event) => setDeviceLabel(event.currentTarget.value)}
                />
                <span className="settings-hint" id="device-label-hint">
                  This name appears in your Polar account.
                </span>
              </div>
            </details>
            <div className="license-form-actions">
              <button
                type="submit"
                className="primary"
                disabled={busy || !licenseKey.trim() || !deviceLabel.trim()}
              >
                {operation === "activate"
                  ? "Activating..."
                  : "Activate"}
              </button>
            </div>
          </form>
        </section>
      ) : null}

      <section
        className="settings-section"
        aria-labelledby="license-purchase-heading"
      >
        <div className="settings-section-heading license-section-heading">
          <div>
            <h2 id="license-purchase-heading">Billing</h2>
            <p className="settings-section-copy">
              Buy Alfred or manage your billing in Polar.
            </p>
          </div>
        </div>
        <div className="settings-card license-purchase-card">
          <div className="license-purchase-actions">
            <button
              type="button"
              className="primary"
              disabled={!links.isConfigured("desktopCheckout")}
              onClick={() => void openDestination("desktopCheckout")}
            >
              Buy Desktop
            </button>
            <button
              type="button"
              className="ghost license-manage-button"
              disabled={!links.isConfigured("customerPortal")}
              onClick={() => void openDestination("customerPortal")}
            >
              Manage billing
            </button>
          </div>
          {!links.isConfigured("desktopCheckout") ? (
            <p className="settings-hint">
              Licensing checkout is not configured in this build.
            </p>
          ) : null}
          {!links.isConfigured("customerPortal") ? (
            <p className="settings-hint">
              The Polar customer portal is not configured in this build.
            </p>
          ) : null}
          {linkError ? (
            <p className="license-link-error" role="alert">
              {linkError}
            </p>
          ) : null}
        </div>
      </section>

      {confirmingDeactivation ? (
        <ConfirmDialog
          title="Deactivate this device?"
          message="This removes Alfred from this device. It does not cancel your subscription."
          confirmLabel="Deactivate device"
          danger
          onConfirm={() => void confirmDeactivation()}
          onCancel={closeDeactivation}
        />
      ) : null}
    </div>
  );
}
