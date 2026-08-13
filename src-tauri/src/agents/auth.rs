use serde::{Deserialize, Serialize};

use super::AgentProvider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthRequired {
    pub provider: AgentProvider,
    pub label: String,
    pub login_command: String,
}

pub fn auth_required(provider: AgentProvider, message: &str) -> Option<AgentAuthRequired> {
    let normalized = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    let recognized = match provider {
        AgentProvider::ClaudeCode => {
            normalized.contains("failed to authenticate")
                || normalized.contains("oauth session expired")
                || (normalized.contains("oauth") && normalized.contains("could not be refreshed"))
                || (normalized.contains("oauth")
                    && normalized.contains("refresh token")
                    && (normalized.contains("expired") || normalized.contains("invalid")))
        }
        AgentProvider::Cursor => {
            normalized.contains("press any key to sign in")
                || normalized.contains("not authenticated")
                || normalized.contains("not logged in")
                || normalized.contains("authentication required")
        }
        AgentProvider::Codex | AgentProvider::Opencode => {
            normalized.contains("not logged in")
                || normalized.contains("login required")
                || normalized.contains("authentication required")
                || (normalized.contains("401") && normalized.contains("unauthorized"))
                || (normalized.contains("api key")
                    && (normalized.contains("missing")
                        || normalized.contains("invalid")
                        || normalized.contains("required")))
        }
    };

    recognized.then(|| hint_for_provider(provider))
}

fn hint_for_provider(provider: AgentProvider) -> AgentAuthRequired {
    let (label, login_command) = match provider {
        AgentProvider::ClaudeCode => ("Claude Code", "claude auth login"),
        AgentProvider::Cursor => (
            "Cursor",
            cursor_login_command(super::process::find_bin("cursor-agent").is_some()),
        ),
        AgentProvider::Codex => ("Codex", "codex login"),
        AgentProvider::Opencode => ("OpenCode", "opencode auth login"),
    };

    AgentAuthRequired {
        provider,
        label: label.to_string(),
        login_command: login_command.to_string(),
    }
}

fn cursor_login_command(cursor_agent_installed: bool) -> &'static str {
    if cursor_agent_installed {
        "cursor-agent login"
    } else {
        "agent login"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_auth(provider: AgentProvider, message: &str, command: &str) {
        let auth = auth_required(provider, message).expect("expected auth hint");
        assert_eq!(auth.provider, provider);
        assert_eq!(auth.label, provider.label());
        if provider != AgentProvider::Cursor {
            assert_eq!(auth.login_command, command);
        }
    }

    #[test]
    fn recognizes_claude_auth_signatures() {
        for message in [
            "Failed to authenticate with Claude",
            "OAuth session expired",
            "OAuth credentials could not be refreshed",
            "OAuth refresh token expired",
            "OAuth refresh token is invalid",
        ] {
            assert_auth(AgentProvider::ClaudeCode, message, "claude auth login");
        }
    }

    #[test]
    fn recognizes_cursor_auth_signatures() {
        for message in [
            "Press any key to sign in",
            "Not authenticated",
            "You are not logged in",
            "Authentication required",
        ] {
            assert_auth(AgentProvider::Cursor, message, "");
        }
    }

    #[test]
    fn recognizes_codex_and_opencode_auth_signatures() {
        for provider in [AgentProvider::Codex, AgentProvider::Opencode] {
            for message in [
                "Not logged in",
                "Login required",
                "Authentication required",
                "HTTP 401: Unauthorized",
                "API key is missing",
                "Invalid API key",
                "API key required",
            ] {
                assert_auth(
                    provider,
                    message,
                    if provider == AgentProvider::Codex {
                        "codex login"
                    } else {
                        "opencode auth login"
                    },
                );
            }
        }
    }

    #[test]
    fn normalizes_case_and_whitespace() {
        assert_auth(
            AgentProvider::ClaudeCode,
            "OAUTH\n\trefresh token   IS INVALID",
            "claude auth login",
        );
        assert_auth(
            AgentProvider::Codex,
            "HTTP 401\n\tUNAUTHORIZED",
            "codex login",
        );
    }

    #[test]
    fn chooses_both_cursor_command_aliases() {
        assert_eq!(cursor_login_command(true), "cursor-agent login");
        assert_eq!(cursor_login_command(false), "agent login");
    }

    #[test]
    fn rejects_unrelated_failures() {
        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Cursor,
            AgentProvider::Codex,
            AgentProvider::Opencode,
        ] {
            for message in [
                "Agent execution failed",
                "Network connection failed",
                "Rate limit exceeded",
                "Model is unavailable",
                "Permission denied",
                "CLI not found: agent",
                "Cancelled by user",
                "OAuth token was received",
                "Authentication service temporarily unavailable",
                "HTTP 401 from upstream proxy",
            ] {
                assert_eq!(
                    auth_required(provider, message),
                    None,
                    "{provider:?}: {message}"
                );
            }
        }
    }
}
