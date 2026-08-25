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
//! - pi: `pi --list-models` (authenticated providers only)
//! - OMP: `omp models --json`

use super::pi;
use super::AgentProvider;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_variant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_fast_variant: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_fast_toggle: Option<bool>,
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
        base_id: None,
        fast_variant_id: None,
        is_fast_variant: None,
        supports_fast_toggle: None,
    }
}

fn fast_base_id(id: &str) -> Option<&str> {
    id.strip_suffix("-fast").filter(|base| !base.is_empty())
}

fn is_fast_variant(model: &ModelOption) -> bool {
    model.is_fast_variant == Some(true) || fast_base_id(&model.id).is_some()
}

/// Collapse confident base/fast pairs into one picker row.
///
/// A pair is accepted only when one base maps to one fast variant and that
/// fast variant maps back to the same base. Ambiguous or incomplete matches
/// remain flat so discovery never silently changes a model id.
pub fn pair_fast_variants(models: Vec<ModelOption>) -> Vec<ModelOption> {
    if models.len() < 2 {
        return models;
    }

    let mut indices_by_id: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, model) in models.iter().enumerate() {
        indices_by_id
            .entry(model.id.clone())
            .or_default()
            .push(index);
    }

    let mut base_to_fast: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut fast_to_base: HashMap<usize, Vec<usize>> = HashMap::new();

    for (base_index, model) in models.iter().enumerate() {
        if is_fast_variant(model) {
            continue;
        }
        let Some(fast_id) = model.fast_variant_id.as_deref() else {
            continue;
        };
        let Some(fast_indices) = indices_by_id.get(fast_id) else {
            continue;
        };
        for &fast_index in fast_indices {
            if fast_index == base_index {
                continue;
            }
            base_to_fast.entry(base_index).or_default().push(fast_index);
            fast_to_base.entry(fast_index).or_default().push(base_index);
        }
    }

    for (fast_index, model) in models.iter().enumerate() {
        if !is_fast_variant(model) {
            continue;
        }
        let base_id = model.base_id.as_deref().or_else(|| fast_base_id(&model.id));
        let Some(base_id) = base_id else {
            continue;
        };
        let Some(base_indices) = indices_by_id.get(base_id) else {
            continue;
        };
        for &base_index in base_indices {
            if base_index == fast_index || is_fast_variant(&models[base_index]) {
                continue;
            }
            base_to_fast.entry(base_index).or_default().push(fast_index);
            fast_to_base.entry(fast_index).or_default().push(base_index);
        }
    }

    for indices in base_to_fast.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }
    for indices in fast_to_base.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }

    let mut fast_id_by_base = HashMap::new();
    let mut paired_fast = vec![false; models.len()];
    for (&base_index, fast_indices) in &base_to_fast {
        if fast_indices.len() != 1 {
            continue;
        }
        let fast_index = fast_indices[0];
        let Some(base_indices) = fast_to_base.get(&fast_index) else {
            continue;
        };
        if base_indices.len() != 1 || base_indices[0] != base_index {
            continue;
        }
        fast_id_by_base.insert(base_index, models[fast_index].id.clone());
        paired_fast[fast_index] = true;
    }

    if fast_id_by_base.is_empty() {
        return models;
    }

    let mut paired_models = Vec::with_capacity(models.len());
    for (index, mut model) in models.into_iter().enumerate() {
        if paired_fast[index] {
            continue;
        }
        if let Some(fast_id) = fast_id_by_base.get(&index) {
            model.base_id = Some(model.id.clone());
            model.fast_variant_id = Some(fast_id.clone());
            model.is_fast_variant = Some(false);
            model.supports_fast_toggle = Some(true);
        }
        paired_models.push(model);
    }
    paired_models
}

