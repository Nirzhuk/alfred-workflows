//! Account-scoped storage for provider-managed runtime credentials and state.
//!
//! A profile reference is opaque and resolves only inside Alfred's app-data
//! directory. Callers receive provider-specific environment roots for process
//! launch, but profile handles and errors never format absolute paths.

use super::models::{AgentProductId, ManagedRuntimeId};
use crate::agents::OpaqueAgentAccountRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const RUNTIME_PROFILE_SCHEMA_VERSION: u16 = 1;

const MANAGED_RUNTIME_DIR: &str = "managed-runtimes";
const PROFILES_DIR: &str = "profiles";
const PROFILE_REF_PREFIX: &str = "runtime_profile_";
const PROFILE_HOME_DIR: &str = "home";
const PROFILE_TEMP_DIR: &str = "tmp";
const MAX_PROFILE_RECORDS: usize = 1_024;
const MAX_RETAINED_PROFILE_RECORDS: usize = 128;
const MAX_PROFILE_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfileErrorCode {
    InvalidReference,
    InvalidBinding,
    RuntimeMismatch,
    StorageUnavailable,
    ProfileNotFound,
    ProfileInvalid,
    ProfileMismatch,
    ProfileStateInvalid,
    ProfilePreserveFailed,
    ProfileInUse,
    ProfilePurgeFailed,
}

impl RuntimeProfileErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidReference => "runtime_profile_reference_invalid",
            Self::InvalidBinding => "runtime_profile_binding_invalid",
            Self::RuntimeMismatch => "runtime_profile_runtime_mismatch",
            Self::StorageUnavailable => "runtime_profile_storage_unavailable",
            Self::ProfileNotFound => "runtime_profile_not_found",
            Self::ProfileInvalid => "runtime_profile_invalid",
            Self::ProfileMismatch => "runtime_profile_mismatch",
            Self::ProfileStateInvalid => "runtime_profile_state_invalid",
            Self::ProfilePreserveFailed => "runtime_profile_preserve_failed",
            Self::ProfileInUse => "runtime_profile_in_use",
            Self::ProfilePurgeFailed => "runtime_profile_purge_failed",
        }
    }
}

impl fmt::Debug for RuntimeProfileErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for RuntimeProfileErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

pub struct RuntimeProfileError {
    code: RuntimeProfileErrorCode,
}

impl RuntimeProfileError {
    fn new(code: RuntimeProfileErrorCode) -> Self {
        Self { code }
    }

    pub fn code(&self) -> RuntimeProfileErrorCode {
        self.code
    }
}

impl fmt::Debug for RuntimeProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl fmt::Display for RuntimeProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for RuntimeProfileError {}

type ProfileResult<T> = Result<T, RuntimeProfileError>;

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeProfileRef(String);

