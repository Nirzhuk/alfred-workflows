//! Trust inputs for the managed Codex Python SDK sidecar.
//!
//! These are source-package expectations, not verification evidence. Only the
//! shared runtime-package verifier can mint the sealed
//! [`RuntimePackageVerification`] accepted by the package store.

use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
use crate::agents::runtime_package::{
    PublisherVerificationScheme, RuntimeArtifactManifest, RuntimeLicenseNoticeRequirements,
    RuntimePackageExpectation, RuntimePackageManifest, RuntimePackageSelection,
    RuntimePackageVerification, RuntimePublisherRequirement, RuntimeRollbackMetadata,
    RuntimeTargetManifest, RuntimeUpdatePolicy, RUNTIME_PACKAGE_CONTRACT_VERSION,
    RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

pub const CODEX_SDK_RUNTIME_VERSION: &str = "0.147.0";
pub const CODEX_SDK_PYTHON_MINIMUM: &str = "3.10";
pub const CODEX_SDK_SOURCE_COMMIT: &str = "025a88adbd7ae4d448fc938b28d0446eb1753317";
pub const CODEX_SDK_WHEEL_SHA256: &str =
    "ab2e0b3a41dba5a62be8561397cf3e7913afb53b5372ad881002a6f0b77e6a0a";
pub const CODEX_SDK_SDIST_SHA256: &str =
    "e0ab4ea3ac44585a98a80df11e220519fec4641d508d830d2c1d656ce471e2ed";
pub const CODEX_SDK_LICENSE_EXPRESSION: &str = "Apache-2.0";
pub const CODEX_SDK_LICENSE_RESOURCE: &str = "legal/openai-codex/LICENSE";
pub const CODEX_SDK_NOTICE_RESOURCE: &str = "legal/openai-codex/NOTICE";
pub const CODEX_SDK_SBOM_RESOURCE: &str = "legal/sbom.cdx.json";
pub const CODEX_SDK_LICENSE_SHA256: &str =
    "d17f227e4df5da1600391338865ce0f3055211760a36688f816941d58232d8dc";
pub const CODEX_SDK_NOTICE_SHA256: &str =
    "9d71575ecfd9a843fc1677b0efb08053c6ba9fd686a0de1a6f5382fd3c220915";
pub const CODEX_SDK_SIDECAR_RESOURCE: &str = "bin/alfred-codex-sdk-sidecar";
pub const CODEX_SDK_CLI_RESOURCE: &str = "libexec/codex";
pub const CODEX_SDK_PUBLISHER: &str = "OpenAI OpCo, LLC";
pub const CODEX_SDK_SIDECAR_SHA256: &str =
    "da2c0047682756a85a219b24b75232304a0b2effccd4500c506c10f7c1e31a72";
pub const CODEX_SDK_SBOM_SHA256: &str =
    "7d7687c04d3ff16c2a1d3a882761e6f625524bd7be27e6a8cd798fbb32ec95b9";
pub const CODEX_CLI_NATIVE_SHA256_AARCH64_APPLE_DARWIN: &str =
    "19c4f144c5226a9f17c58e6f0fa854843b0f77a6eb420f40e2745a12f10f5d37";

pub const SEALED_PACKAGE_BLOCKER: &str = "codex_python_sdk_sealed_package_unverified";

const CHECKED_SOURCE_MANIFEST: &[u8] =
    include_bytes!("../../../../../sidecars/codex-sdk/runtime-package.source.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliWheel {
    pub target: &'static str,
    pub sha256: &'static str,
}

pub const CODEX_CLI_WHEELS: [CodexCliWheel; 8] = [
    CodexCliWheel {
        target: "x86_64-apple-darwin",
        sha256: "19c3a72a0eac6706bb5023088ab7eb31d8cfc06bf7860250b5c5b90f4505489b",
    },
    CodexCliWheel {
        target: "aarch64-apple-darwin",
        sha256: "b851943fffc48aa7c5c130b6a34be09964833d2785546eda96d749427c6e24f2",
    },
    CodexCliWheel {
        target: "aarch64-unknown-linux-gnu",
        sha256: "aab3e27ce07cd7bc7a78708efa40375b28e89f586851c495ef55aa6cb6806d11",
    },
    CodexCliWheel {
        target: "x86_64-unknown-linux-gnu",
        sha256: "cb3907d633cda87c4b68b47ff4979e6054c02a08c41b15aa2e9a689558a61074",
    },
    CodexCliWheel {
        target: "aarch64-unknown-linux-musl",
        sha256: "ea0bfdee98164fc7d589eea5623aff06c8f8f2ba3f9bc550297429db45ea68f7",
    },
    CodexCliWheel {
        target: "x86_64-unknown-linux-musl",
        sha256: "a9be23f46326494b7ebf4bd98cbe985542f5ad076355f3b0fb636a984df56816",
    },
    CodexCliWheel {
        target: "x86_64-pc-windows-msvc",
        sha256: "1a403ff803ae27e078189a0fd24687f32d43c46f152e50cdbe3eddc9e302f697",
    },
    CodexCliWheel {
        target: "aarch64-pc-windows-msvc",
        sha256: "be0c8b9e34b067151d0964a24e9f4b8b48ea516448a08ef2636f357ebdc877b7",
    },
];

pub fn codex_cli_wheel_for_target(target: &str) -> Result<CodexCliWheel, CodexSdkPackageError> {
    CODEX_CLI_WHEELS
        .iter()
        .copied()
        .find(|wheel| wheel.target == target)
        .ok_or(CodexSdkPackageError::UnsupportedTarget)
}

/// Code-owned sealed package manifest for the host-prepared Codex runtime.
///
/// Targets without a prepared sidecar executable are omitted rather than
/// invented. The Apple verifier authenticates OpenAI's Developer ID signature
/// on `libexec/codex`; the sidecar digest is pinned separately.
pub fn package_manifest() -> RuntimePackageManifest {
    RuntimePackageManifest {
        schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
        contract_version: RUNTIME_PACKAGE_CONTRACT_VERSION,
        runtime_id: ManagedRuntimeId::CodexPythonSdk,
        runtime_version: CODEX_SDK_RUNTIME_VERSION.into(),
        update_policy: RuntimeUpdatePolicy {
            alfred_managed: true,
            self_update_allowed: false,
            path_lookup_allowed: false,
        },
        targets: vec![RuntimeTargetManifest {
            target: "aarch64-apple-darwin".into(),
            executable: RuntimeArtifactManifest {
                relative_path: CODEX_SDK_SIDECAR_RESOURCE.into(),
                sha256: CODEX_SDK_SIDECAR_SHA256.into(),
            },
            resources: vec![
                RuntimeArtifactManifest {
                    relative_path: CODEX_SDK_CLI_RESOURCE.into(),
                    sha256: CODEX_CLI_NATIVE_SHA256_AARCH64_APPLE_DARWIN.into(),
                },
                RuntimeArtifactManifest {
                    relative_path: CODEX_SDK_LICENSE_RESOURCE.into(),
                    sha256: CODEX_SDK_LICENSE_SHA256.into(),
                },
                RuntimeArtifactManifest {
                    relative_path: CODEX_SDK_NOTICE_RESOURCE.into(),
                    sha256: CODEX_SDK_NOTICE_SHA256.into(),
                },
                RuntimeArtifactManifest {
                    relative_path: CODEX_SDK_SBOM_RESOURCE.into(),
                    sha256: CODEX_SDK_SBOM_SHA256.into(),
                },
            ],
            publisher_verification: RuntimePublisherRequirement {
                scheme: PublisherVerificationScheme::AppleDeveloperId,
                publisher: CODEX_SDK_PUBLISHER.into(),
                required: true,
            },
            license_notice: RuntimeLicenseNoticeRequirements {
                license_expression: CODEX_SDK_LICENSE_EXPRESSION.into(),
                license_resource_path: CODEX_SDK_LICENSE_RESOURCE.into(),
                notice_resource_path: CODEX_SDK_NOTICE_RESOURCE.into(),
            },
            rollback: RuntimeRollbackMetadata {
                retain_previous_verified: true,
                automatic_fallback: false,
            },
        }],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSdkPackageError {
    UnsupportedTarget,
    SelectionMismatch,
    SourceInputsInvalid,
    SealedVerificationUnavailable,
}

impl CodexSdkPackageError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedTarget => "codex_python_sdk_target_unsupported",
            Self::SelectionMismatch => "codex_python_sdk_package_selection_mismatch",
            Self::SourceInputsInvalid => "codex_python_sdk_source_inputs_invalid",
            Self::SealedVerificationUnavailable => SEALED_PACKAGE_BLOCKER,
        }
    }
}

