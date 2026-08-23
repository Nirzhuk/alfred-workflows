import { POLAR_SANDBOX_API_BASE } from "./manifest";

const ACTIVATE_PATH = "/v1/customer-portal/license-keys/activate";
const VALIDATE_PATH = "/v1/customer-portal/license-keys/validate";
const DEACTIVATE_PATH = "/v1/customer-portal/license-keys/deactivate";

type LicenseRead = {
  readonly organization_id: string;
  readonly benefit_id: string;
  readonly status: "granted" | "revoked" | "disabled";
  readonly limit_activations: number | null;
  readonly expires_at: string | null;
};

export type ActivationRead = {
  readonly id: string;
  readonly label: string;
  readonly license_key: LicenseRead;
};

export type ActivationAttempt =
  | { readonly limited: true }
  | { readonly limited: false; readonly activation: ActivationRead };

export class PolarPublicEndpointError extends Error {
  /**
   * Which configuration fields mismatched, for the operator. Only non-secret
   * config values ever appear here — organization/benefit IDs are public by
   * design, and status/limit/expiry are configuration. A license key or
   * activation ID must NEVER be placed in this string.
   */
  readonly detail: readonly string[];

  constructor(detail: readonly string[] = []) {
    super(
      detail.length > 0
        ? `Polar public endpoint verification failed: ${detail.join("; ")}`
        : "Polar public endpoint verification failed",
    );
    this.name = "PolarPublicEndpointError";
    this.detail = detail;
  }
}

/**
 * A short, non-secret summary of a failed Polar response. The request body
 * carries the license key; the RESPONSE body does not, so echoing it is safe
 * and is the only way an operator can tell a wrong organization from a
 * missing benefit.
 */
async function errorSummary(response: Response): Promise<string> {
  try {
    const text = (await response.text()).trim();
    return text.length > 400 ? `${text.slice(0, 400)}…` : text || "(empty body)";
  } catch {
    return "(unreadable body)";
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseLicense(value: unknown): LicenseRead {
  if (!isRecord(value)) throw new PolarPublicEndpointError();
  if (
    typeof value.organization_id !== "string" ||
    typeof value.benefit_id !== "string" ||
    !["granted", "revoked", "disabled"].includes(String(value.status)) ||
    (value.limit_activations !== null &&
      typeof value.limit_activations !== "number") ||
    (value.expires_at !== null && typeof value.expires_at !== "string")
  ) {
    throw new PolarPublicEndpointError();
  }
  return value as LicenseRead;
}

function parseActivation(value: unknown): ActivationRead {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    typeof value.label !== "string"
  ) {
    throw new PolarPublicEndpointError();
  }
  return {
    id: value.id,
    label: value.label,
    license_key: parseLicense(value.license_key),
  };
}

async function responseJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    throw new PolarPublicEndpointError();
  }
}

export class PolarPublicLicenseClient {
  constructor(
    private readonly organizationId: string,
    private readonly fetcher: typeof fetch = fetch,
    private readonly timeoutMs = 10_000,
  ) {}

  async activate(key: string, label: string): Promise<ActivationRead> {
    const response = await this.post(ACTIVATE_PATH, {
      key,
      organization_id: this.organizationId,
      label,
    });
    // Report the HTTP status and Polar's own error text. Neither contains the
    // license key, and without them an activation failure is undiagnosable.
    if (!response.ok) {
      throw new PolarPublicEndpointError([
        `activate returned HTTP ${response.status}: ${await errorSummary(response)}`,
      ]);
    }
    return parseActivation(await responseJson(response));
  }

  async attemptLimitedActivation(
    key: string,
    label: string,
  ): Promise<ActivationAttempt> {
    const response = await this.post(ACTIVATE_PATH, {
      key,
      organization_id: this.organizationId,
      label,
    });
    if (response.status === 403) return { limited: true };
    if (!response.ok) throw new PolarPublicEndpointError();
    return {
      limited: false,
      activation: parseActivation(await responseJson(response)),
    };
  }

  async validate(key: string, activationId: string): Promise<LicenseRead> {
    const response = await this.post(VALIDATE_PATH, {
      key,
      organization_id: this.organizationId,
      activation_id: activationId,
    });
    if (!response.ok) throw new PolarPublicEndpointError();
    return parseLicense(await responseJson(response));
  }

  async deactivate(key: string, activationId: string): Promise<void> {
    const response = await this.post(DEACTIVATE_PATH, {
      key,
      organization_id: this.organizationId,
      activation_id: activationId,
    });
    if (response.status !== 204) throw new PolarPublicEndpointError();
  }

  private async post(
    path: string,
    body: Record<string, string>,
  ): Promise<Response> {
    try {
      return await this.fetcher(`${POLAR_SANDBOX_API_BASE}${path}`, {
        method: "POST",
        headers: {
          Accept: "application/json",
          "Content-Type": "application/json",
        },
        body: JSON.stringify(body),
        redirect: "error",
        signal: AbortSignal.timeout(this.timeoutMs),
      });
    } catch {
      throw new PolarPublicEndpointError();
    }
  }
}
