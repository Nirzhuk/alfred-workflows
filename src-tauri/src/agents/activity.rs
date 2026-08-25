use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_LABEL_CHARS: usize = 96;
const ACTIVITY_ID_DIGEST_BYTES: usize = 12;

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
    /// Builds an activity from an adapter event.
    ///
    /// Adapter text is untrusted. The label is reduced to a closed set of
    /// activity categories and detail is deliberately discarded. The legacy
    /// arguments remain so adapters can migrate independently without exposing
    /// provider payloads in run events.
    pub fn new(
        id: impl Into<String>,
        kind: AgentActivityKind,
        state: AgentActivityState,
        label: impl AsRef<str>,
        _detail: Option<&str>,
    ) -> Self {
        let id = opaque_activity_id(&id.into());
        let label = safe_label(&kind, &state, label.as_ref());
        Self {
            id,
            kind,
            state,
            label,
            detail: None,
        }
    }

    /// Rebuilds adapter activity at the run-event boundary.
    ///
    /// This also protects against a deserialized or manually constructed value
    /// that did not pass through `new`.
    pub fn safe_for_emission(&self, label_suffix: &str) -> Self {
        let label = safe_label(&self.kind, &self.state, &self.label);
        let label = match safe_source_suffix(label_suffix) {
            Some(suffix) => format!("{label} · {suffix}"),
            None => label,
        };
        Self {
            id: opaque_activity_id(&self.id),
            kind: self.kind.clone(),
            state: self.state.clone(),
            label: bound_label(label),
            detail: None,
        }
    }
}

fn opaque_activity_id(value: &str) -> String {
    if is_opaque_activity_id(value) {
        return value.to_owned();
    }

    let digest = Sha256::digest(value.as_bytes());
    let suffix = digest
        .iter()
        .take(ACTIVITY_ID_DIGEST_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("agent_activity_{suffix}")
}

fn is_opaque_activity_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("agent_activity_") else {
        return false;
    };
    suffix.len() == ACTIVITY_ID_DIGEST_BYTES * 2
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn safe_label(
    kind: &AgentActivityKind,
    state: &AgentActivityState,
    candidate: &str,
) -> String {
    let label = match (kind, state, candidate) {
        (AgentActivityKind::Status, AgentActivityState::Started, "Thinking") => "Thinking",
        (AgentActivityKind::Status, AgentActivityState::Started, "Working") => "Working",
        (AgentActivityKind::Status, AgentActivityState::Completed, "Work completed") => {
            "Work completed"
        }
        (AgentActivityKind::Status, AgentActivityState::Completed, "Codex session started") => {
            "Codex session started"
        }
        (AgentActivityKind::Status, AgentActivityState::Completed, "Gemini session started") => {
            "Gemini session started"
        }
        (AgentActivityKind::Status, AgentActivityState::Completed, "Claude Code session started") => {
            "Claude Code session started"
        }
        (AgentActivityKind::Status, AgentActivityState::Completed, "Cursor session started") => {
            "Cursor session started"
        }
        (AgentActivityKind::Status, AgentActivityState::Completed, "OpenCode session started") => {
            "OpenCode session started"
        }
        (AgentActivityKind::Status, AgentActivityState::Started, _) => "Working",
        (AgentActivityKind::Status, AgentActivityState::Completed, _) => "Status updated",
        (AgentActivityKind::Assistant, _, _) => "Agent response",
        (AgentActivityKind::Tool, AgentActivityState::Started, "Web search") => "Web search",
        (AgentActivityKind::Tool, AgentActivityState::Started, _) => "Using tool",
        (AgentActivityKind::Tool, AgentActivityState::Completed, _) => "Tool completed",
        (AgentActivityKind::Command, AgentActivityState::Started, _) => "Running command",
        (AgentActivityKind::Command, AgentActivityState::Completed, _) => "Command completed",
        (AgentActivityKind::File, AgentActivityState::Started, _) => "Changing file",
        (AgentActivityKind::File, AgentActivityState::Completed, _) => "File changed",
        (AgentActivityKind::Error, _, _) => "Agent error",
    };
    bound_label(label)
}

fn safe_source_suffix(value: &str) -> Option<&'static str> {
    match value {
        "CLI" => Some("CLI"),
        "Alfred" => Some("Alfred"),
        _ => None,
    }
}

