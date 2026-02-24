use crate::agents_sync::sync_agents_file;
use crate::autocapture::{extract_auto_capture_candidates, get_agent_end_messages};
use crate::commands::{AutoMode, COMMAND_HELP, ExportFormat, MemoryCommand, parse_memory_command};
use crate::config::{MemoryConfig, load_config_file_at, save_config_file_at};
use crate::llm::{LlmSummaryRequest, summarize_memories_with_llm};
use crate::paths::get_memory_dir;
use crate::render::{
    build_injection_block, format_auto_capture_status, format_export_markdown, format_stats,
    render_rows,
};
use crate::scope::detect_project_scope;
use crate::store::MemoryStore;
use crate::types::{
    AddMemoryInput, AddMemoryResult, CompactionMode, CompactionResult, MemoryCategory, PagedResult,
    ResolveIdResult, ScopeInfo, ScopeTarget, SyncAgentsResult,
};
use crate::utils::{format_memory_scope, now_iso, truncate_chars};
use anyhow::{Context, Result};
use base64::Engine;
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_AUTO_CAPTURE_TRACKED_HASHES: usize = 5_000;

fn ok(action: &str, data: Value) -> Value {
    json!({
        "ok": true,
        "action": action,
        "data": data,
    })
}

fn err(action: &str, message: impl AsRef<str>) -> Value {
    json!({
        "ok": false,
        "action": action,
        "error": message.as_ref(),
    })
}

fn cursor_encode(offset: usize) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(format!("o:{offset}"))
}

fn cursor_decode(cursor: Option<&str>) -> Result<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };

    let raw = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(cursor)
        .with_context(|| format!("invalid cursor token {cursor}"))?;
    let decoded = String::from_utf8(raw).context("cursor is not utf8")?;
    let Some(value) = decoded.strip_prefix("o:") else {
        anyhow::bail!("invalid cursor format")
    };
    let parsed = value
        .parse::<usize>()
        .context("cursor offset must be integer")?;
    Ok(parsed)
}

fn current_scopes(scope_info: &ScopeInfo) -> Vec<String> {
    vec![scope_info.scope.clone(), "global".to_string()]
}

fn scope_from_target(scope_info: &ScopeInfo, target: ScopeTarget) -> String {
    match target {
        ScopeTarget::Project => scope_info.scope.clone(),
        ScopeTarget::Global => "global".to_string(),
    }
}

fn cat_for_str(category: Option<String>) -> Result<MemoryCategory> {
    match category {
        Some(v) => v.parse::<MemoryCategory>().map_err(anyhow::Error::msg),
        None => Ok(MemoryCategory::Other),
    }
}

fn canonicalize_for_containment(path: &Path) -> std::result::Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| format!("canonicalize {}: {error}", path.display()));
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| format!("path '{}' has no final segment", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("path '{}' has no parent", path.display()))?;
    Ok(canonicalize_for_containment(parent)?.join(file_name))
}

fn resolve_export_path_within_workspace(
    workspace_dir: &Path,
    format: ExportFormat,
    output_path_raw: &str,
) -> std::result::Result<PathBuf, String> {
    let workspace = canonicalize_for_containment(workspace_dir)?;
    let candidate = if output_path_raw.trim().is_empty() {
        workspace_dir.join(format!(
            "codex-memory-export-{}.{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            format.extension()
        ))
    } else {
        let raw_path = PathBuf::from(output_path_raw.trim());
        if raw_path.is_absolute() {
            return Err("output path must be relative to workspace".to_string());
        }
        workspace_dir.join(raw_path)
    };

    let candidate = canonicalize_for_containment(&candidate)?;
    if candidate == workspace || candidate.starts_with(&workspace) {
        Ok(candidate)
    } else {
        Err(format!(
            "output path '{}' resolves outside workspace '{}'",
            output_path_raw.trim(),
            workspace_dir.display()
        ))
    }
}

