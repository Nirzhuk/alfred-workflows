//! Code-owned Claude Code 2.1.246 package and publisher expectations.
//!
//! Downloaded JSON is descriptive input only. It cannot manufacture the
//! sealed [`RuntimePackageVerification`] capability accepted by the package
//! store. A platform verifier must authenticate Anthropic's detached manifest
//! signature, the exact binary digest, and the native code signature where the
//! platform has one before returning that capability.

use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
use crate::agents::runtime_package::{
    PublisherVerificationScheme, RuntimeArtifactManifest, RuntimeLicenseNoticeRequirements,
    RuntimePackageExpectation, RuntimePackageManifest, RuntimePackageSelection,
    RuntimePackageStore, RuntimePackageVerification, RuntimePublisherRequirement,
    RuntimeRollbackMetadata, RuntimeTargetManifest, RuntimeUpdatePolicy,
    RUNTIME_PACKAGE_CONTRACT_VERSION, RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const CLAUDE_CODE_RUNTIME_VERSION: &str = "2.1.246";
pub const CLAUDE_CODE_RELEASE_COMMIT: &str = "1ba9d2211ae14e591bd1d60451c217c51f415e86";
pub const CLAUDE_CODE_RELEASE_BUILD_DATE: &str = "2026-08-25T18:46:33Z";
pub const CLAUDE_CODE_RELEASE_BASE_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.246";
pub const CLAUDE_CODE_RELEASE_MANIFEST_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json";
pub const CLAUDE_CODE_RELEASE_SIGNATURE_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.246/manifest.json.sig";
pub const CLAUDE_CODE_RELEASE_SIGNING_KEY_URL: &str =
    "https://downloads.claude.ai/keys/claude-code.asc";
pub const CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT: &str =
    "31DDDE24DDFAB679F42D7BD2BAA929FF1A7ECACE";
pub const CLAUDE_CODE_LICENSE_EXPRESSION: &str = "LicenseRef-Anthropic-Claude-Code";
pub const CLAUDE_CODE_LICENSE_RESOURCE: &str = "legal/CLAUDE_CODE_LICENSE.txt";
pub const CLAUDE_CODE_NOTICE_RESOURCE: &str = "legal/NOTICE.txt";
pub const CLAUDE_CODE_LICENSE_SHA256: &str =
    "08edffd8a6fa739c2888b68b23c63ee0ee76a3cf960f0532e882579f6d517f08";
pub const CLAUDE_CODE_NOTICE_SHA256: &str =
    "6ae41b35b33b23e2f4c4f3f370498c9cba247e5953fc01cb6a9a76d3d66faec5";
pub const CLAUDE_CODE_LICENSE_BYTES: &[u8] = include_bytes!("resources/CLAUDE_CODE_LICENSE.txt");
pub const CLAUDE_CODE_NOTICE_BYTES: &[u8] = include_bytes!("resources/NOTICE.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeNativeSignatureExpectation {
    AppleDeveloperIdAndNotarization,
    WindowsAuthenticode,
    SignedReleaseManifestOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeReleaseArtifact {
    pub runtime_target: &'static str,
    pub publisher_platform: &'static str,
    pub executable_name: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub publisher: &'static str,
    pub signature: ClaudeNativeSignatureExpectation,
}

impl ClaudeReleaseArtifact {
    pub fn url(self) -> String {
        format!(
            "{CLAUDE_CODE_RELEASE_BASE_URL}/{}/{}",
            self.publisher_platform, self.executable_name
        )
    }

    fn publisher_scheme(self) -> PublisherVerificationScheme {
        match self.signature {
            ClaudeNativeSignatureExpectation::AppleDeveloperIdAndNotarization => {
                PublisherVerificationScheme::AppleDeveloperId
            }
            ClaudeNativeSignatureExpectation::WindowsAuthenticode => {
                PublisherVerificationScheme::WindowsAuthenticode
            }
            ClaudeNativeSignatureExpectation::SignedReleaseManifestOnly => {
                PublisherVerificationScheme::PlatformPackageSignature
            }
        }
    }
}

pub const CLAUDE_CODE_RELEASE_ARTIFACTS: [ClaudeReleaseArtifact; 8] = [
    ClaudeReleaseArtifact {
        runtime_target: "aarch64-apple-darwin",
        publisher_platform: "darwin-arm64",
        executable_name: "claude",
        sha256: "7b09f01cb76a38e0e3a7c47c5d698d382162a5ff26538fc778683770caf9218b",
        size: 230_824_016,
        publisher: "Anthropic PBC",
        signature: ClaudeNativeSignatureExpectation::AppleDeveloperIdAndNotarization,
    },
    ClaudeReleaseArtifact {
        runtime_target: "x86_64-apple-darwin",
        publisher_platform: "darwin-x64",
        executable_name: "claude",
        sha256: "336625850986371487de7ece776d583f36cc3b3bc7178fcfbde3656d010289fb",
        size: 239_710_512,
        publisher: "Anthropic PBC",
        signature: ClaudeNativeSignatureExpectation::AppleDeveloperIdAndNotarization,
    },
    ClaudeReleaseArtifact {
        runtime_target: "aarch64-unknown-linux-gnu",
        publisher_platform: "linux-arm64",
        executable_name: "claude",
        sha256: "f98296e6e61c507589d1a973b262976b734700ec4e055cb64afdbf6d9a337db7",
        size: 247_389_632,
        publisher: CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT,
        signature: ClaudeNativeSignatureExpectation::SignedReleaseManifestOnly,
    },
    ClaudeReleaseArtifact {
        runtime_target: "x86_64-unknown-linux-gnu",
        publisher_platform: "linux-x64",
        executable_name: "claude",
        sha256: "1a0a662dc1bb938eaec38545abce9a4a69113d7d7f7c5e1a553ea276617b906a",
        size: 247_905_800,
        publisher: CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT,
        signature: ClaudeNativeSignatureExpectation::SignedReleaseManifestOnly,
    },
    ClaudeReleaseArtifact {
        runtime_target: "aarch64-unknown-linux-musl",
        publisher_platform: "linux-arm64-musl",
        executable_name: "claude",
        sha256: "8f7d77a91b95dbeda31393463b2ac7d2b490c577486829b732ce1ac41a747034",
        size: 240_072_248,
        publisher: CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT,
        signature: ClaudeNativeSignatureExpectation::SignedReleaseManifestOnly,
    },
    ClaudeReleaseArtifact {
        runtime_target: "x86_64-unknown-linux-musl",
        publisher_platform: "linux-x64-musl",
        executable_name: "claude",
        sha256: "81de46c50d987b99d2a1b4f3b11f3acf8c6deb57bef2a41f8e770837ddcf52fb",
        size: 241_667_960,
        publisher: CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT,
        signature: ClaudeNativeSignatureExpectation::SignedReleaseManifestOnly,
    },
    ClaudeReleaseArtifact {
        runtime_target: "x86_64-pc-windows-msvc",
        publisher_platform: "win32-x64",
        executable_name: "claude.exe",
        sha256: "9f07f1ecaf26231fc2fac489e7c5214140d38fd14764938a2c8c46f31931d204",
        size: 250_948_768,
        publisher: "Anthropic, PBC",
        signature: ClaudeNativeSignatureExpectation::WindowsAuthenticode,
    },
    ClaudeReleaseArtifact {
        runtime_target: "aarch64-pc-windows-msvc",
        publisher_platform: "win32-arm64",
        executable_name: "claude.exe",
        sha256: "911036439b49e81c5801092764e803207fc56b4e690afadb743f51220c2f8201",
        size: 242_062_496,
        publisher: "Anthropic, PBC",
        signature: ClaudeNativeSignatureExpectation::WindowsAuthenticode,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublisherReleaseManifest {
    version: String,
    commit: String,
    build_date: String,
    platforms: BTreeMap<String, PublisherPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PublisherPlatform {
    binary: String,
    checksum: String,
    size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudePackageErrorCode {
    UnsupportedTarget,
    ReleaseManifestInvalid,
    ReleaseManifestMismatch,
    DetachedSignatureMissing,
    PublisherVerificationRejected,
    SealedVerificationMismatch,
    PackageStoreRejected,
}

impl ClaudePackageErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedTarget => "claude_package_target_unsupported",
            Self::ReleaseManifestInvalid => "claude_release_manifest_invalid",
            Self::ReleaseManifestMismatch => "claude_release_manifest_mismatch",
            Self::DetachedSignatureMissing => "claude_release_signature_missing",
            Self::PublisherVerificationRejected => "claude_publisher_verification_rejected",
            Self::SealedVerificationMismatch => "claude_sealed_verification_mismatch",
            Self::PackageStoreRejected => "claude_package_store_rejected",
        }
    }
}

pub struct ClaudePackageError(ClaudePackageErrorCode);

impl ClaudePackageError {
    pub fn code(&self) -> ClaudePackageErrorCode {
        self.0
    }
}

impl fmt::Debug for ClaudePackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for ClaudePackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for ClaudePackageError {}

type PackageResult<T> = Result<T, ClaudePackageError>;

fn package_error(code: ClaudePackageErrorCode) -> ClaudePackageError {
    ClaudePackageError(code)
}

pub fn artifact_for_target(target: &str) -> PackageResult<ClaudeReleaseArtifact> {
    CLAUDE_CODE_RELEASE_ARTIFACTS
        .iter()
        .copied()
        .find(|artifact| artifact.runtime_target == target)
        .ok_or_else(|| package_error(ClaudePackageErrorCode::UnsupportedTarget))
}

/// The full code-owned package manifest. Every target is frozen even though a
/// package installation selects exactly one target at its trust boundary.
pub fn package_manifest() -> RuntimePackageManifest {
    RuntimePackageManifest {
        schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
        contract_version: RUNTIME_PACKAGE_CONTRACT_VERSION,
        runtime_id: ManagedRuntimeId::ClaudeCodeManaged,
        runtime_version: CLAUDE_CODE_RUNTIME_VERSION.into(),
        update_policy: RuntimeUpdatePolicy {
            alfred_managed: true,
            self_update_allowed: false,
            path_lookup_allowed: false,
        },
        targets: CLAUDE_CODE_RELEASE_ARTIFACTS
            .iter()
            .copied()
            .map(target_manifest)
            .collect(),
    }
}

fn target_manifest(artifact: ClaudeReleaseArtifact) -> RuntimeTargetManifest {
    RuntimeTargetManifest {
        target: artifact.runtime_target.into(),
        executable: RuntimeArtifactManifest {
            relative_path: format!("bin/{}", artifact.executable_name),
            sha256: artifact.sha256.into(),
        },
        resources: vec![
            RuntimeArtifactManifest {
                relative_path: CLAUDE_CODE_LICENSE_RESOURCE.into(),
                sha256: CLAUDE_CODE_LICENSE_SHA256.into(),
            },
            RuntimeArtifactManifest {
                relative_path: CLAUDE_CODE_NOTICE_RESOURCE.into(),
                sha256: CLAUDE_CODE_NOTICE_SHA256.into(),
            },
        ],
        publisher_verification: RuntimePublisherRequirement {
            scheme: artifact.publisher_scheme(),
            publisher: artifact.publisher.into(),
            required: true,
        },
        license_notice: RuntimeLicenseNoticeRequirements {
            license_expression: CLAUDE_CODE_LICENSE_EXPRESSION.into(),
            license_resource_path: CLAUDE_CODE_LICENSE_RESOURCE.into(),
            notice_resource_path: CLAUDE_CODE_NOTICE_RESOURCE.into(),
        },
        rollback: RuntimeRollbackMetadata {
            retain_previous_verified: true,
            automatic_fallback: false,
        },
    }
}

/// Inputs a code-owned platform verifier must authenticate. The request is
/// deliberately borrowed and has no serializable `verified` flag.
pub struct ClaudePublisherVerificationRequest<'a> {
    pub package_root: &'a Path,
    pub package_manifest: &'a RuntimePackageManifest,
    pub expectation: &'a RuntimePackageExpectation,
    pub publisher_release_manifest: &'a [u8],
    pub detached_manifest_signature: &'a [u8],
    pub signing_key_fingerprint: &'static str,
    pub artifact: ClaudeReleaseArtifact,
}

impl fmt::Debug for ClaudePublisherVerificationRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudePublisherVerificationRequest")
            .field("target", &self.artifact.runtime_target)
            .field("version", &CLAUDE_CODE_RUNTIME_VERSION)
            .field("signature", &self.artifact.signature)
            .finish_non_exhaustive()
    }
}

