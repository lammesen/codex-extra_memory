use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    Preference,
    Workflow,
    Constraint,
    Fact,
    Decision,
    Convention,
    Other,
}

impl MemoryCategory {
    pub const ALL: [Self; 7] = [
        Self::Preference,
        Self::Workflow,
        Self::Constraint,
        Self::Fact,
        Self::Decision,
        Self::Convention,
        Self::Other,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Workflow => "workflow",
            Self::Constraint => "constraint",
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Convention => "convention",
            Self::Other => "other",
        }
    }
}

impl Display for MemoryCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MemoryCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "preference" => Ok(Self::Preference),
            "workflow" => Ok(Self::Workflow),
            "constraint" => Ok(Self::Constraint),
            "fact" => Ok(Self::Fact),
            "decision" => Ok(Self::Decision),
            "convention" => Ok(Self::Convention),
            "other" => Ok(Self::Other),
            _ => Err(format!(
                "Invalid category '{s}'. Allowed: {}",
                Self::ALL
                    .iter()
                    .map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScopeTarget {
    Project,
    Global,
}

impl ScopeTarget {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

impl FromStr for ScopeTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "project" => Ok(Self::Project),
            "global" => Ok(Self::Global),
            _ => Err("Scope must be 'project' or 'global'".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: String,
    pub scope: String,
    pub category: MemoryCategory,
    pub content: String,
    pub content_hash: String,
    pub status: String,
    pub pinned: bool,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemoryInput {
    pub scope: String,
    pub category: MemoryCategory,
    pub content: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum AddMemoryResult {
    Added {
        id: String,
        scope: String,
        category: MemoryCategory,
        content: String,
    },
    Deduped {
        id: String,
        scope: String,
        category: MemoryCategory,
        content: String,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResolveIdResult {
    Ok { id: String },
    Missing,
    Ambiguous { candidates: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoCaptureConfig {
    pub enabled: bool,
    pub scope: ScopeTarget,
    pub max_per_turn: usize,
    pub min_chars: usize,
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeInfo {
    pub scope: String,
    pub kind: String,
    pub identifier: String,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoCaptureCandidate {
    pub hash: String,
    pub text: String,
    pub category: MemoryCategory,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub active: i64,
    pub pinned: i64,
    pub global: i64,
    pub project: i64,
    pub has_fts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionMode {
    None,
    Deterministic,
    Llm,
    LlmFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub mode: CompactionMode,
    pub block: String,
    pub input_count: usize,
    pub output_count: usize,
    pub input_chars: usize,
    pub output_chars: usize,
    pub model: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAgentsResult {
    pub changed: bool,
    pub agents_path: String,
    pub applied_on_next_session: bool,
    pub selected_memories: usize,
    pub compaction: CompactionResult,
}
