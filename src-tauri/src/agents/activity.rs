use serde::{Deserialize, Serialize};

const MAX_LABEL_CHARS: usize = 160;
const MAX_DETAIL_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityKind {
    Status,
    Assistant,
    Tool,
    Command,
    File,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityState {
    Started,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivity {
    pub id: String,
    pub kind: AgentActivityKind,
    pub state: AgentActivityState,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AgentActivity {
    pub fn new(
        id: impl Into<String>,
        kind: AgentActivityKind,
        state: AgentActivityState,
        label: impl AsRef<str>,
        detail: Option<&str>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            state,
            label: normalize_label(label.as_ref()),
            detail: detail
                .map(redact_detail)
                .filter(|value| !value.trim().is_empty()),
        }
    }
}

fn normalize_label(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect()
}

fn redact_detail(value: &str) -> String {
    if looks_like_environment_dump(value) {
        return "Environment output hidden".to_string();
    }
    let mut redacted = value
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n");

    while redacted.len() > MAX_DETAIL_BYTES {
        redacted.pop();
    }
    redacted
}

fn looks_like_environment_dump(value: &str) -> bool {
    let lines = value.lines().filter(|line| !line.trim().is_empty());
    let mut total = 0_usize;
    let mut assignments = 0_usize;
    for line in lines.take(64) {
        total += 1;
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.len() >= 2
            && key
                .chars()
                .all(|character| character.is_ascii_uppercase() || character == '_')
        {
            assignments += 1;
        }
    }
    total >= 5 && assignments * 4 >= total * 3
}

fn redact_line(line: &str) -> String {
    let mut value = redact_bearer_tokens(line);
    let lowercase = value.to_ascii_lowercase();
    const SECRET_KEYS: [&str; 8] = [
        "api_key",
        "api-key",
        "apikey",
        "access_token",
        "auth_token",
        "password",
        "secret",
        "token",
    ];

    for key in SECRET_KEYS {
        let Some(key_at) = lowercase.find(key) else {
            continue;
        };
        let suffix = &value[key_at + key.len()..];
        let Some(separator_at) = suffix.find(['=', ':']) else {
            continue;
        };
        let cutoff = key_at + key.len() + separator_at + 1;
        value.truncate(cutoff);
        value.push_str(" [REDACTED]");
        break;
    }
    value
}

fn redact_bearer_tokens(value: &str) -> String {
    let lowercase = value.to_ascii_lowercase();
    let Some(start) = lowercase.find("bearer ") else {
        return value.to_string();
    };
    let token_start = start + "bearer ".len();
    let token_end = value[token_start..]
        .find(char::is_whitespace)
        .map(|offset| token_start + offset)
        .unwrap_or(value.len());
    format!("{}[REDACTED]{}", &value[..token_start], &value[token_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_caps_labels_by_unicode_scalar_value() {
        let label = format!("  one\n two   {}", "é".repeat(200));
        let activity = AgentActivity::new(
            "test",
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            label,
            None,
        );

        assert_eq!(activity.label.chars().count(), MAX_LABEL_CHARS);
        assert!(activity.label.starts_with("one two "));
        assert!(!activity.label.contains('\n'));
    }

    #[test]
    fn redacts_credentials_and_caps_details_on_a_character_boundary() {
        let detail = format!(
            "Authorization: Bearer top-secret\nAPI_KEY=also-secret\n{}",
            "é".repeat(MAX_DETAIL_BYTES)
        );
        let activity = AgentActivity::new(
            "test",
            AgentActivityKind::Tool,
            AgentActivityState::Completed,
            "Tool",
            Some(&detail),
        );
        let detail = activity.detail.expect("detail");

        assert!(detail.len() <= MAX_DETAIL_BYTES);
        assert!(detail.contains("Bearer [REDACTED]"));
        assert!(detail.contains("API_KEY= [REDACTED]"));
        assert!(!detail.contains("top-secret"));
        assert!(!detail.contains("also-secret"));
    }

    #[test]
    fn omits_empty_details() {
        let activity = AgentActivity::new(
            "test",
            AgentActivityKind::Assistant,
            AgentActivityState::Completed,
            "Response",
            Some("   "),
        );
        assert_eq!(activity.detail, None);
    }

    #[test]
    fn replaces_environment_dumps_with_an_explicit_safe_summary() {
        let detail = [
            "PATH=/usr/bin",
            "HOME=/Users/person",
            "SHELL=/bin/zsh",
            "API_KEY=private",
            "DATABASE_URL=postgres://private",
        ]
        .join("\n");
        let activity = AgentActivity::new(
            "test",
            AgentActivityKind::Command,
            AgentActivityState::Completed,
            "Command",
            Some(&detail),
        );
        assert_eq!(
            activity.detail.as_deref(),
            Some("Environment output hidden")
        );
    }
}
