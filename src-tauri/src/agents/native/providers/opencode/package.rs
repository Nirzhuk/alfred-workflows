//! Code-owned OpenCode 1.18.23 package expectations.
//!
//! The GitHub archive digests and extracted executable digests are frozen
//! independently. Neither of them is publisher-verification evidence. The
//! sealed package store accepts a [`RuntimePackageSelection`] only from the
//! shared platform verifier represented by [`OpenCodePackageVerifier`].

use crate::agent_accounts::models::AgentProductId;
use crate::agents::native::{NativeErrorCode, NativeRuntimeError};
use crate::agents::runtime_package::{
    PublisherVerificationScheme, RuntimeArtifactManifest, RuntimeLicenseNoticeRequirements,
    RuntimePackageExpectation, RuntimePackageManifest, RuntimePackageSelection,
    RuntimePublisherRequirement, RuntimeRollbackMetadata, RuntimeTargetManifest,
    RuntimeUpdatePolicy, RUNTIME_PACKAGE_CONTRACT_VERSION, RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
};

pub const OPENCODE_RUNTIME_ID: &str = "opencode_server";
pub const OPENCODE_RUNTIME_VERSION: &str = "1.18.23";
pub const OPENCODE_LICENSE: &str = "MIT";
pub const OPENCODE_RELEASE_BASE: &str =
    "https://github.com/anomalyco/opencode/releases/download/v1.18.23";

pub const COMMERCIAL_GATE_CODE: &str = "opencode_native_commercial_approval_missing";
pub const PACKAGE_GATE_CODE: &str = "opencode_native_package_unverified";
pub const SUPERVISOR_HTTP_GATE_CODE: &str =
    "opencode_native_supervisor_http_capability_unavailable";
pub const ACCOUNT_GATE_CODE: &str = "opencode_native_secret_entry_unavailable";
pub const APPROVAL_GATE_CODE: &str = "opencode_native_host_approval_bridge_unavailable";
pub const LIVE_SMOKE_GATE_CODE: &str = "opencode_native_packaged_live_smoke_missing";
pub const AGGREGATE_GATE_CODE: &str = "opencode_package_account_and_tool_bridge_unverified";

const LICENSE_PATH: &str = "legal/LICENSE";
const NOTICE_PATH: &str = "legal/NOTICE.md";
const LICENSE_SHA256: &str = "625f0f619133f89bbbb2abe37369613dfa1885eba1e50d02170deb62bb42cb6b";
const NOTICE_SHA256: &str = "b223bb239d3b29b67b9e921570e2107d2e592148e8498561067a5d013559a07f";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodePackagePlatform {
    MacOsArm64,
    MacOsX64,
    LinuxArm64,
    LinuxX64,
    WindowsArm64,
    WindowsX64,
}

impl OpenCodePackagePlatform {
    pub const ALL: [Self; 6] = [
        Self::MacOsArm64,
        Self::MacOsX64,
        Self::LinuxArm64,
        Self::LinuxX64,
        Self::WindowsArm64,
        Self::WindowsX64,
    ];

