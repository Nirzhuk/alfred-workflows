//! Centralized secret detection, redaction, and CLI-permission denial.
//!
//! Every native surface (context preparation, request validation, event
//! normalization, and the Alfred tool boundary) shares these helpers so a
//! provider plan cannot pick up a weaker copy of the same policy.

/// Case-insensitive markers used by the request-admission gate. Keep this
/// deliberately narrow: admission must not reject ordinary source code or
/// prose merely because it names a credential-shaped field.
const ADMISSION_SECRET_VALUE_MARKERS: [&str; 6] = [
    "bearer ",
    "basic ",
    "cookie: ",
    "set-cookie: ",
    "authorization: ",
    "credential_path=",
];

/// Field-like markers considered by diagnostic redaction only when their
/// trailing value is actually credential-shaped.
const DIAGNOSTIC_CREDENTIAL_VALUE_MARKERS: [&str; 10] = [
    "password=",
    "password: ",
    "secret=",
    "secret: ",
    "token=",
    "token: ",
    "api_key=",
    "api_key: ",
    "credential=",
    "credential: ",
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
    if ADMISSION_SECRET_VALUE_MARKERS
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

/// True when a diagnostic contains material that [`redact_text`] will remove.
/// This is intentionally separate from [`contains_secret_marker`]: output
/// diagnostics need broader credential-value detection than request admission.
pub(crate) fn contains_diagnostic_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    has_private_key_block(&lower) || !redaction_spans(value, &lower).is_empty()
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

    let mut spans = redaction_spans(value, &lower);
    if spans.is_empty() {
        return value.to_string();
    }

    spans.sort_by_key(|(start, _)| *start);
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some((_, previous_end)) = merged.last_mut() {
            if start <= *previous_end
                || value[*previous_end..start].chars().all(char::is_whitespace)
            {
                *previous_end = (*previous_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    for (start, end) in merged {
        output.push_str(&value[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn redaction_spans(value: &str, lower: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for marker in ADMISSION_SECRET_VALUE_MARKERS {
        collect_marker_spans(value, lower, marker, &mut spans);
    }
    for marker in DIAGNOSTIC_CREDENTIAL_VALUE_MARKERS {
        collect_credential_marker_spans(value, lower, marker, &mut spans);
    }
    collect_token_spans(value, lower, &mut spans);
    spans.extend(credential_shaped_spans(value, lower));
    spans
}

/// Finds standalone JWTs and long high-entropy token-shaped values. This is
/// intentionally conservative: ordinary hashes, UUIDs, paths, and prose do
/// not meet the mixed-character requirement.
fn credential_shaped_spans<'a>(
    value: &'a str,
    lower: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    token_ranges(value).filter(move |(start, end)| {
        let token = &value[*start..*end];
        let lowered = &lower[*start..*end];
        looks_like_jwt(token, lowered) || looks_like_high_entropy_secret(token, false)
    })
}

fn token_ranges(value: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, character) in value.char_indices() {
        if is_token_boundary(character) {
            if let Some(begin) = start.take() {
                ranges.push((begin, index));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        ranges.push((begin, value.len()));
    }
    ranges.into_iter()
}

fn looks_like_jwt(token: &str, lower: &str) -> bool {
    let mut segments = token.split('.');
    let Some(header) = segments.next() else {
        return false;
    };
    let Some(payload) = segments.next() else {
        return false;
    };
    let Some(signature) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && lower.starts_with("eyj")
        && [header, payload, signature].iter().all(|segment| {
            segment.len() >= 8
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn looks_like_high_entropy_secret(token: &str, marker_context: bool) -> bool {
    if token.len() < 32
        || token.len() > 512
        || token.bytes().any(|byte| matches!(byte, b'/' | b'\\'))
        || looks_like_uuid(token)
        || looks_like_integrity_or_hash(token)
        || looks_like_code_value(token)
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'+' | b'='))
    {
        return false;
    }
    let has_lower = token.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_upper = token.bytes().any(|byte| byte.is_ascii_uppercase());
    let has_digit = token.bytes().any(|byte| byte.is_ascii_digit());
    let distinct = token.bytes().fold([false; 128], |mut seen, byte| {
        if byte.is_ascii() {
            seen[usize::from(byte)] = true;
        }
        seen
    });
    has_lower
        && has_upper
        && has_digit
        && (marker_context
            || token
                .bytes()
                .any(|byte| matches!(byte, b'-' | b'_' | b'+' | b'=')))
        && distinct.into_iter().filter(|present| *present).count() >= 12
}

fn looks_like_uuid(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .iter()
            .all(|index| bytes.get(*index) == Some(&b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn looks_like_integrity_or_hash(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    let npm_integrity = ["sha1-", "sha256-", "sha384-", "sha512-"]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    npm_integrity || (token.len() >= 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn looks_like_code_value(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "any"
            | "bool"
            | "boolean"
            | "nil"
            | "none"
            | "null"
            | "number"
            | "object"
            | "str"
            | "string"
            | "undefined"
            | "unknown"
            | "void"
    )
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

fn collect_credential_marker_spans(
    value: &str,
    lower: &str,
    marker: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find(marker) {
        let start = search + offset + marker.len();
        let end = value_end(value, start);
        if end > start {
            let token = &value[start..end];
            let lowered = &lower[start..end];
            if looks_like_jwt(token, lowered)
                || looks_like_high_entropy_secret(token, true)
                || SECRET_TOKEN_PREFIXES
                    .iter()
                    .any(|prefix| lowered.starts_with(prefix))
            {
                spans.push((start, end));
            }
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
    fn ordinary_source_paths_hashes_ids_and_prose_survive_byte_identical() {
        for value in [
            "token: string;",
            "export interface Auth { token: string; secret: string }",
            "refresh_token=None",
            "where is the refresh token: in auth.ts?",
            "fix the api_key: undefined bug in config.rs",
            "run the tool on /Users/nirzhuk/Library/Caches/alfred/v2/session",
            r#""integrity": "sha512-AbCdEf0123456789hIjKlMnOpQrStUvWxYz+/abcdEFGH""#,
            "open doc 1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            "request type/source/path/integrity diagnostics",
            "request id 550e8400-e29b-41d4-a716-446655440000",
        ] {
            assert!(
                !contains_secret_marker(value),
                "admission rejected: {value}"
            );
            assert!(
                !contains_diagnostic_secret(value),
                "diagnostic rejected: {value}"
            );
            assert_eq!(redact_text(value), value, "diagnostic changed: {value}");
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
    fn bare_jwts_and_high_entropy_values_are_redacted() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let opaque = "Ab3dEf5hIj7lMn9pQr2tUv4xYz6_Bc8D";
        for secret in [jwt, opaque] {
            let value = format!("refresh failed: {secret}");
            assert!(!contains_secret_marker(&value));
            assert!(contains_diagnostic_secret(&value));
            assert!(!redact_text(&value).contains(secret));
        }
        assert_eq!(
            redact_text("digest 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "digest 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn credential_markers_require_secret_shaped_values_for_diagnostics() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let marked = format!("token: {jwt}");
        assert!(!contains_secret_marker(&marked));
        assert!(contains_diagnostic_secret(&marked));
        assert_eq!(redact_text(&marked), "token: [REDACTED]");

        let bearer = format!("Authorization: Bearer {jwt}");
        assert!(contains_secret_marker(&bearer));
        assert!(contains_diagnostic_secret(&bearer));
        assert_eq!(redact_text(&bearer), "Authorization: [REDACTED]");
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