impl RuntimeProfileRef {
    pub fn parse(value: &str) -> ProfileResult<Self> {
        let Some(suffix) = value.strip_prefix(PROFILE_REF_PREFIX) else {
            return Err(profile_error(RuntimeProfileErrorCode::InvalidReference));
        };
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(profile_error(RuntimeProfileErrorCode::InvalidReference));
        }
        Ok(Self(value.to_owned()))
    }

    fn generate() -> Self {
        Self(format!(
            "{PROFILE_REF_PREFIX}{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RuntimeProfileRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("runtime_profile_reference_redacted")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeProfileBinding {
    account_scope_hash: String,
    product: AgentProductId,
    runtime_id: ManagedRuntimeId,
    runtime_version: String,
}

impl RuntimeProfileBinding {
    pub fn new(
        account_ref: &OpaqueAgentAccountRef,
        product: AgentProductId,
        runtime_id: ManagedRuntimeId,
        runtime_version: impl Into<String>,
    ) -> ProfileResult<Self> {
        let runtime_version = runtime_version.into();
        if product.managed_runtime() != Some(runtime_id) {
            return Err(profile_error(RuntimeProfileErrorCode::RuntimeMismatch));
        }
        if !valid_component(&runtime_version, 128)
            || runtime_version.eq_ignore_ascii_case("latest")
            || product.managed_runtime_version() != Some(runtime_version.as_str())
        {
            return Err(profile_error(RuntimeProfileErrorCode::InvalidBinding));
        }
        Ok(Self {
            account_scope_hash: account_scope_hash(account_ref.as_str()),
            product,
            runtime_id,
            runtime_version,
        })
    }

    pub fn product(&self) -> AgentProductId {
        self.product
    }

    pub fn runtime_id(&self) -> ManagedRuntimeId {
        self.runtime_id
    }

    pub fn runtime_version(&self) -> &str {
        &self.runtime_version
    }
}

impl fmt::Debug for RuntimeProfileBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeProfileBinding")
            .field("product", &self.product)
            .field("runtime_id", &self.runtime_id)
            .field("runtime_version", &self.runtime_version)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProfileLifecycle {
    Active,
    Preserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEnvironmentVariable {
    ClaudeConfigDir,
    CodexHome,
    XdgDataHome,
    XdgConfigHome,
    XdgCacheHome,
    XdgStateHome,
}

impl RuntimeEnvironmentVariable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeConfigDir => "CLAUDE_CONFIG_DIR",
            Self::CodexHome => "CODEX_HOME",
            Self::XdgDataHome => "XDG_DATA_HOME",
            Self::XdgConfigHome => "XDG_CONFIG_HOME",
            Self::XdgCacheHome => "XDG_CACHE_HOME",
            Self::XdgStateHome => "XDG_STATE_HOME",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeEnvironmentRoots {
    entries: Vec<(RuntimeEnvironmentVariable, PathBuf)>,
}

impl RuntimeEnvironmentRoots {
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Path)> {
        self.entries
            .iter()
            .map(|(variable, path)| (variable.as_str(), path.as_path()))
    }

    pub fn get(&self, variable: RuntimeEnvironmentVariable) -> Option<&Path> {
        self.entries
            .iter()
            .find(|(candidate, _)| *candidate == variable)
            .map(|(_, path)| path.as_path())
    }
}

impl fmt::Debug for RuntimeEnvironmentRoots {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self
            .entries
            .iter()
            .map(|(variable, _)| variable.as_str())
            .collect::<Vec<_>>();
        formatter
            .debug_tuple("RuntimeEnvironmentRoots")
            .field(&names)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeProfile {
    profile_ref: RuntimeProfileRef,
    binding: RuntimeProfileBinding,
    lifecycle: RuntimeProfileLifecycle,
    generation: u64,
    root: PathBuf,
    environment_roots: RuntimeEnvironmentRoots,
    home_root: PathBuf,
    temp_root: PathBuf,
}

impl RuntimeProfile {
    pub fn profile_ref(&self) -> &RuntimeProfileRef {
        &self.profile_ref
    }

    pub fn binding(&self) -> &RuntimeProfileBinding {
        &self.binding
    }

    pub fn lifecycle(&self) -> RuntimeProfileLifecycle {
        self.lifecycle
    }

    pub fn environment_roots(&self) -> &RuntimeEnvironmentRoots {
        &self.environment_roots
    }

    pub(crate) fn launch_home_root(&self) -> &Path {
        &self.home_root
    }

    pub(crate) fn launch_temp_root(&self) -> &Path {
        &self.temp_root
    }

    pub(crate) fn acquire_supervisor_lease(&self) -> ProfileResult<RuntimeProfileSupervisorLease> {
        let mut leases = supervisor_leases()
            .lock()
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        self.revalidate_for_launch()?;
        let count = leases
            .entry(self.profile_ref.as_str().to_owned())
            .or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        Ok(RuntimeProfileSupervisorLease {
            profile_ref: self.profile_ref.as_str().to_owned(),
        })
    }

    /// Revalidates the current immutable profile record immediately before a
    /// managed process launch. This prevents an older in-memory `Active`
    /// handle from being used after the profile was preserved, purged, or
    /// replaced on disk.
    pub(crate) fn revalidate_for_launch(&self) -> ProfileResult<()> {
        let metadata = fs::symlink_metadata(&self.root)
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileInvalid))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
        let canonical = self
            .root
            .canonicalize()
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileInvalid))?;
        if canonical != self.root {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
        let record = read_profile_record(&self.root)?;
        if record.generation != self.generation
            || record.profile_ref != self.profile_ref.as_str()
            || record.account_scope_hash != self.binding.account_scope_hash
            || record.product != self.binding.product
            || record.runtime_id != self.binding.runtime_id
            || record.runtime_version != self.binding.runtime_version
            || record.lifecycle != RuntimeProfileLifecycle::Active
        {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileMismatch));
        }
        open_profile_launch_roots(&self.root)?;
        open_environment_roots(&self.root, self.binding.runtime_id).map(|_| ())
    }
}

impl fmt::Debug for RuntimeProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeProfile")
            .field("product", &self.binding.product)
            .field("runtime_id", &self.binding.runtime_id)
            .field("runtime_version", &self.binding.runtime_version)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

#[derive(Clone)]
pub struct RuntimeProfileStore {
    profiles_root: PathBuf,
}