    pub fn target(self) -> &'static str {
        match self {
            Self::MacOsArm64 => "aarch64-apple-darwin",
            Self::MacOsX64 => "x86_64-apple-darwin",
            Self::LinuxArm64 => "aarch64-unknown-linux-gnu",
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::WindowsArm64 => "aarch64-pc-windows-msvc",
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCodeReleaseArtifact {
    pub platform: OpenCodePackagePlatform,
    pub archive_name: &'static str,
    /// GitHub's digest for the immutable release asset. This checks download
    /// identity but is deliberately not accepted as publisher verification.
    pub archive_sha256: &'static str,
    /// Digest of the unmodified `opencode` executable extracted from the
    /// release archive. This is the digest used by the sealed package store.
    pub executable_sha256: &'static str,
    pub executable_bytes: u64,
}

impl OpenCodeReleaseArtifact {
    pub fn download_url(self) -> String {
        format!("{OPENCODE_RELEASE_BASE}/{}", self.archive_name)
    }
}

pub const OPENCODE_RELEASE_ARTIFACTS: [OpenCodeReleaseArtifact; 6] = [
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::MacOsArm64,
        archive_name: "opencode-darwin-arm64.zip",
        archive_sha256: "373cf36673836f2ce8847295a0bb2cd2447d03c769b44d84185916bd471b4274",
        executable_sha256: "f7c45939a895e5a9febf141ab16307418bc41da31879aa0b2e65223190ca1c1a",
        executable_bytes: 144_024_674,
    },
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::MacOsX64,
        archive_name: "opencode-darwin-x64.zip",
        archive_sha256: "6b617da75b5773836fcdc7247d7ea2bd39aec942a58b89a041bafb3d4d2a8c23",
        executable_sha256: "00f71a32da3c05c9170380f673100a9dc4aea7f0e5cce90daad847d7cd8d3641",
        executable_bytes: 149_504_080,
    },
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::LinuxArm64,
        archive_name: "opencode-linux-arm64.tar.gz",
        archive_sha256: "86d3afaf4e8784f9adab189be2a315c12b27ec40a04b70defbe70595c3cc7c65",
        executable_sha256: "ef8514274321679d97f1c7f2ad251890d7f073f2fca743859f379bff55085ac8",
        executable_bytes: 184_068_240,
    },
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::LinuxX64,
        archive_name: "opencode-linux-x64.tar.gz",
        archive_sha256: "ab7015cd8113e011a461f30a0c2b77d8299a144ff688cb62e93e8802835d7288",
        executable_sha256: "de0724a36eaf3166e7f1ff38d0f4478b95ccc47725e9597b3fe66d3d3e18baa2",
        executable_bytes: 184_584_320,
    },
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::WindowsArm64,
        archive_name: "opencode-windows-arm64.zip",
        archive_sha256: "3ff8c553ae270e89499808fbce7635535762f75cfaae4b0bb818b10d7eb18d9b",
        executable_sha256: "a7471b91fc91146f09ed82a7df499c9c07dfdd0a851f0c2885e941bf8caecfb5",
        executable_bytes: 175_446_568,
    },
    OpenCodeReleaseArtifact {
        platform: OpenCodePackagePlatform::WindowsX64,
        archive_name: "opencode-windows-x64.zip",
        archive_sha256: "a2fe9e8c2d074d26975024d494927b966680b3efdc3e0377eadb9afb05f7e191",
        executable_sha256: "f831518278ded5090c41cc532b16ab80629e980f710a0b46d1e5b605808bb1d9",
        executable_bytes: 179_550_760,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeNativeReleaseGate {
    pub runtime_version: &'static str,
    pub license: &'static str,
    pub platforms: &'static [OpenCodePackagePlatform],
    pub ready: bool,
    pub blockers: &'static [(&'static str, &'static str)],
}

const PLATFORMS: &[OpenCodePackagePlatform] = &OpenCodePackagePlatform::ALL;
const BLOCKERS: &[(&str, &str)] = &[
    (
        COMMERCIAL_GATE_CODE,
        "written commercial approval for Alfred-managed OpenCode Go use is not recorded",
    ),
    (
        PACKAGE_GATE_CODE,
        "the shared sealed package store has no production OpenCode publisher-verification constructor",
    ),
    (
        SUPERVISOR_HTTP_GATE_CODE,
        "the supervisor does not yet return its generated Basic-auth capability to a trusted provider client",
    ),
    (
        ACCOUNT_GATE_CODE,
        "the account service has not integrated the transient OpenCode Go secret-entry boundary",
    ),
    (
        APPROVAL_GATE_CODE,
        "the native host has no decision callback for runtime-executed permission requests",
    ),
    (
        LIVE_SMOKE_GATE_CODE,
        "the packaged no-external-CLI OpenCode Go live smoke has not passed",
    ),
];

pub fn native_release_gate() -> OpenCodeNativeReleaseGate {
    OpenCodeNativeReleaseGate {
        runtime_version: OPENCODE_RUNTIME_VERSION,
        license: OPENCODE_LICENSE,
        platforms: PLATFORMS,
        ready: false,
        blockers: BLOCKERS,
    }
}

pub fn package_manifest() -> RuntimePackageManifest {
    RuntimePackageManifest {
        schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
        contract_version: RUNTIME_PACKAGE_CONTRACT_VERSION,
        runtime_id: crate::agent_accounts::models::ManagedRuntimeId::OpencodeServer,
        runtime_version: OPENCODE_RUNTIME_VERSION.into(),
        update_policy: RuntimeUpdatePolicy {
            alfred_managed: true,
            self_update_allowed: false,
            path_lookup_allowed: false,
        },
        targets: OPENCODE_RELEASE_ARTIFACTS
            .iter()
            .copied()
            .map(target_manifest)
            .collect(),
    }
}

fn target_manifest(artifact: OpenCodeReleaseArtifact) -> RuntimeTargetManifest {
    let executable = if matches!(
        artifact.platform,
        OpenCodePackagePlatform::WindowsArm64 | OpenCodePackagePlatform::WindowsX64
    ) {
        "opencode.exe"
    } else {
        "opencode"
    };
    RuntimeTargetManifest {
        target: artifact.platform.target().into(),
        executable: RuntimeArtifactManifest {
            relative_path: executable.into(),
            sha256: artifact.executable_sha256.into(),
        },
        resources: vec![
            RuntimeArtifactManifest {
                relative_path: LICENSE_PATH.into(),
                sha256: LICENSE_SHA256.into(),
            },
            RuntimeArtifactManifest {
                relative_path: NOTICE_PATH.into(),
                sha256: NOTICE_SHA256.into(),
            },
        ],
        publisher_verification: RuntimePublisherRequirement {
            scheme: PublisherVerificationScheme::PlatformPackageSignature,
            publisher: "Alfred verified OpenCode runtime package".into(),
            required: true,
        },
        license_notice: RuntimeLicenseNoticeRequirements {
            license_expression: OPENCODE_LICENSE.into(),
            license_resource_path: LICENSE_PATH.into(),
            notice_resource_path: NOTICE_PATH.into(),
        },
        rollback: RuntimeRollbackMetadata {
            retain_previous_verified: true,
            automatic_fallback: false,
        },
    }
}

/// Provider-facing boundary for the sealed shared package verifier.
///
/// Implementations must live with the trusted platform verifier. Downloaded
/// manifest JSON, release checksums, and provider code cannot implement this by
/// asserting that publisher verification succeeded.
pub trait OpenCodePackageVerifier: Send + Sync {
    fn select_verified_active(
        &self,
        manifest: &RuntimePackageManifest,
        expectation: &RuntimePackageExpectation,
        release: OpenCodeReleaseArtifact,
    ) -> Result<RuntimePackageSelection, NativeRuntimeError>;
}

pub fn select_verified_package(
    verifier: &dyn OpenCodePackageVerifier,
    target: &str,
) -> Result<RuntimePackageSelection, NativeRuntimeError> {
    let manifest = package_manifest();
    manifest.validate().map_err(|_| package_unavailable())?;
    let expectation = RuntimePackageExpectation::for_product(AgentProductId::OpencodeGo, target)
        .map_err(|_| package_unavailable())?;
    let release = OPENCODE_RELEASE_ARTIFACTS
        .iter()
        .copied()
        .find(|artifact| artifact.platform.target() == target)
        .ok_or_else(package_unavailable)?;
    let selection = verifier.select_verified_active(&manifest, &expectation, release)?;
    if selection.expectation() != &expectation {
        return Err(package_unavailable());
    }
    Ok(selection)
}

fn package_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        PACKAGE_GATE_CODE,
        false,
    )
}

pub const OPENCODE_LICENSE_BYTES: &[u8] = include_bytes!("legal/LICENSE");
pub const OPENCODE_NOTICE_BYTES: &[u8] = include_bytes!("legal/NOTICE.md");
