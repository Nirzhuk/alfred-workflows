//! Discover models from each local agent automatically.
//!
//! Sources:
//! - Claude Code: stable CLI aliases (without opening a headless session)
//! - OpenCode: `opencode models`
//! - Codex: `~/.codex/models_cache.json` (+ default from `config.toml`)
//! - Cursor: `cursor-agent models` / `--list-models`, else Cursor IDE
//!   `availableDefaultModels2` from `state.vscdb`
//! - GitHub Copilot, Gemini, and Grok: stable CLI model aliases after binary
//!   detection; each CLI keeps its own model picker and does not expose a
//!   reliable non-interactive catalog.

use super::AgentProvider;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    /// Value passed to the CLI (`--model`).
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    pub provider: String,
    pub default_model: String,
    pub models: Vec<ModelOption>,
    /// Allow typing a custom model id (useful for OpenCode provider/model).
    pub allow_custom: bool,
    /// `discovered` when loaded from the agent; `fallback` when CLI/cache missing.
    pub source: String,
    /// Whether the agent binary/cache was found.
    pub available: bool,
    #[serde(default)]
    pub error: Option<String>,
}

fn opt(
    id: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
) -> ModelOption {
    ModelOption {
        id: id.into(),
        label: label.into(),
        description: description.into(),
    }
}

pub fn default_model(provider: AgentProvider) -> &'static str {
    match provider {
        AgentProvider::ClaudeCode => "sonnet",
        AgentProvider::Cursor => "grok-4.5",
        AgentProvider::Codex => "gpt-5.6-luna",
        AgentProvider::Opencode => "opencode/big-pickle",
        AgentProvider::GithubCopilot => "claude-sonnet-4.5",
        AgentProvider::Gemini => "auto",
        AgentProvider::Grok => "grok-build",
    }
}

/// Static fallback when discovery fails (CLI missing / not logged in).
pub fn fallback_models(provider: AgentProvider) -> ProviderModels {
    let models = match provider {
        AgentProvider::ClaudeCode => vec![
            opt("sonnet", "Sonnet 5", "Claude Code alias"),
            opt("opus", "Opus 5", "Claude Code alias"),
            opt("haiku", "Haiku 4.5", "Claude Code alias"),
            opt("fable", "Fable 5", "Claude Code alias"),
        ],
        AgentProvider::Cursor => cursor_fallback_models(),
        AgentProvider::Codex => vec![
            opt("gpt-5.6-luna", "gpt-5.6-luna", "Codex fallback"),
            opt("gpt-5.6-terra", "gpt-5.6-terra", "Codex fallback"),
            opt("gpt-5.6-sol", "gpt-5.6-sol", "Codex fallback"),
        ],
        AgentProvider::Opencode => vec![opt(
            "opencode/big-pickle",
            "opencode/big-pickle",
            "OpenCode fallback",
        )],
        AgentProvider::GithubCopilot => vec![
            opt(
                "claude-sonnet-4.5",
                "Claude Sonnet 4.5",
                "GitHub Copilot default",
            ),
            opt("claude-opus-4.5", "Claude Opus 4.5", "GitHub Copilot"),
            opt("claude-haiku-4.5", "Claude Haiku 4.5", "GitHub Copilot"),
            opt("gpt-5.3-codex", "GPT-5.3 Codex", "GitHub Copilot"),
            opt("gpt-5.2", "GPT-5.2", "GitHub Copilot"),
        ],
        AgentProvider::Gemini => vec![
            opt("auto", "Auto", "Gemini CLI model routing"),
            opt(
                "gemini-3.1-pro-preview",
                "Gemini 3.1 Pro Preview",
                "Gemini CLI",
            ),
            opt("gemini-3-pro-preview", "Gemini 3 Pro Preview", "Gemini CLI"),
            opt(
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "Gemini CLI",
            ),
            opt("gemini-2.5-pro", "Gemini 2.5 Pro", "Gemini CLI"),
            opt("gemini-2.5-flash", "Gemini 2.5 Flash", "Gemini CLI"),
        ],
        AgentProvider::Grok => vec![
            opt("grok-build", "Grok Build", "Grok CLI coding model"),
            opt("grok-4.5", "Grok 4.5", "Grok CLI"),
            opt("grok-code-fast-1", "Grok Code Fast 1", "Grok CLI"),
        ],
    };

    ProviderModels {
        provider: provider.as_str().into(),
        default_model: default_model(provider).into(),
        models,
        allow_custom: true,
        source: "fallback".into(),
        available: false,
        error: None,
    }
}

