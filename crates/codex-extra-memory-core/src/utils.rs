use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

#[must_use]
pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

#[must_use]
pub fn now_iso() -> String {
    now_utc().to_rfc3339()
}

#[must_use]
pub fn normalize_for_hash(value: &str) -> String {
    value
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[must_use]
pub fn normalize_content_for_storage(value: &str) -> String {
    value
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[must_use]
pub fn sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn split_first_token(value: &str) -> (&str, &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return ("", "");
    }

    if let Some(index) = trimmed.find(char::is_whitespace) {
        let token = &trimmed[..index];
        let rest = trimmed[index + 1..].trim();
        (token, rest)
    } else {
        (trimmed, "")
    }
}

#[must_use]
pub fn parse_positive_int(input: Option<i64>, fallback: usize) -> usize {
    let Some(raw) = input else {
        return fallback;
    };
    if raw <= 0 {
        return fallback;
    }
    raw as usize
}

#[must_use]
pub fn parse_boolean(input: Option<Value>, fallback: bool) -> bool {
    let Some(value) = input else {
        return fallback;
    };

    match value {
        Value::Bool(v) => v,
        Value::String(v) => match v.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => fallback,
        },
        Value::Number(v) => {
            if v.as_i64() == Some(1) {
                true
            } else if v.as_i64() == Some(0) {
                false
            } else {
                fallback
            }
        }
        _ => fallback,
    }
}

#[must_use]
pub fn format_memory_scope(scope: &str, project_scope: &str) -> String {
    if scope == "global" {
        "global".to_string()
    } else if scope == project_scope || scope.starts_with("project:") {
        "project".to_string()
    } else {
        scope.to_string()
    }
}

fn secret_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"\bsk-[A-Za-z0-9]{16,}\b",
            r"\bghp_[A-Za-z0-9]{20,}\b",
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bAIza[0-9A-Za-z_-]{20,}\b",
            r"\b(xox[pbar]-[A-Za-z0-9-]{10,})\b",
            r"\bkey_live_[A-Za-z0-9]{16,}\b",
            r"-----BEGIN (RSA|EC|OPENSSH|PGP) PRIVATE KEY-----",
            r"\bBearer\s+[A-Za-z0-9._-]{20,}\b",
            r#"\b(?:api[_-]?key|token|secret|password)\b\s*[:=]\s*['"]?[A-Za-z0-9._\-/=+]{12,}"#,
            r"\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis):\/\/[^\s]+",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid secret regex"))
        .collect()
    })
}

fn has_high_entropy_token(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        if token.len() < 32 {
            return false;
        }
        let alpha = token.chars().any(char::is_alphabetic);
        let digit = token.chars().any(char::is_numeric);
        let special = token
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '_');
        alpha && digit && special
    })
}

#[must_use]
pub fn is_probably_secret(value: &str) -> bool {
    if secret_patterns()
        .iter()
        .any(|pattern| pattern.is_match(value))
    {
        return true;
    }
    has_high_entropy_token(value)
}

pub fn sanitize_memory_text(value: &str) -> Result<String, String> {
    let text = normalize_content_for_storage(value);
    if text.is_empty() {
        return Err("Memory text cannot be empty.".to_string());
    }
    if text.chars().count() > 1_200 {
        return Err("Memory text is too long (max 1200 characters).".to_string());
    }
    if is_probably_secret(&text) {
        return Err("Memory looks like a secret/token. Refusing to store it.".to_string());
    }
    Ok(text)
}

#[must_use]
pub fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[must_use]
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect::<String>()
}
