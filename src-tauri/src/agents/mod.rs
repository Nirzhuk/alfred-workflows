pub mod active;
pub mod activity;
pub mod auth;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod github_copilot;
pub mod grok;
pub mod models;
pub mod omp;
pub mod opencode;
pub mod pi;
pub(crate) mod process;
pub mod usage;

use crate::skills::{compose_prompt_with_skills, SkillRef};
use active::RunControl;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use activity::{AgentActivity, AgentActivityKind, AgentActivityState};
pub use auth::{auth_required, AgentAuthRequired};
pub use models::{list_all_provider_models, ProviderModels};
pub use usage::{list_provider_usage, AgentUsageSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    ClaudeCode,
    Cursor,
    Codex,
    Opencode,
    GithubCopilot,
    Gemini,
    Grok,
    Pi,
    Omp,
}

impl AgentProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::GithubCopilot => "github_copilot",
            Self::Gemini => "gemini",
            Self::Grok => "grok",
            Self::Pi => "pi",
            Self::Omp => "omp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::Opencode => "OpenCode",
            Self::GithubCopilot => "GitHub Copilot",
            Self::Gemini => "Gemini",
            Self::Grok => "Grok",
            Self::Pi => "Pi",
            Self::Omp => "OMP",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Self::ClaudeCode),
            "cursor" => Some(Self::Cursor),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "github_copilot" => Some(Self::GithubCopilot),
            "gemini" => Some(Self::Gemini),
            "grok" => Some(Self::Grok),
            "pi" => Some(Self::Pi),
            "omp" => Some(Self::Omp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    pub prompt: String,
    /// CLI model id / alias (`--model`).
    #[serde(default)]
    pub model: Option<String>,
    /// Optional skill to invoke for this step (e.g. `tdd` → `/tdd …`).
    #[serde(default)]
    pub skill: Option<SkillRef>,
    /// Legacy single skill name.
    #[serde(default)]
    pub skill_name: Option<String>,
    /// Preferred: multiple skills → `/a /b …`.
    #[serde(default)]
    pub skill_names: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

impl AgentRequest {
    fn resolved_skill_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .skill_names
            .iter()
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .collect();
        if names.is_empty() {
            if let Some(ref skill) = self.skill {
                if !skill.name.trim().is_empty() {
                    names.push(skill.name.as_str());
                }
            }
        }
        if names.is_empty() {
            if let Some(ref name) = self.skill_name {
                if !name.trim().is_empty() {
                    names.push(name.as_str());
                }
            }
        }
        names
    }

    /// Final prompt sent to the CLI, with skill slash-commands applied when set.
    pub fn effective_prompt(&self) -> String {
        let names = self.resolved_skill_names();
        if names.is_empty() {
            self.prompt.clone()
        } else {
            compose_prompt_with_skills(&names, &self.prompt)
        }
    }

    pub fn effective_model(&self, provider: AgentProvider) -> String {
        self.model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| models::default_model(provider).to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    pub output: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Message(String),
}

/// Live-run hooks shared by CLI adapters (cancel + safe activity streaming).
pub struct AgentRunHooks<'a> {
    pub control: Option<&'a RunControl>,
    pub on_activity: Option<&'a dyn Fn(&AgentActivity)>,
}

pub trait AgentAdapter: Send + Sync {
    fn provider(&self) -> AgentProvider;
    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError>;
}

pub fn adapter_for(provider: AgentProvider) -> Box<dyn AgentAdapter> {
    match provider {
        AgentProvider::ClaudeCode => Box::new(claude_code::ClaudeCodeAdapter),
        AgentProvider::Cursor => Box::new(cursor::CursorAdapter),
        AgentProvider::Codex => Box::new(codex::CodexAdapter),
        AgentProvider::Opencode => Box::new(opencode::OpencodeAdapter),
        AgentProvider::GithubCopilot => Box::new(github_copilot::GithubCopilotAdapter),
        AgentProvider::Gemini => Box::new(gemini::GeminiAdapter),
        AgentProvider::Grok => Box::new(grok::GrokAdapter),
        AgentProvider::Pi => Box::new(pi::PiAdapter),
        AgentProvider::Omp => Box::new(omp::OmpAdapter),
    }
}

pub fn list_providers() -> Vec<serde_json::Value> {
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
    .map(|p| {
        serde_json::json!({
            "id": p.as_str(),
            "label": p.label(),
            "defaultModel": models::default_model(p),
        })
    })
    .collect()
}

pub(crate) fn map_cmd_err(err: String) -> AgentError {
    if err == "cancelled" {
        AgentError::Cancelled
    } else {
        AgentError::Message(err)
    }
}