/// Discover models from the installed agents (may take a few seconds).
pub fn discover_all() -> Vec<ProviderModels> {
    [
        AgentProvider::ClaudeCode,
        AgentProvider::Cursor,
        AgentProvider::Codex,
        AgentProvider::Opencode,
        AgentProvider::GithubCopilot,
        AgentProvider::Gemini,
        AgentProvider::Grok,
    ]
    .into_iter()
    .map(discover_for)
    .collect()
}

pub fn list_all_provider_models() -> Vec<ProviderModels> {
    discover_all()
}

fn discover_for(provider: AgentProvider) -> ProviderModels {
    let result = match provider {
        AgentProvider::ClaudeCode => discover_claude(),
        AgentProvider::Cursor => discover_cursor(),
        AgentProvider::Codex => discover_codex(),
        AgentProvider::Opencode => discover_opencode(),
        AgentProvider::GithubCopilot => {
            discover_static_cli(AgentProvider::GithubCopilot, "copilot")
        }
        AgentProvider::Gemini => discover_static_cli(AgentProvider::Gemini, "gemini"),
        AgentProvider::Grok => discover_static_cli(AgentProvider::Grok, "grok"),
    };

    match result {
        Ok(mut catalog) => {
            catalog.provider = provider.as_str().into();
            catalog.allow_custom = true;
            if catalog.source.is_empty() {
                catalog.source = "discovered".into();
            }
            catalog.available = true;
            if catalog.default_model.is_empty() {
                catalog.default_model = default_model(provider).into();
            }
            if catalog.models.is_empty() {
                let mut fb = fallback_models(provider);
                fb.error = Some("Agent returned no models".into());
                fb
            } else {
                catalog
            }
        }
        Err(err) => {
            let mut fb = fallback_models(provider);
            fb.error = Some(err);
            fb
        }
    }
}

fn discover_static_cli(provider: AgentProvider, command: &str) -> Result<ProviderModels, String> {
    find_bin(command).ok_or_else(|| format!("{command} CLI not found on PATH"))?;
    let fallback = fallback_models(provider);
    Ok(ProviderModels {
        provider: String::new(),
        default_model: fallback.default_model,
        models: fallback.models,
        allow_custom: true,
        source: "fallback".into(),
        available: true,
        error: None,
    })
}

