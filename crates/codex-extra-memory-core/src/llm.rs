use crate::types::MemoryRow;
use crate::utils::truncate_chars;
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlmSummaryRequest {
    pub model: String,
    pub timeout_ms: u64,
    pub max_output_chars: usize,
}

fn extract_summary_text_from_responses(response: &Value) -> Option<String> {
    if let Some(text) = response.get("output_text").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(parts) = response.get("output_text").and_then(Value::as_array) {
        let joined = parts
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.is_empty() {
            return Some(joined);
        }
    }

    let mut collected = Vec::new();
    let output = response.get("output").and_then(Value::as_array)?;

    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            let part_type = part.get("type").and_then(Value::as_str);
            if part_type != Some("output_text") && part_type != Some("text") {
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    collected.push(trimmed.to_string());
                }
            }
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}

pub fn summarize_memories_with_llm(
    rows: &[MemoryRow],
    request: &LlmSummaryRequest,
) -> Result<Option<String>> {
    let api_key = std::env::var("OPENAI_API_KEY").ok();
    let Some(api_key) = api_key.filter(|x| !x.trim().is_empty()) else {
        return Ok(None);
    };

    if rows.is_empty() {
        return Ok(Some(String::new()));
    }

    let mut memory_lines = Vec::new();
    for row in rows.iter().take(200) {
        memory_lines.push(format!(
            "- [{}{} / {}] {}",
            if row.scope == "global" {
                "global"
            } else {
                "project"
            },
            if row.pinned { " pinned" } else { "" },
            row.category,
            row.content
        ));
    }

    let system = "You compress memory bullet points for coding assistants. Return only bullet lines prefixed with '- '. Preserve constraints and preferences. Do not include secrets.";
    let user = format!(
        "Compress these memory facts into concise bullet points. Max output characters: {}.\n\n{}",
        request.max_output_chars,
        memory_lines.join("\n")
    );

    let payload = json!({
        "model": request.model,
        "input": [
            {
                "role": "system",
                "content": [{"type": "input_text", "text": system}],
            },
            {
                "role": "user",
                "content": [{"type": "input_text", "text": user}],
            }
        ],
    });

    let client = Client::builder()
        .timeout(Duration::from_millis(request.timeout_ms))
        .build()
        .context("build llm client")?;

    let response = client
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .context("send llm summary request")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("llm request failed: {status} {body}");
    }

    let json: Value = response.json().context("parse llm response json")?;
    let text = extract_summary_text_from_responses(&json);

    Ok(text.map(|x| truncate_chars(&x, request.max_output_chars)))
}

#[cfg(test)]
mod tests {
    use super::extract_summary_text_from_responses;
    use serde_json::json;

    #[test]
    fn extracts_summary_from_output_text_string() {
        let response = json!({"output_text": "  - item one\n- item two  "});
        let summary = extract_summary_text_from_responses(&response).expect("summary");
        assert_eq!(summary, "- item one\n- item two");
    }

    #[test]
    fn extracts_summary_from_output_message_parts() {
        let response = json!({
            "output": [
                {"type": "reasoning", "summary": []},
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "- keep tests"},
                        {"type": "output_text", "text": "- avoid secrets"}
                    ]
                }
            ]
        });
        let summary = extract_summary_text_from_responses(&response).expect("summary");
        assert_eq!(summary, "- keep tests\n- avoid secrets");
    }
}