fn bound_label(value: impl AsRef<str>) -> String {
    value.as_ref().chars().take(MAX_LABEL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitted_with(
        kind: AgentActivityKind,
        state: AgentActivityState,
        label: &str,
        detail: Option<&str>,
    ) -> AgentActivity {
        AgentActivity::new("provider-id", kind, state, label, detail).safe_for_emission("CLI")
    }

    #[test]
    fn drops_all_unstructured_provider_detail() {
        for detail in [
            "arbitrary provider output",
            "credential=plain-sensitive-value",
            "Authorization: Bearer bearer-sensitive-value",
            "Cookie: session=cookie-sensitive-value",
            "API_KEY=key-sensitive-value",
            "reasoning: private chain of thought",
        ] {
            let activity = emitted_with(
                AgentActivityKind::Assistant,
                AgentActivityState::Completed,
                "Agent response",
                Some(detail),
            );
            let serialized = serde_json::to_string(&activity).unwrap();

            assert_eq!(activity.detail, None);
            assert!(!serialized.contains(detail));
            assert!(!serialized.contains("plain-sensitive-value"));
            assert!(!serialized.contains("bearer-sensitive-value"));
            assert!(!serialized.contains("cookie-sensitive-value"));
            assert!(!serialized.contains("key-sensitive-value"));
            assert!(!serialized.contains("private chain of thought"));
        }
    }

    #[test]
    fn drops_oversized_detail_instead_of_truncating_and_emitting_it() {
        let detail = format!("credential=plain-sensitive-value{}", "x".repeat(100_000));
        let activity = emitted_with(
            AgentActivityKind::Command,
            AgentActivityState::Completed,
            "Command",
            Some(&detail),
        );
        let serialized = serde_json::to_string(&activity).unwrap();

        assert_eq!(activity.detail, None);
        assert!(!serialized.contains("plain-sensitive-value"));
        assert!(serialized.len() < 512);
    }

    #[test]
    fn arbitrary_labels_reduce_to_bounded_activity_categories() {
        let unsafe_label = format!(
            "reasoning credential=plain-sensitive-value Bearer bearer-value Cookie=cookie-value {}",
            "x".repeat(100_000)
        );
        let cases = [
            (
                AgentActivityKind::Status,
                AgentActivityState::Started,
                "Working · CLI",
            ),
            (
                AgentActivityKind::Tool,
                AgentActivityState::Started,
                "Using tool · CLI",
            ),
            (
                AgentActivityKind::File,
                AgentActivityState::Completed,
                "File changed · CLI",
            ),
            (
                AgentActivityKind::Command,
                AgentActivityState::Completed,
                "Command completed · CLI",
            ),
            (
                AgentActivityKind::Error,
                AgentActivityState::Completed,
                "Agent error · CLI",
            ),
        ];

        for (kind, state, expected) in cases {
            let activity = emitted_with(kind, state, &unsafe_label, None);
            assert_eq!(activity.label, expected);
            assert!(activity.label.chars().count() <= MAX_LABEL_CHARS);
            assert!(!activity.label.contains("plain-sensitive-value"));
            assert!(!activity.label.contains("bearer-value"));
            assert!(!activity.label.contains("cookie-value"));
        }
    }

    #[test]
    fn keeps_known_safe_status_and_tool_labels_useful() {
        let cases = [
            (
                AgentActivityKind::Status,
                AgentActivityState::Started,
                "Thinking",
                "Thinking · Alfred",
            ),
            (
                AgentActivityKind::Status,
                AgentActivityState::Completed,
                "Work completed",
                "Work completed · Alfred",
            ),
            (
                AgentActivityKind::Tool,
                AgentActivityState::Started,
                "Web search",
                "Web search · Alfred",
            ),
        ];

        for (kind, state, label, expected) in cases {
            let activity =
                AgentActivity::new("id", kind, state, label, None).safe_for_emission("Alfred");
            assert_eq!(activity.label, expected);
            assert!(activity.label.chars().count() <= MAX_LABEL_CHARS);
        }
    }

    #[test]
    fn forged_activity_is_closed_again_at_emission() {
        let unsafe_activity = AgentActivity {
            id: "credential=plain-sensitive-value".into(),
            kind: AgentActivityKind::Tool,
            state: AgentActivityState::Completed,
            label: "Bearer bearer-sensitive-value".into(),
            detail: Some("reasoning and Cookie=cookie-sensitive-value".into()),
        };
        let safe = unsafe_activity.safe_for_emission("credential=suffix-sensitive-value");
        let serialized = serde_json::to_string(&safe).unwrap();

        assert_eq!(safe.label, "Tool completed");
        assert_eq!(safe.detail, None);
        assert!(safe.id.starts_with("agent_activity_"));
        assert_eq!(
            safe.id.len(),
            "agent_activity_".len() + ACTIVITY_ID_DIGEST_BYTES * 2
        );
        for secret in [
            "plain-sensitive-value",
            "bearer-sensitive-value",
            "cookie-sensitive-value",
            "suffix-sensitive-value",
            "reasoning",
        ] {
            assert!(!serialized.contains(secret));
        }
    }
}