fn discover_claude() -> Result<ProviderModels, String> {
    find_bin("claude").ok_or_else(|| "claude CLI not found on PATH".to_string())?;

    // `/model` is an interactive slash command. Calling it through `claude
    // -p` just to populate a picker creates a real headless session and can
    // participate in the macOS OAuth refresh race. Claude's aliases are the
    // stable CLI interface, and custom full model IDs remain allowed.
    let fallback = fallback_models(AgentProvider::ClaudeCode);

    Ok(ProviderModels {
        provider: String::new(),
        default_model: fallback.default_model,
        models: fallback.models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

fn discover_opencode() -> Result<ProviderModels, String> {
    let bin = find_bin("opencode").ok_or_else(|| "opencode CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["models"], Duration::from_secs(30))?;
    let mut models = Vec::new();

    for line in output.stdout.lines() {
        let id = line.trim();
        if id.is_empty() || id.starts_with('{') || id.starts_with('[') {
            continue;
        }
        // `opencode models` prints one `provider/model` per line.
        if !id.contains('/') {
            continue;
        }
        models.push(opt(id, id, "Discovered via `opencode models`"));
    }

    if models.is_empty() {
        return Err(format!(
            "no models from opencode (stderr: {})",
            output.stderr.trim()
        ));
    }

    let default = models
        .iter()
        .find(|m| m.id == "opencode/big-pickle")
        .map(|m| m.id.clone())
        .unwrap_or_else(|| models[0].id.clone());

    Ok(ProviderModels {
        provider: String::new(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

fn discover_codex() -> Result<ProviderModels, String> {
    let home = BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .ok_or_else(|| "could not resolve home directory".to_string())?;

    let cache = home.join(".codex/models_cache.json");
    if !cache.is_file() {
        // Try CLI if present.
        if let Some(bin) = find_bin("codex") {
            let _ = bin;
        }
        return Err(format!(
            "Codex model cache not found at {}",
            cache.display()
        ));
    }

    let raw = std::fs::read_to_string(&cache).map_err(|e| e.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid models_cache.json: {e}"))?;

    let list = value
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "models_cache.json missing models[]".to_string())?;

    let mut models = Vec::new();
    for item in list {
        let id = item
            .get("slug")
            .or_else(|| item.get("id"))
            .or_else(|| item.get("model"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if id.is_empty() {
            continue;
        }
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("From ~/.codex/models_cache.json")
            .to_string();
        let label = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();
        models.push(opt(id, label, description));
    }

    if models.is_empty() {
        return Err("Codex models_cache.json contained no models".into());
    }

    let default = read_codex_default_model(&home)
        .or_else(|| models.first().map(|m| m.id.clone()))
        .unwrap_or_else(|| default_model(AgentProvider::Codex).into());

    Ok(ProviderModels {
        provider: String::new(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

fn read_codex_default_model(home: &Path) -> Option<String> {
    let config = home.join(".codex/config.toml");
    let text = std::fs::read_to_string(config).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("model ") {
            // model = "gpt-5.6-luna"
            if let Some(val) = rest.split('=').nth(1) {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        if let Some(rest) = line.strip_prefix("model=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn cursor_fallback_models() -> Vec<ModelOption> {
    [
        ("default", "Auto", "Cursor Auto"),
        ("grok-4.5", "Cursor Grok 4.5", "Cursor Grok"),
        ("composer-2.5", "Composer 2.5", "Cursor Composer"),
        ("claude-opus-5", "Opus 5", "Anthropic"),
        ("claude-sonnet-5", "Sonnet 5", "Anthropic"),
        ("claude-fable-5", "Fable 5", "Anthropic"),
        ("gpt-5.6-sol", "GPT-5.6 Sol", "OpenAI"),
        ("gpt-5.6-terra", "GPT-5.6 Terra", "OpenAI"),
        ("gpt-5.6-luna", "GPT-5.6 Luna", "OpenAI"),
        ("gpt-5.5", "GPT-5.5", "OpenAI"),
        ("gpt-5.3-codex", "Codex 5.3", "OpenAI"),
        ("claude-sonnet-4-6", "Sonnet 4.6", "Anthropic"),
        ("claude-opus-4-6", "Opus 4.6", "Anthropic"),
        ("claude-haiku-4-5", "Haiku 4.5", "Anthropic"),
        ("gemini-3.1-pro", "Gemini 3.1 Pro", "Google"),
        ("gemini-3.6-flash", "Gemini 3.6 Flash", "Google"),
        ("kimi-k2.7-code", "Kimi K2.7 Code", "Moonshot"),
        (
            "cursor-grok-4.5-high",
            "Cursor Grok 4.5 High",
            "Legacy slug",
        ),
    ]
    .into_iter()
    .map(|(id, label, desc)| opt(id, label, desc))
    .collect()
}

fn discover_cursor() -> Result<ProviderModels, String> {
    let mut errors = Vec::new();

    match discover_cursor_from_cli() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("CLI returned no models".into()),
        Err(e) => errors.push(e),
    }

    match discover_cursor_from_ide_state() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("Cursor IDE state had no agent models".into()),
        Err(e) => errors.push(e),
    }

    Err(format!(
        "could not discover Cursor models ({})",
        errors.join("; ")
    ))
}

fn discover_cursor_from_cli() -> Result<ProviderModels, String> {
    let bin = find_bin("cursor-agent")
        .or_else(|| find_bin("agent"))
        .ok_or_else(|| "cursor-agent / agent CLI not found on PATH".to_string())?;

    let attempts: &[&[&str]] = &[&["models"], &["--list-models"], &["model", "list"]];
    let mut last_err = String::new();

    for args in attempts {
        match run_cmd(&bin, args, Duration::from_secs(20)) {
            Ok(output) => {
                let text = format!("{}\n{}", output.stdout, output.stderr);
                // Old CLIs open an interactive login UI instead of listing models.
                if text.contains("Press any key to sign in")
                    || text.contains("not in the list of known options")
                    || text.contains("version is too old")
                {
                    last_err = format!(
                        "`{} {}` unsupported on this install",
                        bin.display(),
                        args.join(" ")
                    );
                    continue;
                }
                let models = parse_cursor_cli_models(&text);
                if models.is_empty() {
                    last_err = format!(
                        "`{} {}` produced no model ids",
                        bin.display(),
                        args.join(" ")
                    );
                    continue;
                }
                return Ok(cursor_catalog_from_models(
                    models,
                    "Discovered via cursor-agent CLI",
                ));
            }
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

fn parse_cursor_cli_models(text: &str) -> Vec<ModelOption> {
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Common formats:
        // - `grok-4.5`
        // - `grok-4.5  Cursor Grok 4.5`
        // - `- grok-4.5 (Cursor Grok 4.5)`
        let cleaned = line.trim_start_matches(['-', '*', '•', '·']).trim();
        let mut parts = cleaned.split_whitespace();
        let Some(raw_id) = parts.next() else {
            continue;
        };
        let id = raw_id
            .trim_matches(|c: char| {
                c == '`' || c == ',' || c == '(' || c == ')' || c == '"' || c == '\''
            })
            .to_string();
        if !looks_like_cursor_model_id(&id) || !seen.insert(id.clone()) {
            continue;
        }
        let rest = parts.collect::<Vec<_>>().join(" ");
        let label = strip_html_tags(
            rest.trim_matches(|c: char| c == '(' || c == ')' || c == '-' || c == ':')
                .trim(),
        );
        let label = if label.is_empty() { id.clone() } else { label };
        models.push(opt(id, label, "From cursor-agent models"));
    }

    models
}

fn looks_like_cursor_model_id(id: &str) -> bool {
    if id.len() < 3 || id.len() > 80 || id.contains('/') || id.contains(' ') {
        return false;
    }
    let lower = id.to_ascii_lowercase();
    // Reject help noise.
    if matches!(
        lower.as_str(),
        "model" | "models" | "available" | "name" | "id" | "usage" | "error" | "warning"
    ) {
        return false;
    }
    lower.contains('-')
        || lower.starts_with("gpt")
        || lower.starts_with("claude")
        || lower.starts_with("gemini")
        || lower.starts_with("composer")
        || lower.starts_with("grok")
        || lower.starts_with("kimi")
        || lower.starts_with("glm")
        || lower == "default"
        || lower == "auto"
}

fn discover_cursor_from_ide_state() -> Result<ProviderModels, String> {
    let db_path = cursor_state_db_path()
        .ok_or_else(|| "Cursor IDE state.vscdb not found (is Cursor installed?)".to_string())?;

    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;

    let key = "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";
    let raw: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .map_err(|e| format!("Cursor model catalog missing in state.vscdb: {e}"))?;

    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid Cursor state JSON: {e}"))?;

    let list = value
        .get("availableDefaultModels2")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "availableDefaultModels2 missing in Cursor state".to_string())?;

    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for item in list {
        // Prefer models that work in Agent mode.
        if let Some(supports) = item.get("supportsAgent") {
            if supports.as_bool() == Some(false) {
                continue;
            }
        }

        let base_id = item
            .get("name")
            .or_else(|| item.get("serverModelName"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if base_id.is_empty() {
            continue;
        }

        let base_label = item
            .get("clientDisplayName")
            .or_else(|| item.get("inputboxShortModelName"))
            .and_then(|v| v.as_str())
            .map(strip_html_tags)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base_id.to_string());

        let vendor = item
            .get("vendor")
            .and_then(|v| v.get("displayName"))
            .or_else(|| item.get("vendorName"))
            .and_then(|v| v.as_str())
            .unwrap_or("Cursor IDE");

        let default_on = item
            .get("defaultOn")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let description = if default_on {
            format!("{vendor} · shown by default")
        } else {
            format!("{vendor} · from Cursor Settings → Models")
        };

        let mut pushed_variant = false;
        if let Some(variants) = item.get("variants").and_then(|v| v.as_array()) {
            for variant in variants {
                let variant_id = variant
                    .get("legacySlug")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        variant
                            .get("variantStringRepresentation")
                            .and_then(|v| v.as_str())
                    })
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .unwrap_or(base_id);
                if !seen.insert(variant_id.to_string()) {
                    continue;
                }
                let variant_label = variant
                    .get("displayNameOutsidePicker")
                    .or_else(|| variant.get("displayName"))
                    .and_then(|v| v.as_str())
                    .map(strip_html_tags)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| base_label.clone());
                models.push(ModelOption {
                    id: variant_id.to_string(),
                    label: variant_label,
                    description: description.clone(),
                });
                pushed_variant = true;
            }
        }
        if !pushed_variant && seen.insert(base_id.to_string()) {
            models.push(ModelOption {
                id: base_id.to_string(),
                label: base_label,
                description,
            });
        }
    }

    if models.is_empty() {
        return Err("no agent-capable models in Cursor IDE catalog".into());
    }

    // Keep IDE order (already curated), but float defaultOn models first.
    models.sort_by_key(|m| {
        let default_first = !m.description.contains("shown by default");
        (default_first, m.label.to_ascii_lowercase())
    });

    let default = value
        .pointer("/aiSettings/composerModel")
        .and_then(|v| v.as_str())
        .and_then(|selected| {
            models
                .iter()
                .find(|m| m.id == selected)
                .or_else(|| {
                    // IDE stores base ids like `grok-4.5`, while the picker can
                    // expose variant slugs like `cursor-grok-4.5-high-fast`.
                    models
                        .iter()
                        .find(|m| m.id.contains(selected) && !m.id.ends_with("-fast"))
                })
                .or_else(|| {
                    models
                        .iter()
                        .find(|m| m.id.contains(selected) && m.id.ends_with("-fast"))
                })
                .map(|m| m.id.clone())
        })
        .or_else(|| {
            models
                .iter()
                .find(|m| m.id == "grok-4.5" || m.id == "composer-2.5" || m.id == "default")
                .map(|m| m.id.clone())
        })
        .unwrap_or_else(|| models[0].id.clone());

    Ok(ProviderModels {
        provider: String::new(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

fn cursor_catalog_from_models(models: Vec<ModelOption>, _source: &str) -> ProviderModels {
    let default = models
        .iter()
        .find(|m| m.id == "grok-4.5" || m.id == "composer-2.5" || m.id == "default")
        .map(|m| m.id.clone())
        .unwrap_or_else(|| models[0].id.clone());

    ProviderModels {
        provider: String::new(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    }
}

fn cursor_state_db_path() -> Option<PathBuf> {
    let home = BaseDirs::new()?.home_dir().to_path_buf();
    let candidates = [
        home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"),
        home.join(".config/Cursor/User/globalStorage/state.vscdb"),
        home.join("AppData/Roaming/Cursor/User/globalStorage/state.vscdb"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\u{200b}' => {}
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct CmdOutput {
    stdout: String,
    stderr: String,
}

fn run_cmd(bin: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
    // Prefer a thread+kill approach without extra deps: spawn and wait with timeout via try_wait loop.
    let mut child = Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} timed out after {}s",
                        bin.display(),
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed for {}: {e}", bin.display())),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed reading output from {}: {e}", bin.display()))?;

    Ok(CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn find_bin(name: &str) -> Option<PathBuf> {
    // 1) PATH
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(path);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    // 2) Common install locations (Tauri apps often lack shell PATH).
    let mut candidates = Vec::new();
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        candidates.push(home.join(format!(".local/bin/{name}")));
        candidates.push(home.join(format!(".opencode/bin/{name}")));
        candidates.push(home.join(format!(".cargo/bin/{name}")));
        candidates.push(home.join(format!("bin/{name}")));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/bin/{name}")));

    candidates.into_iter().find(|p| p.is_file())
}
