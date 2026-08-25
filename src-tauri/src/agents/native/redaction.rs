//! Centralized secret detection, redaction, and CLI-permission denial.
//!
//! Every native surface (context preparation, request validation, event
//! normalization, and the Alfred tool boundary) shares these helpers so a
//! provider plan cannot pick up a weaker copy of the same policy.

/// Case-insensitive markers whose trailing value is a secret.
const SECRET_VALUE_MARKERS: [&str; 6] = [
    "bearer ",
    "basic ",
    "cookie: ",
    "set-cookie: ",
    "authorization: ",
    "credential_path=",
];

/// Case-insensitive token prefixes that are secrets on their own.
const SECRET_TOKEN_PREFIXES: [&str; 4] = ["sk-", "ghp_", "github_pat_", "xox"];

/// CLI escape hatches the Alfred harness must never inherit.
const CLI_PERMISSION_FLAGS: [&str; 6] = [
    "--full-auto",
    "--allow-all",
    "--yolo",
    "bypasspermissions",
    "bypass_permissions",
    "dangerously-skip-permissions",
];

fn has_private_key_block(lower: &str) -> bool {
    lower.contains("-----begin ") && lower.contains("private key-----")
}

/// True when the text carries credential-shaped material. Case-insensitive so
/// lowercase HTTP/2 headers (`authorization: bearer ...`) are caught too.
pub fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if SECRET_VALUE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
        || has_private_key_block(&lower)
    {
        return true;
    }
    lower.split_whitespace().any(|word| {
        SECRET_TOKEN_PREFIXES
            .iter()
            .any(|prefix| word.starts_with(prefix))
    })
}

/// True when the text carries a CLI permission escape hatch.
pub fn contains_cli_permission_flag(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    CLI_PERMISSION_FLAGS.iter().any(|flag| lower.contains(flag))
}

/// True when a metadata or tool key names credential material.
pub fn is_secret_key(key: &str) -> bool {
    let canonical = canonical_key(key);
    matches!(
        canonical.as_str(),
        "authorization" | "cookie" | "setcookie" | "password" | "promptsecret"
    ) || canonical.contains("token")
        || canonical.contains("secret")
        || canonical.contains("credential")
        || canonical.contains("apikey")
        || canonical.contains("privatekey")
        || canonical.contains("cookie")
}

pub fn canonical_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Replaces secret-bearing spans with `[REDACTED]`.
///
/// Matching is case-insensitive; the surviving text keeps its original casing
/// because every span is spliced out of the source string by byte offset.
pub fn redact_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if has_private_key_block(&lower) {
        return "[REDACTED PRIVATE KEY]".into();
    }

    let mut spans = Vec::new();
    for marker in SECRET_VALUE_MARKERS {
        collect_marker_spans(value, &lower, marker, &mut spans);
    }
    collect_token_spans(value, &lower, &mut spans);
    if spans.is_empty() {
        return value.to_string();
    }

    spans.sort_by_key(|(start, _)| *start);
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    for (start, end) in spans {
        if start < cursor {
            continue;
        }
        output.push_str(&value[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

/// Records the value that follows each occurrence of `marker` (the marker text
/// itself is kept, only its value is replaced).
fn collect_marker_spans(value: &str, lower: &str, marker: &str, spans: &mut Vec<(usize, usize)>) {
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find(marker) {
        let start = search + offset + marker.len();
        let end = value_end(value, start);
        if end > start {
            spans.push((start, end));
        }
        search = end.max(start).max(search + offset + 1);
        if search >= value.len() {
            break;
        }
    }
}

/// Records whole words that begin with a known secret token prefix.
fn collect_token_spans(value: &str, lower: &str, spans: &mut Vec<(usize, usize)>) {
    for prefix in SECRET_TOKEN_PREFIXES {
        let mut search = 0usize;
        while let Some(offset) = lower[search..].find(prefix) {
            let start = search + offset;
            // A token starts a word: after whitespace or a delimiter, but not
            // mid-identifier, so `task-sk-not-a-token` is left alone while
            // `"sk-live-1"` inside JSON is still caught.
            let at_word_start = start == 0
                || value[..start]
                    .chars()
                    .next_back()
                    .is_some_and(is_token_boundary);
            let end = value_end(value, start);
            if at_word_start && end > start {
                spans.push((start, end));
            }
            search = start + prefix.len();
            if search >= value.len() {
                break;
            }
        }
    }
}

fn is_token_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '{' | '}' | '[' | ']' | '=' | '<' | '>'
        )
}

/// A secret value runs until whitespace or a common delimiter.
fn value_end(value: &str, start: usize) -> usize {
    if start >= value.len() {
        return value.len();
    }
    value[start..]
        .find(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | '"' | '\'' | ')' | '}')
        })
        .map(|offset| start + offset)
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_and_redaction_are_case_insensitive() {
        for value in [
            "authorization: bearer eyJsecret",
            "Authorization: Bearer eyJsecret",
            "AUTHORIZATION: BEARER eyJsecret",
            "basic dXNlcjpwYXNz",
            "cookie: session=abc",
            "set-cookie: session=abc",
            "-----begin rsa private key-----",
        ] {
            assert!(contains_secret_marker(value), "not detected: {value}");
            let redacted = redact_text(value);
            assert!(
                redacted.contains("[REDACTED]") || redacted.contains("[REDACTED PRIVATE KEY]"),
                "not redacted: {value} -> {redacted}"
            );
            assert!(!redacted.contains("eyJsecret"));
            assert!(!redacted.contains("dXNlcjpwYXNz"));
        }
    }

    #[test]
    fn token_prefixes_are_redacted_but_ordinary_text_survives() {
        let redacted = redact_text("use sk-live-123 and ghp_abc plus xoxb-9 now");
        assert!(!redacted.contains("sk-live-123"));
        assert!(!redacted.contains("ghp_abc"));
        assert!(!redacted.contains("xoxb-9"));
        assert!(redacted.starts_with("use "));
        assert!(redacted.ends_with(" now"));
        assert_eq!(redact_text("nothing to hide"), "nothing to hide");
        // A prefix inside a word is not a token boundary.
        assert_eq!(redact_text("task-sk-not-a-token"), "task-sk-not-a-token");
        // A quoted JSON value still is.
        assert!(!redact_text("{\"key\":\"sk-live-1\"}").contains("sk-live-1"));
    }

    #[test]
    fn cli_permission_flags_are_detected_case_insensitively() {
        for flag in [
            "--full-auto",
            "--ALLOW-ALL",
            "bypassPermissions",
            "bypass_permissions",
            "--dangerously-skip-permissions",
            "--yolo",
        ] {
            assert!(contains_cli_permission_flag(flag), "missed {flag}");
        }
        assert!(!contains_cli_permission_flag("run the tests"));
    }

    #[test]
    fn secret_keys_cover_every_credential_shape() {
        for key in [
            "Authorization",
            "set-cookie",
            "accessToken",
            "refresh_token",
            "apiKey",
            "private_key",
            "credentialPath",
            "promptSecret",
        ] {
            assert!(is_secret_key(key), "missed {key}");
        }
        assert!(!is_secret_key("toolName"));
    }
}
