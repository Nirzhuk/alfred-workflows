use super::{CODEX_APP_SERVER_TAG, CODEX_APP_SERVER_VERSION, CODEX_PROTOCOL_REVISION};
use crate::agents::native::CapabilityReportStatus;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const OFFICIAL_RELEASE_BASE: &str =
    "https://github.com/openai/codex/releases/download/rust-v0.149.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexArtifactSignature {
    SigstoreBundle,
    NotPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeArtifact {
    pub target: &'static str,
    pub archive_name: &'static str,
    pub sha256: &'static str,
    pub signature: CodexArtifactSignature,
}

impl CodexRuntimeArtifact {
    pub fn url(self) -> String {
        format!("{OFFICIAL_RELEASE_BASE}/{}", self.archive_name)
    }
}

/// Release-manifest digests returned by GitHub for OpenAI's official 0.149.1
/// app-server archives on 2026-08-25.
pub const CODEX_RUNTIME_ARTIFACTS: [CodexRuntimeArtifact; 6] = [
    CodexRuntimeArtifact {
        target: "aarch64-apple-darwin",
        archive_name: "codex-app-server-aarch64-apple-darwin.tar.gz",
        sha256: "23500f566f25e675c11de5327c57a6ca9ba109ac213f4e1f9585588c060c9d88",
        signature: CodexArtifactSignature::NotPublished,
    },
    CodexRuntimeArtifact {
        target: "x86_64-apple-darwin",
        archive_name: "codex-app-server-x86_64-apple-darwin.tar.gz",
        sha256: "386aaf051f42094d1c00de6421979ccdd80bbc2dff6daf44d94393fbf37d94e7",
        signature: CodexArtifactSignature::NotPublished,
    },
    CodexRuntimeArtifact {
        target: "aarch64-pc-windows-msvc",
        archive_name: "codex-app-server-aarch64-pc-windows-msvc.exe.zip",
        sha256: "e81eaee21bcda8200382aa7ea76e859d2e6061840658741204386c1102cf79da",
        signature: CodexArtifactSignature::NotPublished,
    },
    CodexRuntimeArtifact {
        target: "x86_64-pc-windows-msvc",
        archive_name: "codex-app-server-x86_64-pc-windows-msvc.exe.zip",
        sha256: "c808dc2d26473f20b0afdac24fcd219ed85bc64b3f3f5fade2bdcfdad7d2a513",
        signature: CodexArtifactSignature::NotPublished,
    },
    CodexRuntimeArtifact {
        target: "aarch64-unknown-linux-musl",
        archive_name: "codex-app-server-aarch64-unknown-linux-musl.tar.gz",
        sha256: "1c2747edfa83619006c5c2a59cc5e92bf95c184d821fdca29cdbb3c2a1fbbaed",
        signature: CodexArtifactSignature::SigstoreBundle,
    },
    CodexRuntimeArtifact {
        target: "x86_64-unknown-linux-musl",
        archive_name: "codex-app-server-x86_64-unknown-linux-musl.tar.gz",
        sha256: "f014c146b7cabb8b5240df6c22ee6762df694ae65a00f345e1de79560189ecb1",
        signature: CodexArtifactSignature::SigstoreBundle,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReleaseGateEntry {
    pub gate: &'static str,
    pub status: CapabilityReportStatus,
    pub evidence: &'static str,
}

pub fn codex_release_gates() -> Vec<CodexReleaseGateEntry> {
    vec![
        CodexReleaseGateEntry {
            gate: "auth",
            status: CapabilityReportStatus::Supported,
            evidence: "official app-server account/login/start supports chatgpt and chatgptDeviceCode",
        },
        CodexReleaseGateEntry {
            gate: "runtime_artifacts",
            status: CapabilityReportStatus::Supported,
            evidence: "OpenAI release rust-v0.149.1 publishes app-server archives for macOS, Windows, and Linux",
        },
        CodexReleaseGateEntry {
            gate: "checksums",
            status: CapabilityReportStatus::Supported,
            evidence: "GitHub release assets publish sha256 digests pinned in this module",
        },
        CodexReleaseGateEntry {
            gate: "license",
            status: CapabilityReportStatus::Supported,
            evidence: "OpenAI Codex rust-v0.149.1 is Apache-2.0 with redistribution conditions",
        },
        CodexReleaseGateEntry {
            gate: "protocol",
            status: CapabilityReportStatus::Supported,
            evidence: "rust-v0.149.1 documents version-specific JSONL JSON-RPC schemas and stable auth/turn methods",
        },
        CodexReleaseGateEntry {
            gate: "cross_platform_signing",
            status: CapabilityReportStatus::Blocked,
            evidence: "official rust-v0.149.1 app-server release publishes Sigstore bundles only for Linux targets",
        },
        CodexReleaseGateEntry {
            gate: "native_ready",
            status: CapabilityReportStatus::Blocked,
            evidence: "runtime registration and release claims stay disabled until every packaged target has an approved signing verification route and packaged smoke evidence",
        },
    ]
}

pub fn codex_native_ready() -> bool {
    codex_release_gates()
        .iter()
        .all(|entry| entry.status != CapabilityReportStatus::Blocked)
}

#[derive(Debug, Error)]
pub enum CodexRuntimePackageError {
    #[error("Codex runtime target is unsupported")]
    UnsupportedTarget,
    #[error("Codex runtime artifact checksum did not match")]
    ChecksumMismatch,
    #[error("Codex runtime artifact could not be read")]
    ArtifactIo,
    #[error("Codex runtime home path is invalid")]
    InvalidRuntimeHome,
    #[error("Codex native release gate is blocked")]
    ReleaseBlocked,
}

pub fn artifact_for_target(target: &str) -> Result<CodexRuntimeArtifact, CodexRuntimePackageError> {
    CODEX_RUNTIME_ARTIFACTS
        .iter()
        .copied()
        .find(|artifact| artifact.target == target)
        .ok_or(CodexRuntimePackageError::UnsupportedTarget)
}

pub fn verify_artifact_checksum(
    artifact: CodexRuntimeArtifact,
    archive_path: &Path,
) -> Result<(), CodexRuntimePackageError> {
    let mut file = fs::File::open(archive_path).map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != artifact.sha256 {
        return Err(CodexRuntimePackageError::ChecksumMismatch);
    }
    Ok(())
}

/// Creates an Alfred-owned runtime home. It is versioned and account-scoped,
/// never `~/.codex` and never a global `CODEX_HOME`.
pub fn prepare_codex_runtime_home(
    app_data_root: &Path,
    account_ref: &str,
) -> Result<PathBuf, CodexRuntimePackageError> {
    if !safe_component(account_ref) {
        return Err(CodexRuntimePackageError::InvalidRuntimeHome);
    }
    fs::create_dir_all(app_data_root).map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    let canonical_root = app_data_root
        .canonicalize()
        .map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    let home = canonical_root
        .join("native-runtimes")
        .join("codex")
        .join(CODEX_APP_SERVER_VERSION)
        .join("accounts")
        .join(account_ref);
    fs::create_dir_all(&home).map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    let canonical_home = home
        .canonicalize()
        .map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    if !canonical_home.starts_with(&canonical_root) {
        return Err(CodexRuntimePackageError::InvalidRuntimeHome);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&canonical_home, fs::Permissions::from_mode(0o700))
            .map_err(|_| CodexRuntimePackageError::ArtifactIo)?;
    }
    Ok(canonical_home)
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeFreeze {
    pub runtime_version: &'static str,
    pub release_tag: &'static str,
    pub protocol_revision: &'static str,
    pub native_ready: bool,
}

pub fn runtime_freeze() -> CodexRuntimeFreeze {
    CodexRuntimeFreeze {
        runtime_version: CODEX_APP_SERVER_VERSION,
        release_tag: CODEX_APP_SERVER_TAG,
        protocol_revision: CODEX_PROTOCOL_REVISION,
        native_ready: codex_native_ready(),
    }
}

/// Process cleanup primitive used by the eventual packaged adapter. Keeping it
/// separate makes exit/Drop behavior testable without enabling runtime launch.
pub(crate) struct CodexChildGuard {
    child: Option<std::process::Child>,
}

impl CodexChildGuard {
    #[allow(dead_code)]
    pub(crate) fn adopt(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    #[allow(dead_code)]
    pub(crate) fn terminate(&mut self) -> io::Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        match child.try_wait()? {
            Some(_) => Ok(()),
            None => {
                child.kill()?;
                let _ = child.wait();
                Ok(())
            }
        }
    }
}

impl Drop for CodexChildGuard {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn release_gate_is_honestly_blocked_and_artifacts_are_complete() {
        assert!(!codex_native_ready());
        assert_eq!(runtime_freeze().runtime_version, "0.149.1");
        assert_eq!(CODEX_RUNTIME_ARTIFACTS.len(), 6);
        for target in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-pc-windows-msvc",
            "x86_64-pc-windows-msvc",
            "aarch64-unknown-linux-musl",
            "x86_64-unknown-linux-musl",
        ] {
            let artifact = artifact_for_target(target).unwrap();
            assert_eq!(artifact.sha256.len(), 64);
            assert!(artifact.url().starts_with("https://github.com/openai/codex/releases/"));
        }
    }

    #[test]
    fn checksum_mismatch_fails_closed() {
        let root = std::env::temp_dir().join(format!("alfred-codex-digest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("artifact.tar.gz");
        fs::write(&archive, b"not the official artifact").unwrap();
        let artifact = artifact_for_target("aarch64-apple-darwin").unwrap();
        assert!(matches!(
            verify_artifact_checksum(artifact, &archive),
            Err(CodexRuntimePackageError::ChecksumMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_home_is_alfred_owned_account_scoped_and_private() {
        let root = std::env::temp_dir().join(format!("alfred-codex-home-{}", uuid::Uuid::new_v4()));
        let home = prepare_codex_runtime_home(&root, "account_fixture-01").unwrap();
        assert!(home.starts_with(root.canonicalize().unwrap()));
        assert!(home.ends_with("native-runtimes/codex/0.149.1/accounts/account_fixture-01"));
        assert!(!home.to_string_lossy().contains("/.codex"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&home).unwrap().permissions().mode() & 0o777, 0o700);
        }
        assert!(matches!(
            prepare_codex_runtime_home(&root, "../escape"),
            Err(CodexRuntimePackageError::InvalidRuntimeHome)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_guard_terminates_and_reaps_the_exact_child() {
        let executable = std::env::current_exe().unwrap();
        let child = Command::new(executable)
            .args([
                "--exact",
                "agents::native::providers::codex::runtime::tests::codex_process_fixture_child",
                "--nocapture",
            ])
            .env("ALFRED_CODEX_PROCESS_FIXTURE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut guard = CodexChildGuard::adopt(child);
        std::thread::sleep(Duration::from_millis(30));
        guard.terminate().unwrap();
        // Idempotence matters when cancellation and Drop race.
        guard.terminate().unwrap();
    }

    #[test]
    fn codex_process_fixture_child() {
        if std::env::var_os("ALFRED_CODEX_PROCESS_FIXTURE").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }
}
