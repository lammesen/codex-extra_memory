use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const START_MARKER: &str = "<!-- codex-extra-memory:start v1 -->";
pub const END_MARKER: &str = "<!-- codex-extra-memory:end -->";

fn normalize_document(mut doc: String) -> String {
    if !doc.ends_with('\n') {
        doc.push('\n');
    }
    doc
}

#[must_use]
pub fn render_managed_section(block: &str) -> String {
    format!("{START_MARKER}\n{block}\n{END_MARKER}")
}

#[must_use]
pub fn upsert_managed_section(existing: &str, managed_section: Option<&str>) -> String {
    let start = existing.find(START_MARKER);
    let end = existing.find(END_MARKER);

    let mut result = if let (Some(start_idx), Some(end_idx)) = (start, end) {
        let end_boundary = end_idx + END_MARKER.len();
        let before = existing[..start_idx].trim_end();
        let after = existing[end_boundary..].trim_start();

        match managed_section {
            Some(section) => {
                if before.is_empty() && after.is_empty() {
                    section.to_string()
                } else if before.is_empty() {
                    format!("{section}\n\n{after}")
                } else if after.is_empty() {
                    format!("{before}\n\n{section}")
                } else {
                    format!("{before}\n\n{section}\n\n{after}")
                }
            }
            None => {
                if before.is_empty() {
                    after.to_string()
                } else if after.is_empty() {
                    before.to_string()
                } else {
                    format!("{before}\n\n{after}")
                }
            }
        }
    } else {
        match managed_section {
            Some(section) => {
                if existing.trim().is_empty() {
                    section.to_string()
                } else {
                    format!("{}\n\n{section}", existing.trim_end())
                }
            }
            None => existing.trim_end().to_string(),
        }
    };

    if result.trim().is_empty() {
        result.clear();
    }

    normalize_document(result)
}

pub fn sync_agents_file(workspace_dir: &Path, block: Option<&str>) -> Result<(bool, PathBuf)> {
    let agents_path = workspace_dir.join("AGENTS.md");
    let existing = if agents_path.exists() {
        fs::read_to_string(&agents_path)
            .with_context(|| format!("read {}", agents_path.display()))?
    } else {
        String::new()
    };

    let managed_section = block.map(render_managed_section);
    let next = upsert_managed_section(&existing, managed_section.as_deref());

    if next == existing {
        return Ok((false, agents_path));
    }

    if let Some(parent) = agents_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(&agents_path, next).with_context(|| format!("write {}", agents_path.display()))?;

    Ok((true, agents_path))
}

#[cfg(test)]
mod tests {
    use super::{END_MARKER, START_MARKER, render_managed_section, upsert_managed_section};

    #[test]
    fn insert_section_into_empty_file() {
        let section = render_managed_section("## Memory\n- item");
        let out = upsert_managed_section("", Some(&section));
        assert!(out.contains(START_MARKER));
        assert!(out.contains(END_MARKER));
    }

    #[test]
    fn replace_existing_section() {
        let old = format!("Intro\n\n{START_MARKER}\nold\n{END_MARKER}\n");
        let section = render_managed_section("new");
        let out = upsert_managed_section(&old, Some(&section));
        assert!(out.contains("Intro"));
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
    }

    #[test]
    fn remove_section() {
        let old = format!("Intro\n\n{START_MARKER}\nold\n{END_MARKER}\n\nTail\n");
        let out = upsert_managed_section(&old, None);
        assert!(out.contains("Intro"));
        assert!(out.contains("Tail"));
        assert!(!out.contains(START_MARKER));
    }
}
