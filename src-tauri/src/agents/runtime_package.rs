//! Fail-closed lookup for native runtime resources in a packaged app.
//!
//! The result contains statuses only. Absolute resource paths and I/O errors
//! are deliberately excluded so it is safe to include in support diagnostics.

use super::capability_manifest::PackagedRuntimeMetadata;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResourceStatus {
    NotApplicable,
    Missing,
    InvalidPath,
    ChecksumMismatch,
    LicenseMissing,
    NoticeMissing,
    DataCoupled,
    SigningUnverified,
    RollbackUnverified,
    Unversioned,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackageInspection {
    pub status: RuntimeResourceStatus,
    pub versioned: bool,
    pub checksum_verified: bool,
    pub license_present: bool,
    pub notice_present: bool,
    pub data_independent: bool,
    pub signing_verified: bool,
    pub rollback_verified: bool,
}

impl RuntimePackageInspection {
    pub fn missing(runtime_version: Option<&str>) -> Self {
        inspection(RuntimeResourceStatus::Missing, runtime_version, false)
    }
}

pub fn inspect_runtime_package(
    resource_root: &Path,
    metadata: &PackagedRuntimeMetadata,
    runtime_version: Option<&str>,
) -> RuntimePackageInspection {
    if metadata.kind == "not_applicable" {
        return inspection(RuntimeResourceStatus::NotApplicable, runtime_version, false);
    }
    if !runtime_version.is_some_and(valid_version) {
        return inspection(RuntimeResourceStatus::Unversioned, runtime_version, false);
    }
    let Some(relative) = metadata.resource_path.as_deref() else {
        return inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    if !metadata.included || !safe_relative_path(relative) {
        return inspection(RuntimeResourceStatus::InvalidPath, runtime_version, false);
    }
    let Ok(root) = resource_root.canonicalize() else {
        return inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    let Ok(runtime_path) = root.join(relative).canonicalize() else {
        return inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    if !runtime_path.starts_with(&root) || !runtime_path.is_file() {
        return inspection(RuntimeResourceStatus::InvalidPath, runtime_version, false);
    }
    let checksum_verified = metadata.sha256.as_deref().is_some_and(|expected| {
        expected.len() == 64
            && expected.bytes().all(|byte| byte.is_ascii_hexdigit())
            && sha256(&runtime_path).is_some_and(|actual| actual == expected.to_ascii_lowercase())
    });
    if !checksum_verified {
        return inspection(
            RuntimeResourceStatus::ChecksumMismatch,
            runtime_version,
            false,
        );
    }
    let license_present = !metadata.license.is_empty()
        && metadata.license != "not_applicable"
        && resource_exists(&root, metadata.license_resource_path.as_deref());
    if !license_present {
        return inspection(RuntimeResourceStatus::LicenseMissing, runtime_version, true);
    }
    let notice_present = resource_exists(&root, metadata.notice_resource_path.as_deref());
    if !notice_present {
        return inspection(RuntimeResourceStatus::NoticeMissing, runtime_version, true);
    }
    if !metadata.data_independent {
        return inspection(RuntimeResourceStatus::DataCoupled, runtime_version, true);
    }
    let signing_verified = metadata.signing_status == "verified"
        && resource_exists(&root, metadata.signing_resource_path.as_deref());
    if !signing_verified {
        return inspection(
            RuntimeResourceStatus::SigningUnverified,
            runtime_version,
            true,
        );
    }
    let rollback_verified = metadata.rollback_status == "verified"
        && !metadata.automatic_fallback
        && resource_exists(&root, metadata.rollback_resource_path.as_deref());
    if !rollback_verified {
        return inspection(
            RuntimeResourceStatus::RollbackUnverified,
            runtime_version,
            true,
        );
    }
    RuntimePackageInspection {
        status: RuntimeResourceStatus::Ready,
        versioned: runtime_version.is_some_and(valid_version),
        checksum_verified: true,
        license_present: true,
        notice_present: true,
        data_independent: true,
        signing_verified: true,
        rollback_verified: true,
    }
}

fn inspection(
    status: RuntimeResourceStatus,
    runtime_version: Option<&str>,
    checksum_verified: bool,
) -> RuntimePackageInspection {
    RuntimePackageInspection {
        status,
        versioned: runtime_version.is_some_and(valid_version),
        checksum_verified,
        license_present: false,
        notice_present: false,
        data_independent: false,
        signing_verified: false,
        rollback_verified: false,
    }
}

fn resource_exists(root: &Path, relative: Option<&str>) -> bool {
    let Some(relative) = relative else {
        return false;
    };
    if !safe_relative_path(relative) {
        return false;
    }
    root.join(relative)
        .canonicalize()
        .is_ok_and(|path| path.starts_with(root) && path.is_file())
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && Path::new(value).components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        })
}

fn valid_version(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'/' | b'_'))
}

