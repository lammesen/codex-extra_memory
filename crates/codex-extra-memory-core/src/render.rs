use crate::config::MemoryConfig;
use crate::types::{MemoryRow, MemoryStats, ScopeInfo};
use crate::utils::{format_memory_scope, now_iso};

#[must_use]
pub fn render_rows(rows: &[MemoryRow], scope_info: &ScopeInfo) -> String {
    if rows.is_empty() {
        return "No active memories.".to_string();
    }

    rows.iter()
        .map(|row| {
            let scope = format_memory_scope(&row.scope, &scope_info.scope);
            let pin = if row.pinned { " [pinned]" } else { "" };
            format!(
                "- {} ({}/{}){}\n  {}",
                row.id, scope, row.category, pin, row.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub fn build_injection_block(
    rows: &[MemoryRow],
    scope_info: &ScopeInfo,
    max_items: usize,
    max_chars: usize,
) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let header_lines = vec![
        "## Extra Memory (Codex)".to_string(),
        "Use these as stable user/project facts. Prefer project scope over global when they conflict."
            .to_string(),
    ];
    let header_len = header_lines.join("\n").chars().count();
    if header_len > max_chars {
        return String::new();
    }

    let mut total_chars = header_len;
    let mut selected = Vec::new();

    for row in rows {
        if selected.len() >= max_items {
            break;
        }
        let scope = format_memory_scope(&row.scope, &scope_info.scope);
        let line = format!(
            "- [{}{}/{}] {}",
            scope,
            if row.pinned { "/pinned" } else { "" },
            row.category,
            row.content
        );
        let line_len = line.chars().count() + 1;
        if total_chars + line_len > max_chars {
            continue;
        }
        total_chars += line_len;
        selected.push(line);
    }

    if selected.is_empty() {
        String::new()
    } else {
        [header_lines, selected].concat().join("\n")
    }
}

#[must_use]
pub fn format_stats(stats: &MemoryStats) -> String {
    [
        "Persistent memory stats".to_string(),
        String::new(),
        format!("- Active: {}", stats.active),
        format!("- Pinned: {}", stats.pinned),
        format!("- Project scope: {}", stats.project),
        format!("- Global scope: {}", stats.global),
        format!(
            "- FTS search: {}",
            if stats.has_fts {
                "enabled"
            } else {
                "fallback (LIKE)"
            }
        ),
    ]
    .join("\n")
}

#[must_use]
pub fn format_auto_capture_status(config: &MemoryConfig) -> String {
    [
        "Auto-capture status".to_string(),
        String::new(),
        format!(
            "- Enabled: {}",
            if config.auto_capture.enabled {
                "on"
            } else {
                "off"
            }
        ),
        format!("- Scope: {}", config.auto_capture.scope.as_str()),
        format!(
            "- Max captures per turn: {}",
            config.auto_capture.max_per_turn
        ),
        format!(
            "- Capture length: {}-{} chars",
            config.auto_capture.min_chars, config.auto_capture.max_chars
        ),
        String::new(),
        "Heuristic mode: explicit patterns only.".to_string(),
        "- Captures user statements like 'remember ...' and 'I prefer ...'".to_string(),
        "- Captures assistant lines prefixed with 'Memory:' or 'Remember:'".to_string(),
        "- Uses dedupe + secret filtering before write".to_string(),
    ]
    .join("\n")
}

#[must_use]
pub fn format_export_markdown(rows: &[MemoryRow]) -> String {
    let mut lines = Vec::new();
    lines.push("# Codex Extra Memory Export".to_string());
    lines.push(String::new());
    lines.push(format!("Generated: {}", now_iso()));
    lines.push(String::new());

    let mut grouped = std::collections::BTreeMap::<String, Vec<&MemoryRow>>::new();
    for row in rows {
        grouped.entry(row.scope.clone()).or_default().push(row);
    }

    for (scope, entries) in grouped {
        lines.push(format!("## {scope}"));
        lines.push(String::new());
        for row in entries {
            lines.push(format!(
                "- {} ({}, {})",
                row.id,
                row.category,
                if row.pinned { "pinned" } else { "unpinned" }
            ));
            lines.push(format!("  {}", row.content));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}