impl RuntimeProfileStore {
    pub fn new(app_data_root: &Path) -> ProfileResult<Self> {
        fs::create_dir_all(app_data_root)
            .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))?;
        let app_data_root = canonical_directory(app_data_root)?;
        let managed_root = app_data_root.join(MANAGED_RUNTIME_DIR);
        create_private_dir_all(&managed_root)?;
        let managed_root = canonical_directory(&managed_root)?;
        if !managed_root.starts_with(&app_data_root) {
            return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable));
        }
        let profiles_root = managed_root.join(PROFILES_DIR);
        create_private_dir_all(&profiles_root)?;
        let profiles_root = canonical_directory(&profiles_root)?;
        if !profiles_root.starts_with(&managed_root) {
            return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable));
        }
        Ok(Self { profiles_root })
    }

    pub fn create(&self, binding: &RuntimeProfileBinding) -> ProfileResult<RuntimeProfile> {
        validate_binding(binding)?;
        for _ in 0..16 {
            let profile_ref = RuntimeProfileRef::generate();
            let root = self.profiles_root.join(profile_ref.as_str());
            match fs::create_dir(&root) {
                Ok(()) => {
                    if let Err(error) = set_private_dir_permissions(&root) {
                        let _ = fs::remove_dir(&root);
                        return Err(error);
                    }
                    let create_result = (|| {
                        create_environment_roots(&root, binding.runtime_id)?;
                        create_private_dir_all(&root.join(PROFILE_HOME_DIR))?;
                        create_private_dir_all(&root.join(PROFILE_TEMP_DIR))?;
                        if binding.runtime_id == ManagedRuntimeId::ClaudeCodeManaged {
                            link_host_keychain_dir(&root.join(PROFILE_HOME_DIR))?;
                        }
                        let record = RuntimeProfileRecord {
                            schema_version: RUNTIME_PROFILE_SCHEMA_VERSION,
                            generation: 1,
                            profile_ref: profile_ref.as_str().to_owned(),
                            account_scope_hash: binding.account_scope_hash.clone(),
                            product: binding.product,
                            runtime_id: binding.runtime_id,
                            runtime_version: binding.runtime_version.clone(),
                            lifecycle: RuntimeProfileLifecycle::Active,
                        };
                        write_profile_record(
                            &root,
                            &record,
                            RuntimeProfileErrorCode::StorageUnavailable,
                        )?;
                        self.open(&profile_ref, binding)
                    })();
                    if create_result.is_err() {
                        let _ = fs::remove_dir_all(&root);
                    }
                    return create_result;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable)),
            }
        }
        Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable))
    }

    pub fn open(
        &self,
        profile_ref: &RuntimeProfileRef,
        expected_binding: &RuntimeProfileBinding,
    ) -> ProfileResult<RuntimeProfile> {
        validate_binding(expected_binding)?;
        let root = self.profile_root(profile_ref)?;
        let record = read_profile_record(&root)?;
        if record.profile_ref != profile_ref.as_str()
            || record.account_scope_hash != expected_binding.account_scope_hash
            || record.product != expected_binding.product
            || record.runtime_id != expected_binding.runtime_id
            || record.runtime_version != expected_binding.runtime_version
        {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileMismatch));
        }
        let binding = RuntimeProfileBinding {
            account_scope_hash: record.account_scope_hash,
            product: record.product,
            runtime_id: record.runtime_id,
            runtime_version: record.runtime_version,
        };
        let environment_roots = open_environment_roots(&root, binding.runtime_id)?;
        let (home_root, temp_root) = open_profile_launch_roots(&root)?;
        Ok(RuntimeProfile {
            profile_ref: profile_ref.clone(),
            binding,
            lifecycle: record.lifecycle,
            generation: record.generation,
            root,
            environment_roots,
            home_root,
            temp_root,
        })
    }

    pub fn preserve(
        &self,
        profile_ref: &RuntimeProfileRef,
        expected_binding: &RuntimeProfileBinding,
    ) -> ProfileResult<RuntimeProfile> {
        let profile = self.open(profile_ref, expected_binding)?;
        if profile.lifecycle == RuntimeProfileLifecycle::Preserved {
            return Ok(profile);
        }
        let generation = profile
            .generation
            .checked_add(1)
            .ok_or_else(|| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        let record = RuntimeProfileRecord {
            schema_version: RUNTIME_PROFILE_SCHEMA_VERSION,
            generation,
            profile_ref: profile_ref.as_str().to_owned(),
            account_scope_hash: expected_binding.account_scope_hash.clone(),
            product: expected_binding.product,
            runtime_id: expected_binding.runtime_id,
            runtime_version: expected_binding.runtime_version.clone(),
            lifecycle: RuntimeProfileLifecycle::Preserved,
        };
        write_profile_record(
            &profile.root,
            &record,
            RuntimeProfileErrorCode::ProfilePreserveFailed,
        )?;
        self.open(profile_ref, expected_binding)
    }

    pub fn purge(
        &self,
        profile_ref: &RuntimeProfileRef,
        expected_binding: &RuntimeProfileBinding,
    ) -> ProfileResult<()> {
        let profile = self.open(profile_ref, expected_binding)?;
        let leases = supervisor_leases()
            .lock()
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        if leases
            .get(profile_ref.as_str())
            .copied()
            .unwrap_or_default()
            > 0
        {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInUse));
        }
        if profile.root.parent() != Some(self.profiles_root.as_path()) {
            return Err(profile_error(RuntimeProfileErrorCode::ProfilePurgeFailed));
        }
        fs::remove_dir_all(&profile.root)
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfilePurgeFailed))?;
        let result = sync_directory(
            &self.profiles_root,
            RuntimeProfileErrorCode::ProfilePurgeFailed,
        );
        drop(leases);
        result
    }

    fn profile_root(&self, profile_ref: &RuntimeProfileRef) -> ProfileResult<PathBuf> {
        RuntimeProfileRef::parse(profile_ref.as_str())?;
        let root = self.profiles_root.join(profile_ref.as_str());
        let metadata = fs::symlink_metadata(&root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                profile_error(RuntimeProfileErrorCode::ProfileNotFound)
            } else {
                profile_error(RuntimeProfileErrorCode::ProfileInvalid)
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
        let root = root
            .canonicalize()
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileInvalid))?;
        if root.parent() != Some(self.profiles_root.as_path()) {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
        Ok(root)
    }
}

