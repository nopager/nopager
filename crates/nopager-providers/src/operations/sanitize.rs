use serde_json::Value;

const MAX_CONTEXT_DEPTH: usize = 8;
const MAX_CONTEXT_KEYS: usize = 64;
const MAX_CONTEXT_ITEMS: usize = 32;
const MAX_CONTEXT_STRING_CHARS: usize = 12_000;
const REDACTED: &str = "[REDACTED_BY_NOPAGER]";
const REDACTED_PRIVATE_KEY: &str = "[REDACTED_PRIVATE_KEY_BY_NOPAGER]";
const TRUNCATED: &str = "[TRUNCATED_BY_NOPAGER]";

pub(crate) fn sanitize_and_bound(value: &mut Value, depth: usize) {
    if depth >= MAX_CONTEXT_DEPTH {
        *value = Value::String(TRUNCATED.to_owned());
        return;
    }

    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            let mut truncated = false;
            for (index, key) in keys.into_iter().enumerate() {
                if index >= MAX_CONTEXT_KEYS {
                    map.remove(&key);
                    truncated = true;
                    continue;
                }
                if sensitive_key(&key) {
                    map.insert(key, Value::String(REDACTED.to_owned()));
                } else if let Some(child) = map.get_mut(&key) {
                    sanitize_and_bound(child, depth + 1);
                }
            }
            if truncated {
                map.insert("_nopagerContextTruncated".into(), Value::Bool(true));
            }
        }
        Value::Array(values) => {
            let truncated = values.len() > MAX_CONTEXT_ITEMS;
            values.truncate(MAX_CONTEXT_ITEMS);
            for child in values.iter_mut() {
                sanitize_and_bound(child, depth + 1);
            }
            if truncated && values.len() < MAX_CONTEXT_ITEMS {
                values.push(Value::String(TRUNCATED.to_owned()));
            }
        }
        Value::String(text) => *text = sanitize_text(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    normalized.contains("password")
        || normalized.contains("secret")
        || normalized.contains("apikey")
        || normalized.contains("privatekey")
        || normalized == "authorization"
        || normalized.ends_with("token")
        || matches!(
            normalized.as_str(),
            "cookie" | "setcookie" | "databaseurl" | "connectionstring" | "dsn"
        )
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn sanitize_text(value: &str) -> String {
    let without_private_keys = redact_private_key_blocks(value);
    let mut sanitized = without_private_keys
        .lines()
        .map(redact_sensitive_assignment)
        .collect::<Vec<_>>()
        .join("\n");
    if without_private_keys.ends_with('\n') {
        sanitized.push('\n');
    }

    sanitized = redact_url_credentials(&sanitized);
    for (prefix, minimum_length) in [
        ("github_pat_", 20),
        ("ghp_", 16),
        ("glpat-", 16),
        ("xoxb-", 16),
        ("xoxp-", 16),
        ("sk-proj-", 16),
        ("sk-", 16),
        ("AIza", 20),
        ("AKIA", 20),
    ] {
        sanitized = redact_prefixed_token(&sanitized, prefix, minimum_length);
    }
    truncate_chars(&sanitized, MAX_CONTEXT_STRING_CHARS)
}

fn redact_private_key_blocks(value: &str) -> String {
    let mut output = Vec::new();
    let mut inside_private_key = false;
    for line in value.lines() {
        if !inside_private_key && line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----")
        {
            output.push(REDACTED_PRIVATE_KEY.to_owned());
            inside_private_key = true;
            continue;
        }
        if inside_private_key {
            if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                inside_private_key = false;
            }
            continue;
        }
        output.push(line.to_owned());
    }

    let mut rendered = output.join("\n");
    if value.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn redact_sensitive_assignment(line: &str) -> String {
    let separator = line
        .find('=')
        .map(|index| (index, '='))
        .or_else(|| line.find(':').map(|index| (index, ':')));
    let Some((index, separator)) = separator else {
        return line.to_owned();
    };

    let key = line[..index]
        .trim()
        .trim_matches(|character: char| matches!(character, '"' | '\'' | '`' | ' '));
    if !sensitive_key(key) {
        return line.to_owned();
    }

    let rhs = line[index + separator.len_utf8()..].trim();
    let env_like = key.chars().any(|character| character.is_ascii_uppercase())
        && key.chars().all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-' | '.')
        });
    let normalized = normalize_key(key);
    let header_like = matches!(
        normalized.as_str(),
        "authorization" | "cookie" | "setcookie"
    ) || normalized.contains("apikey");

    if env_like || header_like || looks_like_secret(rhs) {
        format!("{}{} {REDACTED}", line[..index].trim_end(), separator)
    } else {
        line.to_owned()
    }
}