fn sha256(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_metadata() -> PackagedRuntimeMetadata {
        PackagedRuntimeMetadata {
            kind: "fixture_sidecar".into(),
            included: true,
            resource_path: Some("runtime/fixture-runtime.txt".into()),
            checksum_status: "verified".into(),
            sha256: Some(
                "cbf38a1c6d10d43e8451d55fc441d491c9055a2ca5d386e1dc467e63cce2e289".into(),
            ),
            license: "MIT".into(),
            license_resource_path: Some("runtime/LICENSE.txt".into()),
            notice_resource_path: Some("runtime/NOTICE.txt".into()),
            signing_resource_path: Some("runtime/SIGNING.txt".into()),
            rollback_resource_path: Some("runtime/ROLLBACK.json".into()),
            signing_status: "verified".into(),
            rollback_status: "verified".into(),
            data_independent: true,
            automatic_fallback: false,
        }
    }

    #[test]
    fn fixture_package_requires_version_checksum_license_signing_and_rollback() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");
        let report = inspect_runtime_package(&root, &fixture_metadata(), Some("fixture-1.0.0"));
        assert_eq!(report.status, RuntimeResourceStatus::Ready);
        assert!(report.versioned);
        assert!(report.checksum_verified);
        assert!(report.license_present);
        assert!(report.notice_present);
        assert!(report.data_independent);
        assert!(report.signing_verified);
        assert!(report.rollback_verified);
    }

    #[test]
    fn traversal_and_automatic_fallback_fail_closed_without_exposing_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");
        let mut metadata = fixture_metadata();
        metadata.resource_path = Some("../Cargo.toml".into());
        let report = inspect_runtime_package(&root, &metadata, Some("fixture-1.0.0"));
        assert_eq!(report.status, RuntimeResourceStatus::InvalidPath);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("Cargo.toml"));

        let mut metadata = fixture_metadata();
        metadata.automatic_fallback = true;
        assert_eq!(
            inspect_runtime_package(&root, &metadata, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::RollbackUnverified
        );
        assert_eq!(
            inspect_runtime_package(&root, &fixture_metadata(), None).status,
            RuntimeResourceStatus::Unversioned
        );
    }

    #[test]
    fn missing_tampered_and_unlicensed_packages_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");

        let mut missing = fixture_metadata();
        missing.resource_path = Some("runtime/not-present".into());
        assert_eq!(
            inspect_runtime_package(&root, &missing, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::Missing
        );

        let mut tampered = fixture_metadata();
        tampered.sha256 = Some("0".repeat(64));
        assert_eq!(
            inspect_runtime_package(&root, &tampered, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::ChecksumMismatch
        );

        let mut unlicensed = fixture_metadata();
        unlicensed.license_resource_path = Some("runtime/NO-LICENSE".into());
        assert_eq!(
            inspect_runtime_package(&root, &unlicensed, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::LicenseMissing
        );
        let mut undeclared_license = fixture_metadata();
        undeclared_license.license.clear();
        assert_eq!(
            inspect_runtime_package(&root, &undeclared_license, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::LicenseMissing
        );

        let mut unsigned = fixture_metadata();
        unsigned.signing_resource_path = Some("runtime/NO-SIGNING".into());
        assert_eq!(
            inspect_runtime_package(&root, &unsigned, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::SigningUnverified
        );

        let mut no_rollback = fixture_metadata();
        no_rollback.rollback_resource_path = Some("runtime/NO-ROLLBACK".into());
        assert_eq!(
            inspect_runtime_package(&root, &no_rollback, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::RollbackUnverified
        );
    }
}
