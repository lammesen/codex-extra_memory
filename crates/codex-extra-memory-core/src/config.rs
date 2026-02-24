use crate::paths::{get_config_path, get_memory_dir};
use crate::types::{AutoCaptureConfig, ScopeTarget};
use crate::utils::{parse_boolean, parse_positive_int};
use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectionConfig {
    pub max_items: usize,
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmCompactionConfig {
    pub enabled: bool,
    pub model: String,
    pub timeout_ms: u64,
    pub max_output_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionConfig {
    pub event_days: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConfig {
    pub injection: InjectionConfig,
    pub list_limit: usize,
    pub search_limit: usize,
    pub auto_capture: AutoCaptureConfig,
    pub llm_compaction: LlmCompactionConfig,
    pub retention: RetentionConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            injection: InjectionConfig {
                max_items: 10,
                max_chars: 3_000,
            },
            list_limit: 50,
            search_limit: 20,
            auto_capture: AutoCaptureConfig {
                enabled: true,
                scope: ScopeTarget::Project,
                max_per_turn: 2,
                min_chars: 12,
                max_chars: 240,
            },
            llm_compaction: LlmCompactionConfig {
                enabled: true,
                model: "gpt-5-mini".to_string(),
                timeout_ms: 8_000,
                max_output_chars: 1_500,
            },
            retention: RetentionConfig { event_days: 180 },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialInjectionConfig {
    max_items: Option<usize>,
    max_chars: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialAutoCaptureConfig {
    enabled: Option<serde_json::Value>,
    scope: Option<String>,
    max_per_turn: Option<usize>,
    min_chars: Option<usize>,
    max_chars: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialLlmCompactionConfig {
    enabled: Option<serde_json::Value>,
    model: Option<String>,
    timeout_ms: Option<u64>,
    max_output_chars: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialRetentionConfig {
    event_days: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialMemoryConfig {
    injection: Option<PartialInjectionConfig>,
    list_limit: Option<usize>,
    search_limit: Option<usize>,
    auto_capture: Option<PartialAutoCaptureConfig>,
    llm_compaction: Option<PartialLlmCompactionConfig>,
    retention: Option<PartialRetentionConfig>,
}

fn normalize_config(partial: PartialMemoryConfig) -> MemoryConfig {
    let defaults = MemoryConfig::default();

    let mut auto_min = parse_positive_int(
        partial
            .auto_capture
            .as_ref()
            .and_then(|c| c.min_chars)
            .map(|x| x as i64),
        defaults.auto_capture.min_chars,
    );
    let mut auto_max = parse_positive_int(
        partial
            .auto_capture
            .as_ref()
            .and_then(|c| c.max_chars)
            .map(|x| x as i64),
        defaults.auto_capture.max_chars,
    );
    if auto_min > auto_max {
        std::mem::swap(&mut auto_min, &mut auto_max);
    }

    let scope = partial
        .auto_capture
        .as_ref()
        .and_then(|c| c.scope.clone())
        .and_then(|x| x.parse().ok())
        .unwrap_or(defaults.auto_capture.scope);

    MemoryConfig {
        injection: InjectionConfig {
            max_items: parse_positive_int(
                partial
                    .injection
                    .as_ref()
                    .and_then(|i| i.max_items)
                    .map(|x| x as i64),
                defaults.injection.max_items,
            ),
            max_chars: parse_positive_int(
                partial
                    .injection
                    .as_ref()
                    .and_then(|i| i.max_chars)
                    .map(|x| x as i64),
                defaults.injection.max_chars,
            ),
        },
        list_limit: parse_positive_int(partial.list_limit.map(|x| x as i64), defaults.list_limit),
        search_limit: parse_positive_int(
            partial.search_limit.map(|x| x as i64),
            defaults.search_limit,
        ),
        auto_capture: AutoCaptureConfig {
            enabled: parse_boolean(
                partial
                    .auto_capture
                    .as_ref()
                    .and_then(|c| c.enabled.clone()),
                defaults.auto_capture.enabled,
            ),
            scope,
            max_per_turn: parse_positive_int(
                partial
                    .auto_capture
                    .as_ref()
                    .and_then(|c| c.max_per_turn)
                    .map(|x| x as i64),
                defaults.auto_capture.max_per_turn,
            ),
            min_chars: auto_min,
            max_chars: auto_max,
        },
        llm_compaction: LlmCompactionConfig {
            enabled: parse_boolean(
                partial
                    .llm_compaction
                    .as_ref()
                    .and_then(|c| c.enabled.clone()),
                defaults.llm_compaction.enabled,
            ),
            model: partial
                .llm_compaction
                .as_ref()
                .and_then(|c| c.model.clone())
                .filter(|m| !m.trim().is_empty())
                .unwrap_or(defaults.llm_compaction.model),
            timeout_ms: partial
                .llm_compaction
                .as_ref()
                .and_then(|c| c.timeout_ms)
                .unwrap_or(defaults.llm_compaction.timeout_ms),
            max_output_chars: parse_positive_int(
                partial
                    .llm_compaction
                    .as_ref()
                    .and_then(|c| c.max_output_chars)
                    .map(|x| x as i64),
                defaults.llm_compaction.max_output_chars,
            ),
        },
        retention: RetentionConfig {
            event_days: partial
                .retention
                .as_ref()
                .and_then(|r| r.event_days)
                .unwrap_or(defaults.retention.event_days),
        },
    }
}

fn next_invalid_backup_path(config_path: &Path) -> anyhow::Result<PathBuf> {
    let Some(parent) = config_path.parent() else {
        anyhow::bail!("invalid config path")
    };
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");

    for suffix in 0..10_000 {
        let file_name = if suffix == 0 {
            format!("config.invalid-{stamp}.json.bak")
        } else {
            format!("config.invalid-{stamp}-{suffix}.json.bak")
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!("could not allocate invalid config backup path")
}

fn backup_invalid_config(config_path: &Path) -> anyhow::Result<PathBuf> {
    let backup_path = next_invalid_backup_path(config_path)?;
    fs::rename(config_path, &backup_path).with_context(|| {
        format!(
            "backup invalid config {} to {}",
            config_path.display(),
            backup_path.display()
        )
    })?;
    Ok(backup_path)
}

pub fn load_config_file_at(config_path: &Path) -> anyhow::Result<MemoryConfig> {
    let Some(parent) = config_path.parent() else {
        anyhow::bail!("invalid config path")
    };
    fs::create_dir_all(parent)
        .with_context(|| format!("create config dir {}", parent.display()))?;

    if !config_path.exists() {
        let default = MemoryConfig::default();
        save_config_file_at(config_path, &default)?;
        return Ok(default);
    }

    let raw = fs::read_to_string(config_path)
        .with_context(|| format!("read config {}", config_path.display()))?;

    match serde_json::from_str::<PartialMemoryConfig>(&raw) {
        Ok(parsed) => Ok(normalize_config(parsed)),
        Err(error) => {
            let backup_path = backup_invalid_config(config_path)?;
            eprintln!(
                "codex-extra-memory: invalid config at {} ({}). Backed up to {} and regenerated defaults.",
                config_path.display(),
                error,
                backup_path.display()
            );
            let default = MemoryConfig::default();
            save_config_file_at(config_path, &default)?;
            Ok(default)
        }
    }
}

pub fn save_config_file_at(config_path: &Path, config: &MemoryConfig) -> anyhow::Result<()> {
    let Some(parent) = config_path.parent() else {
        anyhow::bail!("invalid config path")
    };
    fs::create_dir_all(parent)
        .with_context(|| format!("create config dir {}", parent.display()))?;
    let text = serde_json::to_string_pretty(config)?;
    fs::write(config_path, format!("{text}\n"))
        .with_context(|| format!("write config {}", config_path.display()))?;
    Ok(())
}

pub fn load_config_file() -> anyhow::Result<MemoryConfig> {
    let memory_dir = get_memory_dir();
    fs::create_dir_all(&memory_dir)
        .with_context(|| format!("create memory dir {}", memory_dir.display()))?;
    load_config_file_at(&get_config_path())
}

pub fn save_config_file(config: &MemoryConfig) -> anyhow::Result<()> {
    save_config_file_at(&get_config_path(), config)
}

#[cfg(test)]
mod tests {
    use super::{MemoryConfig, load_config_file_at};
    use std::fs;

    #[test]
    fn invalid_config_creates_backup_and_regenerates_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        fs::write(&config_path, "{ invalid json").expect("write invalid config");

        let config = load_config_file_at(&config_path).expect("load config");
        assert_eq!(config.list_limit, MemoryConfig::default().list_limit);

        let backups = fs::read_dir(temp.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("config.invalid-") && name.ends_with(".json.bak"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);

        let rewritten = fs::read_to_string(&config_path).expect("read rewritten config");
        assert!(serde_json::from_str::<MemoryConfig>(&rewritten).is_ok());
    }

    #[test]
    fn valid_config_keeps_content_without_backup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        fs::write(
            &config_path,
            r#"{"listLimit": 7, "searchLimit": 4, "autoCapture": {"enabled": false}}"#,
        )
        .expect("write config");

        let config = load_config_file_at(&config_path).expect("load valid config");
        assert_eq!(config.list_limit, 7);
        assert_eq!(config.search_limit, 4);
        assert!(!config.auto_capture.enabled);

        let backups = fs::read_dir(temp.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("config.invalid-") && name.ends_with(".json.bak"))
            .collect::<Vec<_>>();
        assert!(backups.is_empty());
    }
}