impl fmt::Debug for RuntimeProfileStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RuntimeProfileStore")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RuntimeProfileRecord {
    schema_version: u16,
    generation: u64,
    profile_ref: String,
    account_scope_hash: String,
    product: AgentProductId,
    runtime_id: ManagedRuntimeId,
    runtime_version: String,
    lifecycle: RuntimeProfileLifecycle,
}

fn validate_binding(binding: &RuntimeProfileBinding) -> ProfileResult<()> {
    if binding.product.managed_runtime() != Some(binding.runtime_id) {
        return Err(profile_error(RuntimeProfileErrorCode::RuntimeMismatch));
    }
    if binding.account_scope_hash.len() != 64
        || !binding
            .account_scope_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || !valid_component(&binding.runtime_version, 128)
        || binding.runtime_version.eq_ignore_ascii_case("latest")
        || binding.product.managed_runtime_version() != Some(binding.runtime_version.as_str())
    {
        return Err(profile_error(RuntimeProfileErrorCode::InvalidBinding));
    }
    Ok(())
}

pub(crate) struct RuntimeProfileSupervisorLease {
    profile_ref: String,
}

impl Drop for RuntimeProfileSupervisorLease {
    fn drop(&mut self) {
        let Ok(mut leases) = supervisor_leases().lock() else {
            return;
        };
        let Some(count) = leases.get_mut(&self.profile_ref) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            leases.remove(&self.profile_ref);
        }
    }
}

fn supervisor_leases() -> &'static Mutex<HashMap<String, usize>> {
    static LEASES: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn write_profile_record(
    root: &Path,
    record: &RuntimeProfileRecord,
    failure_code: RuntimeProfileErrorCode,
) -> ProfileResult<()> {
    let nonce = uuid::Uuid::new_v4().simple();
    let temporary = root.join(format!(".profile-{nonce}.tmp"));
    let final_path = root.join(format!("profile-{:020}-{nonce}.json", record.generation));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| profile_error(failure_code))?;
        serde_json::to_writer(&mut file, record).map_err(|_| profile_error(failure_code))?;
        file.write_all(b"\n")
            .map_err(|_| profile_error(failure_code))?;
        set_private_file_permissions(&file, failure_code)?;
        file.sync_all().map_err(|_| profile_error(failure_code))?;
        fs::rename(&temporary, &final_path).map_err(|_| profile_error(failure_code))?;
        prune_profile_records(root, &final_path, failure_code)?;
        sync_directory(root, failure_code)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    write_result
}