/// Implemented only by the shared code-owned platform verifier. Provider code
/// cannot construct [`RuntimePackageVerification`] and downloaded metadata can
/// never deserialize one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudePublisherVerificationError;

pub trait ClaudePublisherVerifier: Send + Sync {
    fn verify(
        &self,
        request: ClaudePublisherVerificationRequest<'_>,
    ) -> Result<RuntimePackageVerification, ClaudePublisherVerificationError>;
}

pub fn verify_package_for_install(
    package_root: &Path,
    target: &str,
    publisher_release_manifest: &[u8],
    detached_manifest_signature: &[u8],
    verifier: &dyn ClaudePublisherVerifier,
) -> PackageResult<RuntimePackageVerification> {
    if detached_manifest_signature.is_empty() || detached_manifest_signature.len() > 64 * 1024 {
        return Err(package_error(
            ClaudePackageErrorCode::DetachedSignatureMissing,
        ));
    }
    validate_publisher_release_manifest(publisher_release_manifest)?;
    let artifact = artifact_for_target(target)?;
    let manifest = package_manifest();
    manifest
        .validate()
        .map_err(|_| package_error(ClaudePackageErrorCode::ReleaseManifestInvalid))?;
    let expectation =
        RuntimePackageExpectation::for_product(AgentProductId::ClaudeCodeSubscription, target)
            .map_err(|_| package_error(ClaudePackageErrorCode::ReleaseManifestMismatch))?;
    let verification = verifier
        .verify(ClaudePublisherVerificationRequest {
            package_root,
            package_manifest: &manifest,
            expectation: &expectation,
            publisher_release_manifest,
            detached_manifest_signature,
            signing_key_fingerprint: CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT,
            artifact,
        })
        .map_err(|_| package_error(ClaudePackageErrorCode::PublisherVerificationRejected))?;
    if verification.manifest() != &manifest || verification.expectation() != &expectation {
        return Err(package_error(
            ClaudePackageErrorCode::SealedVerificationMismatch,
        ));
    }
    Ok(verification)
}

