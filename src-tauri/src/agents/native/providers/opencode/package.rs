pub const OPENCODE_RUNTIME_VERSION: &str = "1.18.23";
pub const OPENCODE_LICENSE: &str = "MIT";

pub const PACKAGE_GATE_CODE: &str = "opencode_native_package_unverified";
pub const ACCOUNT_GATE_CODE: &str = "opencode_native_secret_entry_unavailable";
pub const TOOL_GATE_CODE: &str = "opencode_native_tool_bridge_unavailable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodePackagePlatform {
    MacOsArm64,
    MacOsX64,
    LinuxArm64,
    LinuxX64,
    WindowsArm64,
    WindowsX64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeNativeReleaseGate {
    pub runtime_version: &'static str,
    pub license: &'static str,
    pub platforms: &'static [OpenCodePackagePlatform],
    pub ready: bool,
    pub blockers: &'static [(&'static str, &'static str)],
}

const PLATFORMS: &[OpenCodePackagePlatform] = &[
    OpenCodePackagePlatform::MacOsArm64,
    OpenCodePackagePlatform::MacOsX64,
    OpenCodePackagePlatform::LinuxArm64,
    OpenCodePackagePlatform::LinuxX64,
    OpenCodePackagePlatform::WindowsArm64,
    OpenCodePackagePlatform::WindowsX64,
];

const BLOCKERS: &[(&str, &str)] = &[
    (
        PACKAGE_GATE_CODE,
        "Alfred has no pinned OpenCode artifact manifest, checksum verification, signing/notarization evidence, or updater ownership",
    ),
    (
        ACCOUNT_GATE_CODE,
        "the frozen native-account contract has no approved non-React secret-entry seam for an upstream provider credential",
    ),
    (
        TOOL_GATE_CODE,
        "OpenCode 1.18.23 permission events expose untyped metadata and only allow/reject; the official server has no typed Alfred-owned tool-result injection endpoint",
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