fn prune_profile_records(
    root: &Path,
    current_path: &Path,
    failure_code: RuntimeProfileErrorCode,
) -> ProfileResult<()> {
    let mut records = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| profile_error(failure_code))? {
        let entry = entry.map_err(|_| profile_error(failure_code))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(generation) = record_filename_generation(&name) else {
            continue;
        };
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| profile_error(failure_code))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        records.push((generation, entry.path()));
    }
    records.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let mut retained = 0usize;
    for (_, path) in records {
        if path == current_path || retained < MAX_RETAINED_PROFILE_RECORDS {
            retained = retained.saturating_add(1);
            continue;
        }
        fs::remove_file(path).map_err(|_| profile_error(failure_code))?;
    }
    Ok(())
}

fn read_profile_record(root: &Path) -> ProfileResult<RuntimeProfileRecord> {
    let mut records = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?
    {
        let entry =
            entry.map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("profile-") || !name.ends_with(".json") {
            continue;
        }
        if records.len() >= MAX_PROFILE_RECORDS {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileStateInvalid));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileStateInvalid));
        }
        let bytes = read_bounded_profile_record(&entry.path())?;
        let record: RuntimeProfileRecord = serde_json::from_slice(&bytes)
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
        if record_filename_generation(&name) != Some(record.generation)
            || record.schema_version != RUNTIME_PROFILE_SCHEMA_VERSION
            || record.generation == 0
            || RuntimeProfileRef::parse(&record.profile_ref).is_err()
            || record.account_scope_hash.len() != 64
            || !record
                .account_scope_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || record.product.managed_runtime() != Some(record.runtime_id)
            || record.product.managed_runtime_version() != Some(record.runtime_version.as_str())
            || !valid_component(&record.runtime_version, 128)
            || record.runtime_version.eq_ignore_ascii_case("latest")
        {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileStateInvalid));
        }
        records.push(record);
    }
    let max_generation = records
        .iter()
        .map(|record| record.generation)
        .max()
        .ok_or_else(|| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
    let mut current = records
        .into_iter()
        .filter(|record| record.generation == max_generation);
    let record = current
        .next()
        .ok_or_else(|| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
    if current.next().is_some() {
        return Err(profile_error(RuntimeProfileErrorCode::ProfileStateInvalid));
    }
    Ok(record)
}

fn read_bounded_profile_record(path: &Path) -> ProfileResult<Vec<u8>> {
    let metadata = fs::metadata(path)
        .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))?;
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_RECORD_BYTES {
        return Err(profile_error(RuntimeProfileErrorCode::ProfileStateInvalid));
    }
    fs::read(path).map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileStateInvalid))
}

fn record_filename_generation(name: &str) -> Option<u64> {
    let body = name.strip_prefix("profile-")?.strip_suffix(".json")?;
    let (generation, nonce) = body.split_once('-')?;
    if generation.len() != 20
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
        || nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    generation.parse().ok()
}

fn create_environment_roots(root: &Path, runtime_id: ManagedRuntimeId) -> ProfileResult<()> {
    for (_, relative) in environment_layout(runtime_id) {
        create_private_dir_all(&root.join(relative))?;
    }
    Ok(())
}

fn open_environment_roots(
    root: &Path,
    runtime_id: ManagedRuntimeId,
) -> ProfileResult<RuntimeEnvironmentRoots> {
    let mut entries = Vec::new();
    for (variable, relative) in environment_layout(runtime_id) {
        let path = resolve_profile_directory(root, relative)?;
        entries.push((variable, path));
    }
    Ok(RuntimeEnvironmentRoots { entries })
}

/// Claude Code persists its subscription token by shelling out to
/// `/usr/bin/security`, which resolves the default keychain under
/// `$HOME/Library/Keychains`. An account-scoped home has no keychain, so the
/// token write fails immediately after an otherwise successful login.
///
/// This is a deliberate, narrowly-scoped hole in the isolated home: exactly
/// one directory is exposed, by name, and only for the Claude runtime. Nothing
/// else in the host home becomes reachable, and no other runtime gets this.
/// Claude Code names its own keychain item, so accounts still share one
/// credential — the caller enforces a single Claude account for that reason.
#[cfg(target_os = "macos")]
fn link_host_keychain_dir(home_root: &Path) -> ProfileResult<()> {
    let Some(host_home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable));
    };
    let host_keychains = host_home.join("Library").join("Keychains");
    let metadata = fs::symlink_metadata(&host_keychains)
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable));
    }
    let library = home_root.join("Library");
    create_private_dir_all(&library)?;
    std::os::unix::fs::symlink(&host_keychains, library.join("Keychains"))
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))
}

#[cfg(not(target_os = "macos"))]
fn link_host_keychain_dir(_home_root: &Path) -> ProfileResult<()> {
    Ok(())
}