pub fn stage_and_select_verified_package(
    store: &RuntimePackageStore,
    package_root: &Path,
    verification: &RuntimePackageVerification,
) -> PackageResult<RuntimePackageSelection> {
    if verification.manifest() != &package_manifest()
        || verification.expectation().product() != AgentProductId::ClaudeCodeSubscription
        || verification.expectation().runtime_version() != CLAUDE_CODE_RUNTIME_VERSION
    {
        return Err(package_error(
            ClaudePackageErrorCode::SealedVerificationMismatch,
        ));
    }
    store
        .stage_and_activate(package_root, verification, None)
        .and_then(|_| store.select_active(verification))
        .map_err(|_| package_error(ClaudePackageErrorCode::PackageStoreRejected))
}

fn validate_publisher_release_manifest(bytes: &[u8]) -> PackageResult<()> {
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(package_error(
            ClaudePackageErrorCode::ReleaseManifestInvalid,
        ));
    }
    let parsed: PublisherReleaseManifest = serde_json::from_slice(bytes)
        .map_err(|_| package_error(ClaudePackageErrorCode::ReleaseManifestInvalid))?;
    if parsed.version != CLAUDE_CODE_RUNTIME_VERSION
        || parsed.commit != CLAUDE_CODE_RELEASE_COMMIT
        || parsed.build_date != CLAUDE_CODE_RELEASE_BUILD_DATE
        || parsed.platforms.len() != CLAUDE_CODE_RELEASE_ARTIFACTS.len()
    {
        return Err(package_error(
            ClaudePackageErrorCode::ReleaseManifestMismatch,
        ));
    }
    for artifact in CLAUDE_CODE_RELEASE_ARTIFACTS {
        let Some(platform) = parsed.platforms.get(artifact.publisher_platform) else {
            return Err(package_error(
                ClaudePackageErrorCode::ReleaseManifestMismatch,
            ));
        };
        if platform.binary != artifact.executable_name
            || platform.checksum != artifact.sha256
            || platform.size != artifact.size
        {
            return Err(package_error(
                ClaudePackageErrorCode::ReleaseManifestMismatch,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn official_release_manifest_fixture() -> Vec<u8> {
    let platforms = CLAUDE_CODE_RELEASE_ARTIFACTS
        .iter()
        .map(|artifact| {
            (
                artifact.publisher_platform.to_owned(),
                serde_json::json!({
                    "binary": artifact.executable_name,
                    "checksum": artifact.sha256,
                    "size": artifact.size,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::to_vec(&serde_json::json!({
        "version": CLAUDE_CODE_RUNTIME_VERSION,
        "commit": CLAUDE_CODE_RELEASE_COMMIT,
        "buildDate": CLAUDE_CODE_RELEASE_BUILD_DATE,
        "platforms": platforms,
    }))
    .expect("fixture manifest")
}
