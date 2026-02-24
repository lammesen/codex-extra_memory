use crate::types::{AutoCaptureCandidate, AutoCaptureConfig, MemoryCategory};
use crate::utils::{is_probably_secret, normalize_for_hash, sha256};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

fn explicit_remember_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:please\s+)?remember(?:\s+that)?\s+(.+)").expect("valid regex")
    })
}

fn explicit_preference_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:my\s+preference\s+is|i\s+prefer)\s+(.+)").expect("valid regex")
    })
}

fn assistant_marker_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?i)^(?:memory|remember)\s*:\s*(.+)$").expect("valid regex"))
}

fn extract_text_from_message_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    let Some(blocks) = content.as_array() else {
        return String::new();
    };

    let mut chunks = Vec::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = block.get("text").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            chunks.push(text.to_string());
        }
    }

    chunks.join("\n")
}

fn cleanup_text(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '“' | '”' | '‘' | '’'))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches([';', ':', ',', '.', '!', '?'])
        .trim()
        .to_string()
}

fn infer_category(text: &str) -> MemoryCategory {
    let lower = text.to_lowercase();
    if ["prefer", "preference", "like", "dislike"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return MemoryCategory::Preference;
    }

    if [
        "always", "usually", "workflow", "run", "command", "format", "style",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return MemoryCategory::Workflow;
    }

    if [
        "never", "must", "mustn't", "do not", "don't", "avoid", "required", "forbid",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return MemoryCategory::Constraint;
    }

    MemoryCategory::Other
}

fn extract_user_explicit(text: &str) -> Vec<(String, MemoryCategory, String)> {
    let mut results = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(found) = explicit_remember_regex()
            .captures(line)
            .and_then(|caps| caps.get(1))
        {
            let body = found.as_str().to_string();
            results.push((
                body.clone(),
                infer_category(&body),
                "explicit remember statement".to_string(),
            ));
            continue;
        }

        if let Some(found) = explicit_preference_regex()
            .captures(line)
            .and_then(|caps| caps.get(1))
        {
            results.push((
                found.as_str().to_string(),
                MemoryCategory::Preference,
                "explicit preference statement".to_string(),
            ));
        }
    }
    results
}

fn extract_assistant_marked(text: &str) -> Vec<(String, MemoryCategory, String)> {
    let mut results = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(found) = assistant_marker_regex()
            .captures(line)
            .and_then(|caps| caps.get(1))
        {
            let body = found.as_str().to_string();
            results.push((
                body.clone(),
                infer_category(&body),
                "assistant memory marker".to_string(),
            ));
        }
    }
    results
}

pub fn extract_auto_capture_candidates(
    messages: &Value,
    config: &AutoCaptureConfig,
    processed_hashes: &HashSet<String>,
) -> Vec<AutoCaptureCandidate> {
    let Some(messages) = messages.as_array() else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    let mut seen_turn = HashSet::new();

    for message in messages {
        let role = message.get("role").and_then(Value::as_str);
        let Some(role) = role else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }

        let text = message
            .get("content")
            .map(extract_text_from_message_content)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }

        let extracted = if role == "user" {
            extract_user_explicit(&text)
        } else {
            extract_assistant_marked(&text)
        };

        for (raw_text, category, reason) in extracted {
            let cleaned = cleanup_text(&raw_text);
            if cleaned.is_empty() {
                continue;
            }
            let char_count = cleaned.chars().count();
            if char_count < config.min_chars || char_count > config.max_chars {
                continue;
            }
            if is_probably_secret(&cleaned) {
                continue;
            }

            let hash = sha256(&format!("{role}:{}", normalize_for_hash(&cleaned)));
            if processed_hashes.contains(&hash) || seen_turn.contains(&hash) {
                continue;
            }

            candidates.push(AutoCaptureCandidate {
                hash: hash.clone(),
                text: cleaned,
                category,
                reason,
            });
            seen_turn.insert(hash);

            if candidates.len() >= config.max_per_turn {
                return candidates;
            }
        }
    }

    candidates
}

#[must_use]
pub fn get_agent_end_messages(event: &Value) -> Value {
    event
        .get("messages")
        .cloned()
        .unwrap_or(Value::Array(vec![]))
}

#[cfg(test)]
mod tests {
    use super::{extract_assistant_marked, extract_user_explicit};

    #[test]
    fn user_explicit_patterns_extract_expected_entries() {
        let entries = extract_user_explicit(
            "please remember that always run tests\nI prefer rust for tooling",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "always run tests");
        assert_eq!(entries[1].0, "rust for tooling");
    }

    #[test]
    fn assistant_marker_pattern_extracts_expected_entry() {
        let entries = extract_assistant_marked("Memory: keep answers concise");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "keep answers concise");
    }
}