#[cfg(test)]
fn test_model(id: &str) -> ModelOption {
    opt(id, id, "")
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
        // pi and OMP reach 15+ providers, so the only model id valid on every
        // install is the one the user already configured.
        AgentProvider::Pi | AgentProvider::Omp => pi::CLI_DEFAULT_MODEL,
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
        AgentProvider::Pi => vec![
            opt(
                pi::CLI_DEFAULT_MODEL,
                "CLI default",
                "Model configured in pi",
            ),
            opt("anthropic/claude-sonnet-5", "Claude Sonnet 5", "pi"),
            opt("openai/gpt-5.6-luna", "GPT-5.6 Luna", "pi"),
            opt("google/gemini-3-pro-preview", "Gemini 3 Pro Preview", "pi"),
        ],
        AgentProvider::Omp => vec![
            opt(
                pi::CLI_DEFAULT_MODEL,
                "CLI default",
                "Model configured in OMP",
            ),
            opt("anthropic/claude-sonnet-5", "Claude Sonnet 5", "OMP"),
            opt("openai/gpt-5.6-luna", "GPT-5.6 Luna", "OMP"),
            opt("google/gemini-3-pro-preview", "Gemini 3 Pro Preview", "OMP"),
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
        AgentProvider::Pi,
        AgentProvider::Omp,
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
        AgentProvider::Pi => discover_pi(),
        AgentProvider::Omp => discover_omp(),
    };

    match result {
        Ok(mut catalog) => {
            catalog.models = pair_fast_variants(catalog.models);
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

/// `pi --list-models` prints a padded table whose first row is the header and
/// whose first two columns are `provider` and `model`. pi accepts the pair as
/// `provider/id`, which stays unambiguous when two providers share a model.
fn discover_pi() -> Result<ProviderModels, String> {
    let bin = find_bin("pi").ok_or_else(|| "pi CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["--list-models"], Duration::from_secs(30))?;
    let models = parse_pi_model_table(&output.stdout);

    if models.is_empty() {
        return Err(format!(
            "no models from pi (stderr: {})",
            output.stderr.trim()
        ));
    }

    Ok(pi_family_catalog(models))
}

fn parse_pi_model_table(stdout: &str) -> Vec<ModelOption> {
    let mut models = Vec::new();
    for line in stdout.lines() {
        let mut columns = line.split_whitespace();
        let (Some(provider), Some(id)) = (columns.next(), columns.next()) else {
            continue;
        };
        if provider == "provider" || id == "model" {
            continue;
        }
        let selector = format!("{provider}/{id}");
        models.push(opt(
            &selector,
            &selector,
            "Discovered via `pi --list-models`",
        ));
    }
    models
}

/// `omp models --json` returns `{ "models": [{ selector, name, ... }] }`.
fn discover_omp() -> Result<ProviderModels, String> {
    let bin = find_bin("omp").ok_or_else(|| "omp CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["models", "--json"], Duration::from_secs(30))?;
    let models = parse_omp_model_json(&output.stdout)?;

    if models.is_empty() {
        return Err(format!(
            "no models from omp (stderr: {})",
            output.stderr.trim()
        ));
    }

    Ok(pi_family_catalog(models))
}

fn parse_omp_model_json(stdout: &str) -> Result<Vec<ModelOption>, String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("invalid `omp models --json`: {e}"))?;
    let list = value
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "`omp models --json` missing models[]".to_string())?;

    let mut models = Vec::new();
    for item in list {
        let Some(selector) = item.get("selector").and_then(|v| v.as_str()) else {
            continue;
        };
        let label = item
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(selector);
        models.push(opt(
            selector,
            format!("{label} ({selector})"),
            "Discovered via `omp models`",
        ));
    }
    Ok(models)
}

/// Keep the "CLI default" entry first so a workflow can defer to whatever the
/// user configured instead of pinning a provider they may not be logged into.
fn pi_family_catalog(discovered: Vec<ModelOption>) -> ProviderModels {
    let mut models = vec![opt(
        pi::CLI_DEFAULT_MODEL,
        "CLI default",
        "Model configured in the CLI",
    )];
    models.extend(discovered);

    ProviderModels {
        provider: String::new(),
        default_model: pi::CLI_DEFAULT_MODEL.into(),
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    }
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

    // Cursor IDE carries curated labels and parameterized variants; use the
    // CLI only when the IDE catalog is unavailable.
    match discover_cursor_from_ide_state() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("Cursor IDE state had no agent models".into()),
        Err(e) => errors.push(e),
    }

    match discover_cursor_from_cli() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("CLI returned no models".into()),
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

fn cursor_variant_is_fast(variant: &serde_json::Value, id: &str) -> bool {
    if fast_base_id(id).is_some() {
        return true;
    }
    variant
        .get("parameterValues")
        .and_then(|values| values.as_array())
        .is_some_and(|values| {
            values.iter().any(|parameter| {
                parameter.get("id").and_then(|value| value.as_str()) == Some("fast")
                    && (parameter.get("value").and_then(|value| value.as_bool()) == Some(true)
                        || parameter.get("value").and_then(|value| value.as_str()) == Some("true"))
            })
        })
}

fn cursor_variant_id<'a>(variant: &'a serde_json::Value, fallback: &'a str) -> &'a str {
    variant
        .get("legacySlug")
        .and_then(|value| value.as_str())
        .or_else(|| {
            variant
                .get("variantStringRepresentation")
                .and_then(|value| value.as_str())
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(fallback)
}

fn cursor_variant_pair_key(variant: &serde_json::Value) -> Option<String> {
    let values = variant.get("parameterValues")?.as_array()?;
    let mut parameters = values
        .iter()
        .filter_map(|parameter| {
            let id = parameter.get("id").and_then(|value| value.as_str())?;
            if id == "fast" {
                return None;
            }
            let value = parameter.get("value")?;
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            Some(format!("{id}={value}"))
        })
        .collect::<Vec<_>>();
    parameters.sort_unstable();
    Some(parameters.join(","))
}

fn cursor_variant_counterpart_id(
    variant: &serde_json::Value,
    variant_id: &str,
    variants: &[serde_json::Value],
) -> Option<String> {
    let key = cursor_variant_pair_key(variant)?;
    let mut counterpart_id = None;
    for candidate in variants {
        let candidate_id = cursor_variant_id(candidate, "");
        if candidate_id.is_empty()
            || candidate_id == variant_id
            || cursor_variant_is_fast(candidate, candidate_id)
        {
            continue;
        }
        let Some(candidate_key) = cursor_variant_pair_key(candidate) else {
            continue;
        };
        if candidate_key != key {
            continue;
        }
        if counterpart_id.is_some() {
            return None;
        }
        counterpart_id = Some(candidate_id.to_string());
    }
    counterpart_id
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
                let variant_id = cursor_variant_id(variant, base_id);
                let is_fast = cursor_variant_is_fast(variant, variant_id);
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
                let mut option = opt(variant_id, variant_label, description.clone());
                if is_fast {
                    option.is_fast_variant = Some(true);
                    option.base_id = fast_base_id(variant_id)
                        .map(str::to_owned)
                        .or_else(|| cursor_variant_counterpart_id(variant, variant_id, variants));
                }
                models.push(option);
                pushed_variant = true;
            }
        }
        if !pushed_variant && seen.insert(base_id.to_string()) {
            models.push(opt(base_id, base_label, description));
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

#[cfg(test)]
mod pi_family_tests {
    use super::*;

    #[test]
    fn parses_the_pi_model_table_and_skips_its_header() {
        let stdout = [
            "provider   model              context  max-out  thinking  images",
            "anthropic  claude-sonnet-5    1M       128K     yes       yes",
            "openai     gpt-5.6-luna       272K     128K     yes       yes",
        ]
        .join("\n");

        let models = parse_pi_model_table(&stdout);
        let ids = models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["anthropic/claude-sonnet-5", "openai/gpt-5.6-luna"]
        );
    }

    #[test]
    fn parses_omp_model_json_by_selector() {
        let stdout = r#"{"models":[
            {"provider":"anthropic","id":"claude-opus-5","selector":"anthropic/claude-opus-5","name":"Claude Opus 5"},
            {"provider":"openai","id":"gpt-5.2","selector":"openai/gpt-5.2"},
            {"provider":"broken","id":"no-selector"}
        ]}"#;

        let models = parse_omp_model_json(stdout).expect("valid json");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "anthropic/claude-opus-5");
        assert_eq!(models[0].label, "Claude Opus 5 (anthropic/claude-opus-5)");
        // Missing `name` falls back to the selector on both sides.
        assert_eq!(models[1].label, "openai/gpt-5.2 (openai/gpt-5.2)");
        assert!(parse_omp_model_json("not json").is_err());
    }

    #[test]
    fn catalogs_offer_the_cli_default_first() {
        let catalog = pi_family_catalog(vec![opt("anthropic/x", "X", "")]);
        assert_eq!(catalog.default_model, pi::CLI_DEFAULT_MODEL);
        assert_eq!(catalog.models[0].id, pi::CLI_DEFAULT_MODEL);
        assert!(catalog.allow_custom);
    }
    #[test]
    fn pairs_one_suffix_variant_and_removes_the_duplicate_row() {
        let models = pair_fast_variants(vec![
            test_model("gpt-5.3-codex-high"),
            test_model("gpt-5.3-codex-high-fast"),
            test_model("gpt-5.2"),
        ]);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.3-codex-high", "gpt-5.2"]
        );
        assert_eq!(models[0].base_id.as_deref(), Some("gpt-5.3-codex-high"));
        assert_eq!(
            models[0].fast_variant_id.as_deref(),
            Some("gpt-5.3-codex-high-fast")
        );
        assert_eq!(models[0].supports_fast_toggle, Some(true));
    }

    #[test]
    fn keeps_ambiguous_pairs_flat() {
        let mut fast_low = test_model("cursor-grok-low-priority");
        fast_low.is_fast_variant = Some(true);
        fast_low.base_id = Some("cursor-grok".into());
        let mut fast_high = test_model("cursor-grok-high-priority");
        fast_high.is_fast_variant = Some(true);
        fast_high.base_id = Some("cursor-grok".into());

        let models = pair_fast_variants(vec![test_model("cursor-grok"), fast_low, fast_high]);

        assert_eq!(models.len(), 3);
        assert!(models
            .iter()
            .all(|model| model.supports_fast_toggle != Some(true)));
    }

    #[test]
    fn keeps_missing_half_pairs_flat() {
        let models = pair_fast_variants(vec![
            test_model("gpt-5.3-codex-fast"),
            test_model("gpt-5.2"),
        ]);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.3-codex-fast", "gpt-5.2"]
        );
        assert!(models
            .iter()
            .all(|model| model.supports_fast_toggle != Some(true)));
    }

    #[test]
    fn pairs_explicit_mapper_metadata_without_a_fast_suffix() {
        let mut base = test_model("cursor-grok");
        base.fast_variant_id = Some("cursor-grok-priority".into());
        let mut fast = test_model("cursor-grok-priority");
        fast.is_fast_variant = Some(true);
        fast.base_id = Some("cursor-grok".into());

        let models = pair_fast_variants(vec![base, fast]);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "cursor-grok");
        assert_eq!(
            models[0].fast_variant_id.as_deref(),
            Some("cursor-grok-priority")
        );
        assert_eq!(models[0].supports_fast_toggle, Some(true));
    }
    #[test]
    fn detects_cursor_fast_parameter_values() {
        let fast = serde_json::json!({
            "parameterValues": [{"id": "fast", "value": "true"}]
        });
        let base = serde_json::json!({
            "parameterValues": [{"id": "fast", "value": "false"}]
        });

        assert!(cursor_variant_is_fast(&fast, "cursor-grok-high"));
        assert!(!cursor_variant_is_fast(&base, "cursor-grok-high"));
    }
    #[test]
    fn matches_parameterized_cursor_fast_variant_to_its_counterpart() {
        let base = serde_json::json!({
            "variantStringRepresentation": "grok-4.5[effort=high,fast=false]",
            "parameterValues": [
                {"id": "effort", "value": "high"},
                {"id": "fast", "value": "false"}
            ]
        });
        let fast = serde_json::json!({
            "variantStringRepresentation": "grok-4.5[effort=high,fast=true]",
            "parameterValues": [
                {"id": "effort", "value": "high"},
                {"id": "fast", "value": "true"}
            ]
        });
        let variants = vec![base.clone(), fast.clone()];

        assert_eq!(
            cursor_variant_counterpart_id(&fast, "grok-4.5[effort=high,fast=true]", &variants,)
                .as_deref(),
            Some("grok-4.5[effort=high,fast=false]")
        );
    }
}