fn open_profile_launch_roots(root: &Path) -> ProfileResult<(PathBuf, PathBuf)> {
    Ok((
        resolve_profile_directory(root, PROFILE_HOME_DIR)?,
        resolve_profile_directory(root, PROFILE_TEMP_DIR)?,
    ))
}

fn resolve_profile_directory(root: &Path, relative: &str) -> ProfileResult<PathBuf> {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        if !valid_component(segment, 128) {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
        path.push(segment);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileInvalid))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
        }
    }
    let path = path
        .canonicalize()
        .map_err(|_| profile_error(RuntimeProfileErrorCode::ProfileInvalid))?;
    if !path.starts_with(root) {
        return Err(profile_error(RuntimeProfileErrorCode::ProfileInvalid));
    }
    Ok(path)
}

fn environment_layout(
    runtime_id: ManagedRuntimeId,
) -> Vec<(RuntimeEnvironmentVariable, &'static str)> {
    match runtime_id {
        ManagedRuntimeId::ClaudeCodeManaged => {
            vec![(RuntimeEnvironmentVariable::ClaudeConfigDir, "claude-config")]
        }
        ManagedRuntimeId::CodexPythonSdk => {
            vec![(RuntimeEnvironmentVariable::CodexHome, "codex-home")]
        }
        ManagedRuntimeId::OpencodeServer => vec![
            (RuntimeEnvironmentVariable::XdgDataHome, "xdg/data"),
            (RuntimeEnvironmentVariable::XdgConfigHome, "xdg/config"),
            (RuntimeEnvironmentVariable::XdgCacheHome, "xdg/cache"),
            (RuntimeEnvironmentVariable::XdgStateHome, "xdg/state"),
        ],
    }
}

fn account_scope_hash(account_ref: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"alfred-runtime-profile-account-v1\0");
    hasher.update(account_ref.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn valid_component(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn canonical_directory(path: &Path) -> ProfileResult<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(profile_error(RuntimeProfileErrorCode::StorageUnavailable));
    }
    path.canonicalize()
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))
}

fn create_private_dir_all(path: &Path) -> ProfileResult<()> {
    fs::create_dir_all(path)
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))?;
    set_private_dir_permissions(path)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> ProfileResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| profile_error(RuntimeProfileErrorCode::StorageUnavailable))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> ProfileResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(
    file: &File,
    failure_code: RuntimeProfileErrorCode,
) -> ProfileResult<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| profile_error(failure_code))
}

#[cfg(not(unix))]
fn set_private_file_permissions(
    _file: &File,
    _failure_code: RuntimeProfileErrorCode,
) -> ProfileResult<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path, failure_code: RuntimeProfileErrorCode) -> ProfileResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| profile_error(failure_code))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path, _failure_code: RuntimeProfileErrorCode) -> ProfileResult<()> {
    Ok(())
}

