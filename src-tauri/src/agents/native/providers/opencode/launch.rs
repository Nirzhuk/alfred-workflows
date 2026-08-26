use crate::agents::native::{NativeErrorCode, NativeRuntimeError};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

const SERVER_USERNAME: &str = "alfred-opencode";

/// Fully isolated launch contract for a future verified bundled executable.
///
/// This is deliberately a data contract, not a `Command::spawn` fallback. A
/// caller may only consume it after the package gate is satisfied by release
/// engineering. The environment is allowlisted from empty and redirects every
/// OpenCode-owned XDG path into Alfred's dedicated runtime home.
pub struct OpenCodeLaunchSpec {
    executable: PathBuf,
    runtime_home: PathBuf,
    port: u16,
    environment: BTreeMap<String, String>,
}

impl fmt::Debug for OpenCodeLaunchSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenCodeLaunchSpec")
            .field("executable", &self.executable)
            .field("runtime_home", &self.runtime_home)
            .field("port", &self.port)
            .field("environment", &"[REDACTED]")
            .finish()
    }
}

impl OpenCodeLaunchSpec {
    pub fn new(
        executable: impl Into<PathBuf>,
        runtime_home: impl Into<PathBuf>,
        port: u16,
        server_password: &str,
    ) -> Result<Self, NativeRuntimeError> {
        let executable = executable.into();
        let runtime_home = runtime_home.into();
        validate_absolute_file(&executable, "OpenCode bundled executable")?;
        validate_runtime_home(&runtime_home)?;
        if port == 0 {
            return Err(invalid_launch("OpenCode server port must be preallocated"));
        }
        if server_password.len() < 24
            || server_password.len() > 256
            || server_password.chars().any(char::is_whitespace)
        {
            return Err(invalid_launch("OpenCode server password is invalid"));
        }
        let config = runtime_home.join("config");
        let data = runtime_home.join("data");
        let cache = runtime_home.join("cache");
        let state = runtime_home.join("state");
        let temp = runtime_home.join("tmp");
        let config_file = config.join("opencode.json");
        let environment = BTreeMap::from([
            ("XDG_CONFIG_HOME".into(), path_text(&config)?),
            ("XDG_DATA_HOME".into(), path_text(&data)?),
            ("XDG_CACHE_HOME".into(), path_text(&cache)?),
            ("XDG_STATE_HOME".into(), path_text(&state)?),
            ("TMPDIR".into(), path_text(&temp)?),
            ("OPENCODE_CONFIG".into(), path_text(&config_file)?),
            ("OPENCODE_CONFIG_DIR".into(), path_text(&config)?),
            (
                "OPENCODE_CONFIG_CONTENT".into(),
                r#"{"autoupdate":false,"share":"disabled","permission":{"*":"deny"}}"#.into(),
            ),
            ("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "true".into()),
            ("OPENCODE_SERVER_USERNAME".into(), SERVER_USERNAME.into()),
            ("OPENCODE_SERVER_PASSWORD".into(), server_password.into()),
        ]);
        Ok(Self {
            executable,
            runtime_home,
            port,
            environment,
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn runtime_home(&self) -> &Path {
        &self.runtime_home
    }

    pub fn args(&self) -> [String; 3] {
        [
            "serve".into(),
            "--hostname=127.0.0.1".into(),
            format!("--port={}", self.port),
        ]
    }

    /// The eventual launcher must call `env_clear()` and then apply exactly
    /// this map. Kept crate-private so secrets cannot become command/UI DTOs.
    pub(crate) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

fn validate_absolute_file(path: &Path, label: &str) -> Result<(), NativeRuntimeError> {
    if !path.is_absolute() || path.file_name().is_none() {
        Err(invalid_launch(format!("{label} path is invalid")))
    } else {
        Ok(())
    }
}

fn validate_runtime_home(path: &Path) -> Result<(), NativeRuntimeError> {
    if !path.is_absolute() || path.parent().is_none() || path.file_name().is_none() {
        Err(invalid_launch(
            "OpenCode runtime home must be a dedicated absolute directory",
        ))
    } else {
        Ok(())
    }
}

fn path_text(path: &Path) -> Result<String, NativeRuntimeError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid_launch("OpenCode runtime path is not valid UTF-8"))
}

fn invalid_launch(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidRequest, message, false)
}
