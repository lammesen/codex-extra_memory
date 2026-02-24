use crate::config::MemoryConfig;
use crate::types::{
    AddMemoryInput, AddMemoryResult, CompactionMode, MemoryCategory, MemoryRow, MemoryStats,
    ResolveIdResult,
};
use crate::utils::{escape_like, normalize_for_hash, now_iso, sanitize_memory_text, sha256};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};
use uuid::Uuid;

fn load_schema_v1() -> &'static str {
    r"
CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL,
  category TEXT NOT NULL DEFAULT 'other',
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'deleted', 'superseded')),
  pinned INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'user',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_scope_hash_active
ON memories(scope, content_hash)
WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_memories_scope_status_updated
ON memories(scope, status, pinned DESC, updated_at DESC);

CREATE TABLE IF NOT EXISTS memory_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id TEXT NOT NULL,
  action TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  payload TEXT
);
"
}

fn parse_category(raw: &str) -> MemoryCategory {
    raw.parse().unwrap_or(MemoryCategory::Other)
}

fn parse_ts(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc))
}

fn row_from_stmt(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRow> {
    let category_raw: String = row.get("category")?;
    let pinned: i64 = row.get("pinned")?;

    Ok(MemoryRow {
        id: row.get("id")?,
        scope: row.get("scope")?,
        category: parse_category(&category_raw),
        content: row.get("content")?,
        content_hash: row.get("content_hash")?,
        status: row.get("status")?,
        pinned: pinned != 0,
        source: row.get("source")?,
        created_at: parse_ts(&row.get::<_, String>("created_at")?),
        updated_at: parse_ts(&row.get::<_, String>("updated_at")?),
    })
}

fn scopes_in_clause(scopes: &[String]) -> String {
    format!("({})", vec!["?"; scopes.len()].join(","))
}

fn with_scopes(scopes: &[String]) -> Vec<Value> {
    scopes.iter().map(|s| Value::Text(s.clone())).collect()
}

pub struct MemoryStore {
    conn: Connection,
    pub has_fts: bool,
}

impl MemoryStore {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("open sqlite db {}", db_path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5_000_i64)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let mut store = Self {
            conn,
            has_fts: false,
        };
        store.migrate()?;
        store.setup_fts();
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
              version INTEGER PRIMARY KEY,
              applied_at TEXT NOT NULL
            );
            ",
        )?;

        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version < 1 {
            self.conn.execute_batch(load_schema_v1())?;
            self.conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                params![1_i64, now_iso()],
            )?;
        }

        if version < 2 {
            self.conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_memory_events_timestamp ON memory_events(timestamp)",
                [],
            )?;
            self.conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                params![2_i64, now_iso()],
            )?;
        }

        if version < 3 {
            self.conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS memory_compactions (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  scope TEXT NOT NULL,
                  mode TEXT NOT NULL,
                  input_chars INTEGER NOT NULL,
                  output_chars INTEGER NOT NULL,
                  source_count INTEGER NOT NULL,
                  model TEXT,
                  reason TEXT,
                  details TEXT,
                  created_at TEXT NOT NULL
                );
                ",
            )?;
            self.conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                params![3_i64, now_iso()],
            )?;
        }

        Ok(())
    }

    fn setup_fts(&mut self) {
        let result = self.conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts
            USING fts5(id UNINDEXED, scope UNINDEXED, category UNINDEXED, content);
            ",
        );
        if result.is_ok() {
            self.has_fts = true;
            if let Err(err) = self.ensure_fts_synced() {
                eprintln!("codex-extra-memory: failed to sync fts on startup: {err}");
            }
        } else {
            self.has_fts = false;
        }
    }

    fn add_event(&mut self, memory_id: &str, action: &str, payload: Option<&serde_json::Value>) {
        let payload_text = payload.and_then(|p| serde_json::to_string(p).ok());
        let _ = self.conn.execute(
            "INSERT INTO memory_events (memory_id, action, timestamp, payload) VALUES (?, ?, ?, ?)",
            params![memory_id, action, now_iso(), payload_text],
        );
    }

    fn remove_fts_entry(&mut self, memory_id: &str) {
        if !self.has_fts {
            return;
        }
        let _ = self
            .conn
            .execute("DELETE FROM memories_fts WHERE id = ?", params![memory_id]);
    }

    pub fn ensure_fts_synced(&mut self) -> Result<()> {
        if !self.has_fts {
            return Ok(());
        }

        let active_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;
        let fts_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM memories_fts", [], |row| row.get(0))?;

        if active_count != fts_count {
            self.rebuild_fts()?;
            return Ok(());
        }

        let missing = self
            .conn
            .query_row(
                "
                SELECT m.id FROM memories m
                LEFT JOIN memories_fts f ON f.id = m.id
                WHERE m.status = 'active' AND f.id IS NULL
                LIMIT 1
                ",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if missing.is_some() {
            self.rebuild_fts()?;
            return Ok(());
        }

        let orphaned = self
            .conn
            .query_row(
                "
                SELECT f.id FROM memories_fts f
                LEFT JOIN memories m ON m.id = f.id AND m.status = 'active'
                WHERE m.id IS NULL
                LIMIT 1
                ",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if orphaned.is_some() {
            self.rebuild_fts()?;
        }

        Ok(())
    }

    pub fn rebuild_fts(&mut self) -> Result<()> {
        if !self.has_fts {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM memories_fts", [])?;

        let mut stmt = tx.prepare(
            "
            SELECT id, scope, category, content
            FROM memories
            WHERE status = 'active'
            ORDER BY updated_at DESC
            ",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let scope: String = row.get(1)?;
            let category: String = row.get(2)?;
            let content: String = row.get(3)?;
            tx.execute(
                "INSERT INTO memories_fts (id, scope, category, content) VALUES (?, ?, ?, ?)",
                params![id, scope, category, content],
            )?;
        }

        drop(rows);
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    pub fn add_memory(&mut self, input: AddMemoryInput) -> Result<AddMemoryResult> {
        let sanitized = match sanitize_memory_text(&input.content) {
            Ok(text) => text,
            Err(reason) => {
                return Ok(AddMemoryResult::Blocked { reason });
            }
        };

        let content_hash = sha256(&normalize_for_hash(&sanitized));
        let existing = self
            .conn
            .query_row(
                "SELECT id, category, content FROM memories WHERE scope = ? AND content_hash = ? AND status = 'active' LIMIT 1",
                params![input.scope, content_hash],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;

        if let Some((id, category, content)) = existing {
            self.conn.execute(
                "UPDATE memories SET updated_at = ? WHERE id = ?",
                params![now_iso(), id],
            )?;
            self.add_event(
                &id,
                "deduped",
                Some(&serde_json::json!({"scope": input.scope, "source": input.source})),
            );
            return Ok(AddMemoryResult::Deduped {
                id,
                scope: input.scope,
                category: parse_category(&category),
                content,
            });
        }

        let id = Uuid::new_v4().to_string();
        let timestamp = now_iso();
        let row = MemoryRow {
            id: id.clone(),
            scope: input.scope.clone(),
            category: input.category,
            content: sanitized.clone(),
            content_hash: content_hash.clone(),
            status: "active".to_string(),
            pinned: false,
            source: input.source.clone(),
            created_at: parse_ts(&timestamp),
            updated_at: parse_ts(&timestamp),
        };

        let tx = self.conn.transaction()?;
        tx.execute(
            "
            INSERT INTO memories
            (id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, 'active', 0, ?, ?, ?)
            ",
            params![
                row.id,
                row.scope,
                row.category.as_str(),
                row.content,
                row.content_hash,
                row.source,
                timestamp,
                timestamp,
            ],
        )?;

        if self.has_fts {
            tx.execute(
                "INSERT INTO memories_fts (id, scope, category, content) VALUES (?, ?, ?, ?)",
                params![row.id, row.scope, row.category.as_str(), row.content],
            )?;
        }
        tx.commit()?;

        self.add_event(
            &id,
            "added",
            Some(&serde_json::json!({
                "scope": input.scope,
                "category": input.category,
                "source": input.source,
            })),
        );

        Ok(AddMemoryResult::Added {
            id,
            scope: input.scope,
            category: input.category,
            content: sanitized,
        })
    }

    pub fn resolve_id(
        &self,
        id_or_prefix: &str,
        scopes: Option<&[String]>,
    ) -> Result<ResolveIdResult> {
        let normalized = id_or_prefix.trim();
        if normalized.is_empty() {
            return Ok(ResolveIdResult::Missing);
        }

        let scope_filter = scopes.filter(|s| !s.is_empty());
        let scope_sql = scope_filter
            .map(|s| format!(" AND scope IN {}", scopes_in_clause(s)))
            .unwrap_or_default();

        if Uuid::parse_str(normalized).is_ok() {
            let mut values = vec![Value::Text(normalized.to_string())];
            if let Some(scopes) = scope_filter {
                values.extend(with_scopes(scopes));
            }
            let sql = format!(
                "SELECT id FROM memories WHERE id = ? AND status = 'active'{scope_sql} LIMIT 1"
            );
            let found = self
                .conn
                .query_row(&sql, params_from_iter(values), |row| {
                    row.get::<_, String>(0)
                })
                .optional()?;
            if let Some(id) = found {
                return Ok(ResolveIdResult::Ok { id });
            }
            return Ok(ResolveIdResult::Missing);
        }

        let escaped = escape_like(normalized);
        let mut values = vec![Value::Text(format!("{escaped}%"))];
        if let Some(scopes) = scope_filter {
            values.extend(with_scopes(scopes));
        }
        values.push(Value::Integer(5_i64));

        let sql = format!(
            "
            SELECT id
            FROM memories
            WHERE id LIKE ? ESCAPE '\\' AND status = 'active'{scope_sql}
            ORDER BY updated_at DESC
            LIMIT ?
            "
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let ids = stmt
            .query_map(params_from_iter(values), |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        if ids.is_empty() {
            Ok(ResolveIdResult::Missing)
        } else if ids.len() > 1 {
            Ok(ResolveIdResult::Ambiguous { candidates: ids })
        } else {
            Ok(ResolveIdResult::Ok { id: ids[0].clone() })
        }
    }

    pub fn list_memories(
        &self,
        scopes: &[String],
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<MemoryRow>, bool)> {
        if scopes.is_empty() {
            return Ok((Vec::new(), false));
        }

        let scope_clause = scopes_in_clause(scopes);
        let sql = format!(
            "
            SELECT id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at
            FROM memories
            WHERE status = 'active' AND scope IN {scope_clause}
            ORDER BY pinned DESC, updated_at DESC
            LIMIT ? OFFSET ?
            "
        );

        let mut values = with_scopes(scopes);
        values.push(Value::Integer((limit as i64) + 1));
        values.push(Value::Integer(offset as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let mut items = stmt
            .query_map(params_from_iter(values), row_from_stmt)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let has_more = items.len() > limit;
        if has_more {
            items.pop();
        }

        Ok((items, has_more))
    }

    fn to_fts_query(raw: &str) -> String {
        let terms = raw
            .split_whitespace()
            .map(|token| {
                token
                    .trim()
                    .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
            })
            .filter(|token| token.len() >= 2)
            .take(8)
            .map(|token| format!("{token}*"))
            .collect::<Vec<_>>();

        terms.join(" AND ")
    }

    pub fn search_memories(
        &self,
        scopes: &[String],
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<MemoryRow>, bool)> {
        let cleaned = query.trim();
        if cleaned.is_empty() || scopes.is_empty() {
            return Ok((Vec::new(), false));
        }

        if self.has_fts {
            let fts_query = Self::to_fts_query(cleaned);
            if !fts_query.is_empty() {
                let scope_clause = scopes_in_clause(scopes);
                let sql = format!(
                    "
                    SELECT m.id, m.scope, m.category, m.content, m.content_hash, m.status, m.pinned, m.source, m.created_at, m.updated_at
                    FROM memories_fts
                    JOIN memories m ON m.id = memories_fts.id
                    WHERE memories_fts MATCH ?
                      AND m.status = 'active'
                      AND m.scope IN {scope_clause}
                    ORDER BY bm25(memories_fts), m.updated_at DESC
                    LIMIT ? OFFSET ?
                    "
                );

                let mut values = vec![Value::Text(fts_query)];
                values.extend(with_scopes(scopes));
                values.push(Value::Integer((limit as i64) + 1));
                values.push(Value::Integer(offset as i64));

                if let Ok(mut stmt) = self.conn.prepare(&sql)
                    && let Ok(mapped) = stmt.query_map(params_from_iter(values), row_from_stmt)
                {
                    let mut items = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
                    let has_more = items.len() > limit;
                    if has_more {
                        items.pop();
                    }
                    if !items.is_empty() {
                        return Ok((items, has_more));
                    }
                }
            }
        }

        let escaped_query = escape_like(cleaned);
        let scope_clause = scopes_in_clause(scopes);
        let sql = format!(
            "
            SELECT id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at
            FROM memories
            WHERE status = 'active'
              AND scope IN {scope_clause}
              AND lower(content) LIKE '%' || lower(?) || '%' ESCAPE '\\'
            ORDER BY pinned DESC, updated_at DESC
            LIMIT ? OFFSET ?
            "
        );

        let mut values = with_scopes(scopes);
        values.push(Value::Text(escaped_query));
        values.push(Value::Integer((limit as i64) + 1));
        values.push(Value::Integer(offset as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let mut items = stmt
            .query_map(params_from_iter(values), row_from_stmt)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = items.len() > limit;
        if has_more {
            items.pop();
        }
        Ok((items, has_more))
    }

    pub fn soft_delete_memory(&mut self, memory_id: &str) -> Result<bool> {
        let changes = self.conn.execute(
            "UPDATE memories SET status = 'deleted', updated_at = ? WHERE id = ? AND status = 'active'",
            params![now_iso(), memory_id],
        )?;
        if changes == 0 {
            return Ok(false);
        }

        self.remove_fts_entry(memory_id);
        self.add_event(memory_id, "deleted", None);
        Ok(true)
    }

    pub fn set_pinned(&mut self, memory_id: &str, pinned: bool) -> Result<bool> {
        let changes = self.conn.execute(
            "UPDATE memories SET pinned = ?, updated_at = ? WHERE id = ? AND status = 'active'",
            params![i64::from(u8::from(pinned)), now_iso(), memory_id],
        )?;
        if changes > 0 {
            self.add_event(memory_id, if pinned { "pinned" } else { "unpinned" }, None);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_injection_candidates(
        &self,
        project_scope: &str,
        limit: usize,
    ) -> Result<Vec<MemoryRow>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at
            FROM memories
            WHERE status = 'active' AND scope IN (?, 'global')
            ORDER BY CASE
              WHEN scope = ? AND pinned = 1 THEN 0
              WHEN scope = 'global' AND pinned = 1 THEN 1
              WHEN scope = ? THEN 2
              ELSE 3
            END,
            updated_at DESC
            LIMIT ?
            ",
        )?;

        let items = stmt
            .query_map(
                params![project_scope, project_scope, project_scope, limit as i64],
                row_from_stmt,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    pub fn get_stats(&self, scopes: &[String]) -> Result<MemoryStats> {
        if scopes.is_empty() {
            return Ok(MemoryStats {
                active: 0,
                pinned: 0,
                global: 0,
                project: 0,
                has_fts: self.has_fts,
            });
        }

        let scope_clause = scopes_in_clause(scopes);

        let active_sql = format!(
            "SELECT COUNT(*) FROM memories WHERE status = 'active' AND scope IN {scope_clause}"
        );
        let pinned_sql = format!(
            "SELECT COUNT(*) FROM memories WHERE status = 'active' AND pinned = 1 AND scope IN {scope_clause}"
        );
        let by_scope_sql = format!(
            "SELECT scope, COUNT(*) FROM memories WHERE status = 'active' AND scope IN {scope_clause} GROUP BY scope"
        );

        let active: i64 =
            self.conn
                .query_row(&active_sql, params_from_iter(with_scopes(scopes)), |row| {
                    row.get(0)
                })?;
        let pinned: i64 =
            self.conn
                .query_row(&pinned_sql, params_from_iter(with_scopes(scopes)), |row| {
                    row.get(0)
                })?;

        let mut global = 0_i64;
        let mut project = 0_i64;

        let mut stmt = self.conn.prepare(&by_scope_sql)?;
        let rows = stmt.query_map(params_from_iter(with_scopes(scopes)), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows {
            let (scope, count) = row?;
            if scope == "global" {
                global += count;
            } else {
                project += count;
            }
        }

        Ok(MemoryStats {
            active,
            pinned,
            global,
            project,
            has_fts: self.has_fts,
        })
    }

    pub fn export_active_memories(&self, scopes: Option<&[String]>) -> Result<Vec<MemoryRow>> {
        match scopes {
            Some([]) => Ok(Vec::new()),
            Some(scope_values) => {
                let scope_clause = scopes_in_clause(scope_values);
                let sql = format!(
                    "
                    SELECT id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at
                    FROM memories
                    WHERE status = 'active' AND scope IN {scope_clause}
                    ORDER BY scope, pinned DESC, updated_at DESC
                    "
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(params_from_iter(with_scopes(scope_values)), row_from_stmt)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "
                    SELECT id, scope, category, content, content_hash, status, pinned, source, created_at, updated_at
                    FROM memories
                    WHERE status = 'active'
                    ORDER BY scope, pinned DESC, updated_at DESC
                    ",
                )?;
                let rows = stmt
                    .query_map([], row_from_stmt)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
        }
    }

    pub fn prune_old_events(&mut self, retention_days: u64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        let changes = self.conn.execute(
            "DELETE FROM memory_events WHERE timestamp < ?",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(changes)
    }

    pub fn record_compaction(
        &mut self,
        scope: &str,
        mode: CompactionMode,
        input_chars: usize,
        output_chars: usize,
        source_count: usize,
        model: Option<&str>,
        reason: Option<&str>,
        details: serde_json::Value,
    ) {
        let _ = self.conn.execute(
            "
            INSERT INTO memory_compactions
            (scope, mode, input_chars, output_chars, source_count, model, reason, details, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                scope,
                serde_json::to_string(&mode).unwrap_or_else(|_| "\"none\"".to_string()),
                input_chars as i64,
                output_chars as i64,
                source_count as i64,
                model,
                reason,
                serde_json::to_string(&details).ok(),
                now_iso(),
            ],
        );
    }

    pub fn refresh(&mut self, config: &MemoryConfig) -> Result<()> {
        if self.has_fts {
            self.ensure_fts_synced()?;
        }
        self.prune_old_events(config.retention.event_days)?;
        let _ = self.conn.pragma_update(None, "optimize", true);
        Ok(())
    }
}
