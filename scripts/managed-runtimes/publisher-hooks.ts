import type { VerificationScheme } from "./manifest";

export type PublisherHookPlan = {
  scheme: VerificationScheme;
  tool: "codesign" | "signtool" | "gpg" | "cosign" | "external";
  requiredEvidence: string[];
  description: string;
};

/**
 * Declarative handoff for the trusted platform verifier. The native Rust
 * package store, never this JSON, mints the sealed verification capability.
 * Keeping the hook names explicit prevents a release script from silently
 * treating a digest or a serialized `verified` flag as a signature.
 */
export function publisherHookPlan(scheme: VerificationScheme, hook: string): PublisherHookPlan {
  switch (scheme) {
    case "apple_developer_id":
      return {
        scheme,
        tool: "codesign",
        requiredEvidence: ["manifest.json", "manifest.json.sig", "platform-signature.json"],
        description: `${hook}: verify the detached release manifest with Anthropic's pinned key, then codesign --verify --deep --strict and notarization staple validation`,
      };
    case "windows_authenticode":
      return {
        scheme,
        tool: "signtool",
        requiredEvidence: ["manifest.json", "manifest.json.sig", "platform-signature.json"],
        description: `${hook}: verify the detached release manifest with Anthropic's pinned key, then signtool verify /pa`,
      };
    case "platform_package_signature":
      return {
        scheme,
        tool: hook === "signed_release_manifest" ? "gpg" : "external",
        requiredEvidence: ["publisher-verification.json"],
        description: `${hook}: require independently supplied publisher attestation; release archive SHA-256 alone is not publisher proof`,
      };
    case "sigstore_bundle":
      return {
        scheme,
        tool: "cosign",
        requiredEvidence: ["publisher-verification.json", "python.sigstore.json", "cli-wheel.sigstore.json", "pydantic-core.sigstore.json", "sdk-wheel.sigstore.json"],
        description: `${hook}: run an offline Sigstore bundle verification bound to the exact artifact digest and expected publisher identity`,
      };
  }
}