fn looks_like_secret(value: &str) -> bool {
    let value = value
        .trim_matches(|character: char| matches!(character, '"' | '\'' | '`' | ',' | ';' | ' '));
    value.len() >= 16
        && !value.chars().any(char::is_whitespace)
        && value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && value.chars().any(|character| character.is_ascii_digit())
}

fn redact_url_credentials(value: &str) -> String {
    let mut output = value.to_owned();
    for scheme in [
        "https://",
        "http://",
        "postgres://",
        "postgresql://",
        "mysql://",
    ] {
        let mut cursor = 0;
        while let Some(relative) = output[cursor..].find(scheme) {
            let start = cursor + relative + scheme.len();
            let end = output[start..]
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '/' | '?' | '#')
                })
                .map_or(output.len(), |relative_end| start + relative_end);
            let authority = &output[start..end];
            let Some(at) = authority.rfind('@') else {
                cursor = end;
                continue;
            };
            let credential_end = start + at;
            output.replace_range(start..credential_end, REDACTED);
            cursor = start + REDACTED.len() + 1;
        }
    }
    output
}

fn redact_prefixed_token(value: &str, prefix: &str, minimum_length: usize) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative) = value[cursor..].find(prefix) {
        let start = cursor + relative;
        output.push_str(&value[cursor..start]);
        let mut end = start + prefix.len();
        while end < value.len() {
            let byte = value.as_bytes()[end];
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') {
                end += 1;
            } else {
                break;
            }
        }
        if end - start >= minimum_length {
            output.push_str(REDACTED);
        } else {
            output.push_str(&value[start..end]);
        }
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut bounded = value.chars().take(max_chars).collect::<String>();
    bounded.push('\n');
    bounded.push_str(TRUNCATED);
    bounded
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn model_boundary_redacts_secrets_and_preserves_safe_evidence() {
        let mut value = json!({
            "authorization": "Bearer never-send-this",
            "log": "DATABASE_URL=postgresql://app:supersecret123@db.example.com/prod\nrequest failed",
            "nested": { "apiKey": "sk-proj-never-send-this-either" }
        });
        sanitize_and_bound(&mut value, 0);
        let rendered = serde_json::to_string(&value).unwrap();
        assert!(!rendered.contains("never-send-this"));
        assert!(!rendered.contains("supersecret123"));
        assert!(rendered.contains(REDACTED));
        assert!(rendered.contains("request failed"));
    }

    #[test]
    fn model_boundary_bounds_large_arrays() {
        let mut value = Value::Array((0..100).map(Value::from).collect());
        sanitize_and_bound(&mut value, 0);
        assert!(value.as_array().unwrap().len() <= MAX_CONTEXT_ITEMS);
    }

    #[test]
    fn private_key_material_is_removed() {
        let value = "before\n-----BEGIN PRIVATE KEY-----\nabc123\n-----END PRIVATE KEY-----\nafter";
        let sanitized = sanitize_text(value);
        assert!(sanitized.contains("before"));
        assert!(sanitized.contains("after"));
        assert!(sanitized.contains(REDACTED_PRIVATE_KEY));
        assert!(!sanitized.contains("abc123"));
    }
}