impl fmt::Display for CodexSdkPackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for CodexSdkPackageError {}

/// The target build inputs a shared platform verifier must authenticate before
/// it may return a sealed verification capability. The provider never accepts
/// a downloaded JSON `verified` flag or constructs that capability itself.
pub struct CodexSdkVerifierRequest<'a> {
    pub package_root: &'a Path,
    pub expectation: &'a RuntimePackageExpectation,
    pub source_manifest: &'a [u8],
    pub target_sbom: &'a [u8],
    pub license: &'a [u8],
    pub notice: &'a [u8],
    pub cli_wheel: CodexCliWheel,
    pub sdk_wheel_sha256: &'static str,
    pub sdk_source_commit: &'static str,
}

impl fmt::Debug for CodexSdkVerifierRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSdkVerifierRequest")
            .field("expectation", self.expectation)
            .field("cli_target", &self.cli_wheel.target)
            .field("sdk_source_commit", &self.sdk_source_commit)
            .finish_non_exhaustive()
    }
}

pub trait CodexSdkPackageVerifier: Send + Sync {
    fn verify(
        &self,
        request: CodexSdkVerifierRequest<'_>,
    ) -> Result<RuntimePackageVerification, CodexSdkPackageError>;
}

/// Produces the exact request for the shared verifier. It deliberately does
/// not install or activate the result; the shared package store retains those
/// responsibilities.
pub fn verify_codex_sdk_package(
    package_root: &Path,
    target: &str,
    source_manifest: &[u8],
    target_sbom: &[u8],
    license: &[u8],
    notice: &[u8],
    verifier: &dyn CodexSdkPackageVerifier,
) -> Result<RuntimePackageVerification, CodexSdkPackageError> {
    if !package_root.is_absolute()
        || source_manifest != CHECKED_SOURCE_MANIFEST
        || target_sbom.is_empty()
        || target_sbom.len() > 8 * 1024 * 1024
        || license.is_empty()
        || license.len() > 1024 * 1024
        || notice.is_empty()
        || notice.len() > 1024 * 1024
        || sha256_hex(license) != CODEX_SDK_LICENSE_SHA256
        || sha256_hex(notice) != CODEX_SDK_NOTICE_SHA256
    {
        return Err(CodexSdkPackageError::SourceInputsInvalid);
    }
    let expectation = RuntimePackageExpectation::for_product(AgentProductId::ChatgptCodex, target)
        .map_err(|_| CodexSdkPackageError::SelectionMismatch)?;
    let cli_wheel = codex_cli_wheel_for_target(target)?;
    verifier.verify(CodexSdkVerifierRequest {
        package_root,
        expectation: &expectation,
        source_manifest,
        target_sbom,
        license,
        notice,
        cli_wheel,
        sdk_wheel_sha256: CODEX_SDK_WHEEL_SHA256,
        sdk_source_commit: CODEX_SDK_SOURCE_COMMIT,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A `RuntimePackageSelection` is already sealed by shared code. This final
/// adapter check prevents a valid package for another product/version from
/// being substituted at launch.
pub fn validate_codex_sdk_selection(
    selection: &RuntimePackageSelection,
) -> Result<(), CodexSdkPackageError> {
    let expectation = selection.expectation();
    if expectation.product() != AgentProductId::ChatgptCodex
        || expectation.runtime_id() != ManagedRuntimeId::CodexPythonSdk
        || expectation.runtime_version() != CODEX_SDK_RUNTIME_VERSION
        || codex_cli_wheel_for_target(expectation.target()).is_err()
    {
        return Err(CodexSdkPackageError::SelectionMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_package_manifest_is_valid() {
        package_manifest()
            .validate()
            .expect("codex package manifest");
    }
}