pub struct MemoryService {
    store: MemoryStore,
    config: MemoryConfig,
    config_path: PathBuf,
    processed_hashes: HashSet<String>,
    processed_order: VecDeque<String>,
}

impl MemoryService {
    pub fn new() -> Result<Self> {
        let memory_dir = get_memory_dir();
        Self::new_with_memory_dir(&memory_dir)
    }

    pub fn new_with_memory_dir(memory_dir: &Path) -> Result<Self> {
        fs::create_dir_all(memory_dir)
            .with_context(|| format!("create memory dir {}", memory_dir.display()))?;

        let config_path = memory_dir.join("config.json");
        let db_path = memory_dir.join("memory.sqlite");

        let config = load_config_file_at(&config_path)?;
        let store = MemoryStore::open(&db_path)?;

        Ok(Self {
            store,
            config,
            config_path,
            processed_hashes: HashSet::new(),
            processed_order: VecDeque::new(),
        })
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    fn save_config(&self) -> Result<()> {
        save_config_file_at(&self.config_path, &self.config)
    }

    fn track_processed_hash(&mut self, hash: String) {
        if self.processed_hashes.contains(&hash) {
            return;
        }
        self.processed_hashes.insert(hash.clone());
        self.processed_order.push_back(hash);

        while self.processed_order.len() > MAX_AUTO_CAPTURE_TRACKED_HASHES {
            if let Some(first) = self.processed_order.pop_front() {
                self.processed_hashes.remove(&first);
            }
        }
    }

    fn detect_scope(workspace_dir: &Path) -> ScopeInfo {
        detect_project_scope(workspace_dir)
    }

    pub fn execute_command(&mut self, command: &str, workspace_dir: &Path) -> Result<Value> {
        let parsed = match parse_memory_command(command) {
            Ok(parsed) => parsed,
            Err(message) => return Ok(err("memory_command", message)),
        };

        match parsed {
            MemoryCommand::Help => Ok(ok("help", json!({"text": COMMAND_HELP}))),
            MemoryCommand::Refresh => self.refresh(),
            MemoryCommand::Sync => self.sync_agents(workspace_dir),
            MemoryCommand::Add(args) => self.add_memory(
                args.text,
                Some(args.scope_target),
                Some(args.category),
                workspace_dir,
                "user",
            ),
            MemoryCommand::Show => self.show_injection_preview(workspace_dir),
            MemoryCommand::List { limit, cursor } => {
                self.list_memories(workspace_dir, limit, cursor)
            }
            MemoryCommand::Search {
                query,
                limit,
                cursor,
            } => self.search_memories(workspace_dir, query, limit, cursor),
            MemoryCommand::Delete { id_or_prefix } => {
                self.delete_memory(workspace_dir, id_or_prefix)
            }
            MemoryCommand::Pin {
                id_or_prefix,
                enabled,
            } => self.pin_memory(workspace_dir, id_or_prefix, enabled),
            MemoryCommand::Auto { mode } => self.auto_capture_mode(mode),
            MemoryCommand::Stats => self.stats(workspace_dir),
            MemoryCommand::Export(args) => self.export_memories(
                workspace_dir,
                args.format,
                args.include_all_scopes,
                args.output_path_raw,
            ),
        }
    }

    pub fn add_memory(
        &mut self,
        fact: String,
        scope: Option<ScopeTarget>,
        category: Option<MemoryCategory>,
        workspace_dir: &Path,
        source: &str,
    ) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let target_scope = scope.unwrap_or(ScopeTarget::Project);
        let result = self.store.add_memory(AddMemoryInput {
            scope: scope_from_target(&scope_info, target_scope),
            category: category.unwrap_or(MemoryCategory::Other),
            content: fact,
            source: source.to_string(),
        })?;

        match result {
            AddMemoryResult::Blocked { reason } => Ok(err("add", reason)),
            AddMemoryResult::Added {
                id,
                scope,
                category,
                content,
            } => Ok(ok(
                "add",
                json!({
                    "result": "added",
                    "id": id,
                    "scope": scope,
                    "scope_label": format_memory_scope(&scope, &scope_info.scope),
                    "category": category,
                    "content": content,
                }),
            )),
            AddMemoryResult::Deduped {
                id,
                scope,
                category,
                content,
            } => Ok(ok(
                "add",
                json!({
                    "result": "deduped",
                    "id": id,
                    "scope": scope,
                    "scope_label": format_memory_scope(&scope, &scope_info.scope),
                    "category": category,
                    "content": content,
                }),
            )),
        }
    }