fn profile_error(code: RuntimeProfileErrorCode) -> RuntimeProfileError {
    RuntimeProfileError::new(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(value: &str) -> OpaqueAgentAccountRef {
        OpaqueAgentAccountRef::parse(value).unwrap()
    }

    fn binding(
        account_ref: &OpaqueAgentAccountRef,
        product: AgentProductId,
        runtime_id: ManagedRuntimeId,
        version: &str,
    ) -> RuntimeProfileBinding {
        RuntimeProfileBinding::new(account_ref, product, runtime_id, version).unwrap()
    }

    fn fixture_store() -> (PathBuf, RuntimeProfileStore) {
        let root = std::env::temp_dir().join(format!(
            "alfred-runtime-profiles-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = RuntimeProfileStore::new(&root).unwrap();
        (root, store)
    }

    #[test]
    fn invalid_components_and_product_runtime_mismatches_fail_closed() {
        assert_eq!(
            RuntimeProfileRef::parse("../escape").unwrap_err().code(),
            RuntimeProfileErrorCode::InvalidReference
        );
        let account = account("account_fixture_0001");
        assert_eq!(
            RuntimeProfileBinding::new(
                &account,
                AgentProductId::ChatgptCodex,
                ManagedRuntimeId::OpencodeServer,
                "1.0.0",
            )
            .unwrap_err()
            .code(),
            RuntimeProfileErrorCode::RuntimeMismatch
        );
        assert_eq!(
            RuntimeProfileBinding::new(
                &account,
                AgentProductId::ChatgptCodex,
                ManagedRuntimeId::CodexPythonSdk,
                "latest",
            )
            .unwrap_err()
            .code(),
            RuntimeProfileErrorCode::InvalidBinding
        );
        for component in [".", ".."] {
            assert_eq!(
                RuntimeProfileBinding::new(
                    &account,
                    AgentProductId::ChatgptCodex,
                    ManagedRuntimeId::CodexPythonSdk,
                    component,
                )
                .unwrap_err()
                .code(),
                RuntimeProfileErrorCode::InvalidBinding
            );
        }
    }

    #[test]
    fn two_profiles_are_isolated_and_bound_to_their_accounts() {
        let (root, store) = fixture_store();
        let account_a = account("account_fixture_0001");
        let account_b = account("account_fixture_0002");
        let binding_a = binding(
            &account_a,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        );
        let binding_b = binding(
            &account_b,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        );
        let profile_a = store.create(&binding_a).unwrap();
        let profile_b = store.create(&binding_b).unwrap();
        assert_ne!(profile_a.profile_ref(), profile_b.profile_ref());
        let home_a = profile_a
            .environment_roots()
            .get(RuntimeEnvironmentVariable::CodexHome)
            .unwrap();
        let home_b = profile_b
            .environment_roots()
            .get(RuntimeEnvironmentVariable::CodexHome)
            .unwrap();
        assert_ne!(home_a, home_b);
        fs::write(home_a.join("account-state"), b"a-only").unwrap();
        assert!(!home_b.join("account-state").exists());
        assert_eq!(
            store
                .open(profile_a.profile_ref(), &binding_b)
                .unwrap_err()
                .code(),
            RuntimeProfileErrorCode::ProfileMismatch
        );
        assert_eq!(
            RuntimeProfileBinding::new(
                &account_a,
                AgentProductId::ChatgptCodex,
                ManagedRuntimeId::CodexPythonSdk,
                "0.146.0",
            )
            .unwrap_err()
            .code(),
            RuntimeProfileErrorCode::InvalidBinding
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provider_environment_roots_are_exact_and_debug_redacted() {
        let (root, store) = fixture_store();
        let fixtures = [
            (
                "account_fixture_0001",
                AgentProductId::ClaudeCodeSubscription,
                ManagedRuntimeId::ClaudeCodeManaged,
                "2.1.246",
                vec!["CLAUDE_CONFIG_DIR"],
            ),
            (
                "account_fixture_0002",
                AgentProductId::ChatgptCodex,
                ManagedRuntimeId::CodexPythonSdk,
                "0.147.0",
                vec!["CODEX_HOME"],
            ),
            (
                "account_fixture_0003",
                AgentProductId::OpencodeGo,
                ManagedRuntimeId::OpencodeServer,
                "1.18.23",
                vec![
                    "XDG_DATA_HOME",
                    "XDG_CONFIG_HOME",
                    "XDG_CACHE_HOME",
                    "XDG_STATE_HOME",
                ],
            ),
        ];
        for (account_ref, product, runtime_id, version, expected_names) in fixtures {
            let account = account(account_ref);
            let binding = binding(&account, product, runtime_id, version);
            let profile = store.create(&binding).unwrap();
            let names = profile
                .environment_roots()
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            assert_eq!(names, expected_names);
            let debug = format!("{profile:?} {:?}", profile.environment_roots());
            assert!(!debug.contains(root.to_string_lossy().as_ref()));
            assert!(!debug.contains(profile.profile_ref().as_str()));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserve_is_idempotent_and_purge_removes_only_the_exact_profile() {
        let (root, store) = fixture_store();
        let account_a = account("account_fixture_0001");
        let account_b = account("account_fixture_0002");
        let binding_a = binding(
            &account_a,
            AgentProductId::OpencodeGo,
            ManagedRuntimeId::OpencodeServer,
            "1.18.23",
        );
        let binding_b = binding(
            &account_b,
            AgentProductId::OpencodeGo,
            ManagedRuntimeId::OpencodeServer,
            "1.18.23",
        );
        let profile_a = store.create(&binding_a).unwrap();
        let profile_b = store.create(&binding_b).unwrap();
        let preserved = store.preserve(profile_a.profile_ref(), &binding_a).unwrap();
        assert_eq!(preserved.lifecycle(), RuntimeProfileLifecycle::Preserved);
        assert_eq!(
            store
                .preserve(profile_a.profile_ref(), &binding_a)
                .unwrap()
                .lifecycle(),
            RuntimeProfileLifecycle::Preserved
        );
        store.purge(profile_a.profile_ref(), &binding_a).unwrap();
        assert_eq!(
            store
                .open(profile_a.profile_ref(), &binding_a)
                .unwrap_err()
                .code(),
            RuntimeProfileErrorCode::ProfileNotFound
        );
        assert!(store.open(profile_b.profile_ref(), &binding_b).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_generation_history_is_pruned_to_the_retained_bound() {
        let (root, store) = fixture_store();
        let account = account("account_profile_pruning_0001");
        let binding = binding(
            &account,
            AgentProductId::OpencodeGo,
            ManagedRuntimeId::OpencodeServer,
            "1.18.23",
        );
        let profile = store.create(&binding).unwrap();
        let latest_generation = (MAX_RETAINED_PROFILE_RECORDS + 16) as u64;
        for generation in 2..=latest_generation {
            write_profile_record(
                &profile.root,
                &RuntimeProfileRecord {
                    schema_version: RUNTIME_PROFILE_SCHEMA_VERSION,
                    generation,
                    profile_ref: profile.profile_ref.as_str().to_owned(),
                    account_scope_hash: binding.account_scope_hash.clone(),
                    product: binding.product,
                    runtime_id: binding.runtime_id,
                    runtime_version: binding.runtime_version.clone(),
                    lifecycle: RuntimeProfileLifecycle::Active,
                },
                RuntimeProfileErrorCode::ProfilePreserveFailed,
            )
            .unwrap();
        }
        let record_count = fs::read_dir(&profile.root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("profile-"))
            .count();
        assert_eq!(record_count, MAX_RETAINED_PROFILE_RECORDS);
        assert_eq!(
            read_profile_record(&profile.root).unwrap().generation,
            latest_generation
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[cfg(target_os = "macos")]
    #[test]
    fn a_claude_profile_exposes_only_the_host_keychain_directory() {
        use std::os::unix::fs::PermissionsExt;

        let (root, store) = fixture_store();
        let account = account("account_keychain_probe_0001");
        let binding = binding(
            &account,
            AgentProductId::ClaudeCodeSubscription,
            ManagedRuntimeId::ClaudeCodeManaged,
            "2.1.246",
        );
        let profile = store.create(&binding).unwrap();
        let home = profile.launch_home_root();

        // Claude Code shells out to `security`, which resolves the default
        // keychain under $HOME/Library/Keychains.
        let link = home.join("Library").join("Keychains");
        let metadata = fs::symlink_metadata(&link).expect("keychain link exists");
        assert!(metadata.file_type().is_symlink());
        assert!(link.join("login.keychain-db").exists() || link.is_dir());

        // Nothing else from the host home leaks in.
        let library: Vec<_> = fs::read_dir(home.join("Library"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(library, vec![std::ffi::OsString::from("Keychains")]);
        assert!(!home.join("Library").join("Preferences").exists());
        assert!(!home.join(".ssh").exists());

        let _ = PermissionsExt::mode(&fs::metadata(home).unwrap().permissions());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_symlink_escape_fails_closed() {
        use std::os::unix::fs::symlink;

        let (root, store) = fixture_store();
        let account = account("account_fixture_0001");
        let binding = binding(
            &account,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        );
        let profile_ref = RuntimeProfileRef::generate();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, store.profiles_root.join(profile_ref.as_str())).unwrap();
        assert_eq!(
            store.open(&profile_ref, &binding).unwrap_err().code(),
            RuntimeProfileErrorCode::ProfileInvalid
        );
        fs::remove_file(store.profiles_root.join(profile_ref.as_str())).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn profile_directories_and_state_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let (root, store) = fixture_store();
        let account = account("account_fixture_0001");
        let binding = binding(
            &account,
            AgentProductId::ClaudeCodeSubscription,
            ManagedRuntimeId::ClaudeCodeManaged,
            "2.1.246",
        );
        let profile = store.create(&binding).unwrap();
        assert_eq!(
            fs::metadata(&profile.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let state = fs::read_dir(&profile.root)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with("profile-"))
            .unwrap();
        assert_eq!(
            fs::metadata(state.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn errors_and_public_debug_do_not_expose_paths_or_profile_refs() {
        let (root, store) = fixture_store();
        let account = account("account_fixture_0001");
        let binding = binding(
            &account,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        );
        let profile = store.create(&binding).unwrap();
        assert!(!format!("{profile:?}").contains(root.to_string_lossy().as_ref()));
        assert!(!format!("{profile:?}").contains(profile.profile_ref().as_str()));
        let error = profile_error(RuntimeProfileErrorCode::ProfileMismatch);
        assert_eq!(format!("{error:?}"), "runtime_profile_mismatch");
        assert_eq!(format!("{error}"), "runtime_profile_mismatch");
        assert_eq!(
            serde_json::to_string(&error.code()).unwrap(),
            "\"runtime_profile_mismatch\""
        );
        fs::remove_dir_all(root).unwrap();
    }
}