    pub fn list_memories(
        &self,
        workspace_dir: &Path,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);
        let limit = limit.unwrap_or(self.config.list_limit).min(200);
        let offset = cursor_decode(cursor.as_deref())?;
        let (items, has_more) = self.store.list_memories(&scopes, limit, offset)?;
        let next_cursor = has_more.then(|| cursor_encode(offset + limit));

        let page = PagedResult {
            items: items.clone(),
            next_cursor,
            limit,
            offset,
        };

        Ok(ok(
            "list",
            json!({
                "page": page,
                "rendered": render_rows(&items, &scope_info),
            }),
        ))
    }

    pub fn search_memories(
        &self,
        workspace_dir: &Path,
        query: String,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<Value> {
        if query.trim().is_empty() {
            return Ok(err("search", "query must not be empty"));
        }

        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);
        let limit = limit.unwrap_or(self.config.search_limit).min(200);
        let offset = cursor_decode(cursor.as_deref())?;

        let (items, has_more) = self.store.search_memories(&scopes, &query, limit, offset)?;
        let next_cursor = has_more.then(|| cursor_encode(offset + limit));

        let page = PagedResult {
            items: items.clone(),
            next_cursor,
            limit,
            offset,
        };

        Ok(ok(
            "search",
            json!({
                "query": query,
                "page": page,
                "rendered": if items.is_empty() {
                    "No memory matched query.".to_string()
                } else {
                    render_rows(&items, &scope_info)
                },
            }),
        ))
    }

    pub fn delete_memory(&mut self, workspace_dir: &Path, id_or_prefix: String) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);

        match self.store.resolve_id(&id_or_prefix, Some(&scopes))? {
            ResolveIdResult::Missing => Ok(err("delete", "Memory not found.")),
            ResolveIdResult::Ambiguous { candidates } => Ok(err(
                "delete",
                format!(
                    "Multiple memories match '{}': {}",
                    id_or_prefix,
                    candidates.join(", ")
                ),
            )),
            ResolveIdResult::Ok { id } => {
                let deleted = self.store.soft_delete_memory(&id)?;
                if deleted {
                    Ok(ok("delete", json!({"id": id, "deleted": true})))
                } else {
                    Ok(err("delete", "Memory not found."))
                }
            }
        }
    }

    pub fn pin_memory(
        &mut self,
        workspace_dir: &Path,
        id_or_prefix: String,
        enabled: bool,
    ) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);

        match self.store.resolve_id(&id_or_prefix, Some(&scopes))? {
            ResolveIdResult::Missing => Ok(err("pin", "Memory not found.")),
            ResolveIdResult::Ambiguous { candidates } => Ok(err(
                "pin",
                format!(
                    "Multiple memories match '{}': {}",
                    id_or_prefix,
                    candidates.join(", ")
                ),
            )),
            ResolveIdResult::Ok { id } => {
                let changed = self.store.set_pinned(&id, enabled)?;
                if changed {
                    Ok(ok(
                        "pin",
                        json!({"id": id, "pinned": enabled, "state": if enabled { "on" } else { "off" }}),
                    ))
                } else {
                    Ok(err("pin", "Memory not found."))
                }
            }
        }
    }

    pub fn stats(&self, workspace_dir: &Path) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);
        let stats = self.store.get_stats(&scopes)?;

        Ok(ok(
            "stats",
            json!({
                "stats": stats,
                "rendered": format_stats(&stats),
            }),
        ))
    }

    pub fn show_injection_preview(&mut self, workspace_dir: &Path) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let candidates = self.store.get_injection_candidates(
            &scope_info.scope,
            self.config.injection.max_items.saturating_mul(4).max(20),
        )?;

        let filtered = candidates
            .into_iter()
            .filter(|row| row.scope == scope_info.scope || (row.scope == "global" && row.pinned))
            .collect::<Vec<_>>();

        let block = build_injection_block(
            &filtered,
            &scope_info,
            self.config.injection.max_items,
            self.config.injection.max_chars,
        );

        Ok(ok(
            "show",
            json!({
                "scope": scope_info,
                "candidate_count": filtered.len(),
                "block": block,
            }),
        ))
    }

    pub fn auto_capture_mode(&mut self, mode: AutoMode) -> Result<Value> {
        match mode {
            AutoMode::Status => Ok(ok(
                "auto",
                json!({
                    "enabled": self.config.auto_capture.enabled,
                    "scope": self.config.auto_capture.scope,
                    "rendered": format_auto_capture_status(&self.config),
                }),
            )),
            AutoMode::On => {
                self.config.auto_capture.enabled = true;
                self.save_config()?;
                Ok(ok("auto", json!({"enabled": true})))
            }
            AutoMode::Off => {
                self.config.auto_capture.enabled = false;
                self.save_config()?;
                Ok(ok("auto", json!({"enabled": false})))
            }
        }
    }

    pub fn refresh(&mut self) -> Result<Value> {
        self.store.refresh(&self.config)?;
        Ok(ok("refresh", json!({"refreshed": true})))
    }

    pub fn export_memories(
        &self,
        workspace_dir: &Path,
        format: ExportFormat,
        include_all_scopes: bool,
        output_path_raw: String,
    ) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let scopes = current_scopes(&scope_info);

        let output_path =
            match resolve_export_path_within_workspace(workspace_dir, format, &output_path_raw) {
                Ok(path) => path,
                Err(message) => return Ok(err("export", message)),
            };

        let entries = if include_all_scopes {
            self.store.export_active_memories(None)?
        } else {
            self.store.export_active_memories(Some(&scopes))?
        };

        let stats = if include_all_scopes {
            let all_scopes = entries
                .iter()
                .map(|row| row.scope.clone())
                .collect::<HashSet<_>>();
            self.store
                .get_stats(&all_scopes.into_iter().collect::<Vec<String>>())?
        } else {
            self.store.get_stats(&scopes)?
        };

        let payload = match format {
            ExportFormat::Json => serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "generated_at": now_iso(),
                "workspace_root": workspace_dir.to_string_lossy(),
                "project_scope": scope_info.scope,
                "include_all_scopes": include_all_scopes,
                "stats_snapshot": stats,
                "entries": entries,
            }))?,
            ExportFormat::Markdown => format_export_markdown(&entries),
        };

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create export directory {}", parent.display()))?;
        }
        fs::write(&output_path, payload)
            .with_context(|| format!("write export {}", output_path.display()))?;

        Ok(ok(
            "export",
            json!({
                "count": entries.len(),
                "format": format.as_str(),
                "path": output_path,
            }),
        ))
    }

    fn deterministic_compaction_block(
        &self,
        scope_info: &ScopeInfo,
        rows: &[crate::types::MemoryRow],
    ) -> String {
        let header = [
            "## Extra Memory (Codex)",
            "Compacted memory summary (deterministic fallback).",
        ]
        .join("\n");

        let mut lines = vec![header];
        let mut chars = lines[0].chars().count();
        let mut selected = 0_usize;

        for row in rows {
            if selected >= self.config.injection.max_items {
                break;
            }
            let scope = format_memory_scope(&row.scope, &scope_info.scope);
            let max_content = if row.pinned { 220 } else { 160 };
            let content = truncate_chars(&row.content, max_content);
            let line = format!("- [{scope}/{}] {content}", row.category);
            let needed = line.chars().count() + 1;
            if chars + needed > self.config.injection.max_chars {
                continue;
            }
            lines.push(line);
            chars += needed;
            selected += 1;
        }

        if lines.len() <= 1 {
            String::new()
        } else {
            lines.join("\n")
        }
    }

    fn compact_block_for_agents(
        &mut self,
        scope_info: &ScopeInfo,
        rows: &[crate::types::MemoryRow],
    ) -> CompactionResult {
        let raw_block = build_injection_block(
            rows,
            scope_info,
            self.config.injection.max_items,
            self.config.injection.max_chars,
        );
        let input_chars = rows
            .iter()
            .map(|row| row.content.chars().count())
            .sum::<usize>();

        let over_budget = rows.len() > self.config.injection.max_items
            || rows
                .iter()
                .map(|row| row.content.chars().count())
                .sum::<usize>()
                > self.config.injection.max_chars;

        if !raw_block.is_empty() && !over_budget {
            return CompactionResult {
                mode: CompactionMode::None,
                block: raw_block.clone(),
                input_count: rows.len(),
                output_count: rows.len().min(self.config.injection.max_items),
                input_chars,
                output_chars: raw_block.chars().count(),
                model: None,
                reason: None,
            };
        }

        if self.config.llm_compaction.enabled {
            let llm_request = LlmSummaryRequest {
                model: self.config.llm_compaction.model.clone(),
                timeout_ms: self.config.llm_compaction.timeout_ms,
                max_output_chars: self
                    .config
                    .llm_compaction
                    .max_output_chars
                    .min(self.config.injection.max_chars.saturating_sub(100)),
            };

            match summarize_memories_with_llm(rows, &llm_request) {
                Ok(Some(summary)) if !summary.trim().is_empty() => {
                    let mut block_lines = vec![
                        "## Extra Memory (Codex)".to_string(),
                        format!("Compacted memory summary via {}.", llm_request.model),
                    ];

                    let mut used = block_lines.join("\n").chars().count();
                    let mut output_count = 0_usize;
                    for line in summary
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                    {
                        if output_count >= self.config.injection.max_items {
                            break;
                        }
                        let normalized = if line.starts_with("- ") {
                            line.to_string()
                        } else {
                            format!("- {line}")
                        };
                        let needed = normalized.chars().count() + 1;
                        if used + needed > self.config.injection.max_chars {
                            continue;
                        }
                        block_lines.push(normalized);
                        used += needed;
                        output_count += 1;
                    }

                    let block = block_lines.join("\n");
                    if output_count > 0 {
                        return CompactionResult {
                            mode: CompactionMode::Llm,
                            block: block.clone(),
                            input_count: rows.len(),
                            output_count,
                            input_chars,
                            output_chars: block.chars().count(),
                            model: Some(llm_request.model),
                            reason: None,
                        };
                    }
                }
                Ok(None | Some(_)) => {}
                Err(error) => {
                    let block = self.deterministic_compaction_block(scope_info, rows);
                    return CompactionResult {
                        mode: CompactionMode::LlmFallback,
                        block: block.clone(),
                        input_count: rows.len(),
                        output_count: block.lines().filter(|line| line.starts_with("- ")).count(),
                        input_chars,
                        output_chars: block.chars().count(),
                        model: Some(self.config.llm_compaction.model.clone()),
                        reason: Some(error.to_string()),
                    };
                }
            }
        }

        let block = self.deterministic_compaction_block(scope_info, rows);
        CompactionResult {
            mode: CompactionMode::Deterministic,
            block: block.clone(),
            input_count: rows.len(),
            output_count: block.lines().filter(|line| line.starts_with("- ")).count(),
            input_chars,
            output_chars: block.chars().count(),
            model: None,
            reason: None,
        }
    }

    pub fn sync_agents(&mut self, workspace_dir: &Path) -> Result<Value> {
        let scope_info = Self::detect_scope(workspace_dir);
        let candidates = self.store.get_injection_candidates(
            &scope_info.scope,
            self.config.injection.max_items.saturating_mul(4).max(20),
        )?;

        let selected = candidates
            .into_iter()
            .filter(|row| row.scope == scope_info.scope || (row.scope == "global" && row.pinned))
            .collect::<Vec<_>>();

        let compaction = if selected.is_empty() {
            CompactionResult {
                mode: CompactionMode::None,
                block: String::new(),
                input_count: 0,
                output_count: 0,
                input_chars: 0,
                output_chars: 0,
                model: None,
                reason: None,
            }
        } else {
            self.compact_block_for_agents(&scope_info, &selected)
        };

        self.store.record_compaction(
            &scope_info.scope,
            compaction.mode.clone(),
            compaction.input_chars,
            compaction.output_chars,
            compaction.input_count,
            compaction.model.as_deref(),
            compaction.reason.as_deref(),
            json!({
                "selected": selected.len(),
                "workspace": workspace_dir,
            }),
        );

        let block = if compaction.block.trim().is_empty() {
            None
        } else {
            Some(compaction.block.as_str())
        };

        let (changed, agents_path) = sync_agents_file(workspace_dir, block)?;

        let result = SyncAgentsResult {
            changed,
            agents_path: agents_path.to_string_lossy().to_string(),
            applied_on_next_session: true,
            selected_memories: selected.len(),
            compaction,
        };

        Ok(ok("sync", serde_json::to_value(result)?))
    }

    pub fn capture_candidates(
        &mut self,
        workspace_dir: &Path,
        event_payload: Value,
        persist: bool,
    ) -> Result<Value> {
        let messages = get_agent_end_messages(&event_payload);
        let candidates = extract_auto_capture_candidates(
            &messages,
            &self.config.auto_capture,
            &self.processed_hashes,
        );

        let scope_info = Self::detect_scope(workspace_dir);

        let mut added = 0_usize;
        let mut deduped = 0_usize;
        let mut blocked = 0_usize;

        if persist && self.config.auto_capture.enabled {
            for candidate in &candidates {
                let scope = scope_from_target(&scope_info, self.config.auto_capture.scope);
                let result = self.store.add_memory(AddMemoryInput {
                    scope,
                    category: candidate.category,
                    content: candidate.text.clone(),
                    source: "auto".to_string(),
                })?;

                match result {
                    AddMemoryResult::Added { .. } => {
                        added += 1;
                        self.track_processed_hash(candidate.hash.clone());
                    }
                    AddMemoryResult::Deduped { .. } => {
                        deduped += 1;
                        self.track_processed_hash(candidate.hash.clone());
                    }
                    AddMemoryResult::Blocked { .. } => {
                        blocked += 1;
                    }
                }
            }
        }

        Ok(ok(
            "capture_candidates",
            json!({
                "enabled": self.config.auto_capture.enabled,
                "persisted": persist,
                "candidates": candidates,
                "added": added,
                "deduped": deduped,
                "blocked": blocked,
            }),
        ))
    }

    pub fn memory_add_typed(
        &mut self,
        workspace_dir: &Path,
        fact: String,
        scope: Option<String>,
        category: Option<String>,
    ) -> Result<Value> {
        let scope_target = match scope {
            Some(value) => value.parse::<ScopeTarget>().map_err(anyhow::Error::msg)?,
            None => ScopeTarget::Project,
        };
        let category = cat_for_str(category)?;
        self.add_memory(
            fact,
            Some(scope_target),
            Some(category),
            workspace_dir,
            "tool",
        )
    }
}
