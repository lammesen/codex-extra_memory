use anyhow::{Result, anyhow};
use clap::Parser;
use codex_extra_memory_core::commands::{AutoMode, ExportFormat};
use codex_extra_memory_core::service::MemoryService;
use mcpkit::prelude::*;
use mcpkit::transport::stdio::StdioTransport;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Parser)]
#[command(name = "codex-extra-memory-mcp")]
#[command(about = "Codex extra memory MCP server")]
struct Cli {
    #[arg(long)]
    workspace: Option<PathBuf>,
}

struct App {
    service: Mutex<MemoryService>,
    workspace: PathBuf,
}

#[derive(Clone)]
struct CodexExtraMemoryMcp {
    app: Arc<App>,
}

fn canonicalize_for_containment(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| anyhow!("canonicalize {}: {error}", path.display()));
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("path '{}' has no final segment", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path '{}' has no parent", path.display()))?;
    Ok(canonicalize_for_containment(parent)?.join(file_name))
}

fn resolve_workspace(base: &Path, cwd: Option<String>) -> Result<PathBuf> {
    let base_canonical = canonicalize_for_containment(base)?;
    let candidate = match cwd {
        Some(raw) => {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                path
            } else {
                base.join(path)
            }
        }
        None => base.to_path_buf(),
    };
    let candidate_canonical = canonicalize_for_containment(&candidate)?;

    if candidate_canonical == base_canonical || candidate_canonical.starts_with(&base_canonical) {
        Ok(candidate_canonical)
    } else {
        Err(anyhow!(
            "cwd '{}' resolves outside workspace '{}'",
            candidate.display(),
            base.display()
        ))
    }
}

async fn with_service_blocking<F>(app: Arc<App>, f: F) -> Result<Value>
where
    F: FnOnce(&mut MemoryService) -> Result<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut guard = app
            .service
            .lock()
            .map_err(|_| anyhow!("memory service mutex poisoned"))?;
        f(&mut guard)
    })
    .await
    .map_err(|error| anyhow!("memory task join failure: {error}"))?
}

fn to_tool_output(result: Result<Value>) -> ToolOutput {
    match result {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(text) => ToolOutput::text(text),
            Err(error) => ToolOutput::error(format!("serialization error: {error}")),
        },
        Err(error) => ToolOutput::error(error.to_string()),
    }
}

fn wrap_memory_command_result(data: Value, session_id: String) -> Value {
    let ok = data.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let error = if ok {
        None
    } else {
        data.get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let mut payload = json!({
        "ok": ok,
        "action": "memory_command",
        "session_id": session_id,
        "result": data,
    });
    if let Some(error) = error {
        payload["error"] = json!(error);
    }
    payload
}

#[mcp_server(name = "codex_extra_memory", version = "0.1.0")]
impl CodexExtraMemoryMcp {
    #[tool(description = "Execute memory commands with Pi-compatible command parsing")]
    async fn memory_command(
        &self,
        input: String,
        cwd: Option<String>,
        session_id: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        let result = with_service_blocking(app, move |service| {
            service.execute_command(&input, &workspace)
        })
        .await;
        to_tool_output(result.map(|data| {
            if let Some(session_id) = session_id {
                wrap_memory_command_result(data, session_id)
            } else {
                data
            }
        }))
    }

    #[tool(description = "Add a memory entry")]
    async fn memory_add(
        &self,
        fact: String,
        scope: Option<String>,
        category: Option<String>,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.memory_add_typed(&workspace, fact, scope, category)
            })
            .await,
        )
    }

    #[tool(description = "List memories with cursor pagination")]
    async fn memory_list(
        &self,
        limit: Option<usize>,
        cursor: Option<String>,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.list_memories(&workspace, limit, cursor)
            })
            .await,
        )
    }

    #[tool(description = "Search memories with cursor pagination")]
    async fn memory_search(
        &self,
        query: String,
        limit: Option<usize>,
        cursor: Option<String>,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.search_memories(&workspace, query, limit, cursor)
            })
            .await,
        )
    }

    #[tool(description = "Delete a memory by ID or prefix")]
    async fn memory_delete(&self, id_or_prefix: String, cwd: Option<String>) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.delete_memory(&workspace, id_or_prefix)
            })
            .await,
        )
    }

    #[tool(description = "Pin or unpin a memory")]
    async fn memory_pin(
        &self,
        id_or_prefix: String,
        enabled: bool,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.pin_memory(&workspace, id_or_prefix, enabled)
            })
            .await,
        )
    }

    #[tool(description = "Auto-capture mode (on/off/status)")]
    async fn memory_auto(&self, mode: String) -> ToolOutput {
        let parsed = match mode.to_lowercase().as_str() {
            "on" => AutoMode::On,
            "off" => AutoMode::Off,
            "status" | "" => AutoMode::Status,
            _ => {
                return ToolOutput::error("mode must be one of: on, off, status");
            }
        };

        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| service.auto_capture_mode(parsed)).await,
        )
    }

    #[tool(description = "Get memory stats")]
    async fn memory_stats(&self, cwd: Option<String>) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(with_service_blocking(app, move |service| service.stats(&workspace)).await)
    }

    #[tool(description = "Export memories to json or markdown")]
    async fn memory_export(
        &self,
        format: Option<String>,
        include_all_scopes: Option<bool>,
        output_path: Option<String>,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let format = match format.unwrap_or_else(|| "json".to_string()).as_str() {
            "json" => ExportFormat::Json,
            "md" | "markdown" => ExportFormat::Markdown,
            _ => return ToolOutput::error("format must be 'json' or 'md'"),
        };

        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.export_memories(
                    &workspace,
                    format,
                    include_all_scopes.unwrap_or(false),
                    output_path.unwrap_or_default(),
                )
            })
            .await,
        )
    }

    #[tool(description = "Refresh runtime store and prune old events")]
    async fn memory_refresh(&self) -> ToolOutput {
        let app = Arc::clone(&self.app);
        to_tool_output(with_service_blocking(app, MemoryService::refresh).await)
    }

    #[tool(description = "Sync managed memory block into workspace AGENTS.md")]
    async fn memory_sync_agents(&self, cwd: Option<String>) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| service.sync_agents(&workspace)).await,
        )
    }

    #[tool(
        description = "Extract auto-capture candidates from event payload and optionally persist"
    )]
    async fn memory_capture_candidates(
        &self,
        event_payload: Value,
        persist: Option<bool>,
        cwd: Option<String>,
    ) -> ToolOutput {
        let workspace = match resolve_workspace(&self.app.workspace, cwd) {
            Ok(workspace) => workspace,
            Err(error) => return ToolOutput::error(error.to_string()),
        };
        let app = Arc::clone(&self.app);
        to_tool_output(
            with_service_blocking(app, move |service| {
                service.capture_candidates(&workspace, event_payload, persist.unwrap_or(true))
            })
            .await,
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), McpError> {
    let cli = Cli::parse();
    let workspace = cli.workspace.unwrap_or_else(|| {
        std::env::current_dir().expect("resolve current directory for default workspace")
    });

    let app = App {
        service: Mutex::new(
            MemoryService::new().map_err(|error| McpError::internal(error.to_string()))?,
        ),
        workspace,
    };

    let service = CodexExtraMemoryMcp { app: Arc::new(app) };
    let transport = StdioTransport::new();

    let server = ServerBuilder::new(service.clone())
        .with_tools(service)
        .build();

    server.serve(transport).await
}

#[cfg(test)]
mod tests {
    use super::{App, resolve_workspace, with_service_blocking, wrap_memory_command_result};
    use codex_extra_memory_core::service::MemoryService;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    #[test]
    fn resolve_workspace_allows_absolute_inside_base() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let inside = workspace.join("nested");
        std::fs::create_dir_all(&inside).expect("create workspace dirs");

        let resolved = resolve_workspace(&workspace, Some(inside.to_string_lossy().to_string()))
            .expect("resolve inside");
        assert!(resolved.starts_with(workspace.canonicalize().expect("canonicalize workspace")));
    }

    #[test]
    fn resolve_workspace_rejects_relative_escape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&outside).expect("create outside");

        let result = resolve_workspace(&workspace, Some("../outside".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_workspace_rejects_absolute_outside_base() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&outside).expect("create outside");

        let result = resolve_workspace(&workspace, Some(outside.to_string_lossy().to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn wraps_session_payload_with_inner_failure_status() {
        let wrapped = wrap_memory_command_result(
            json!({"ok": false, "action": "search", "error": "boom"}),
            "session-1".to_string(),
        );
        assert_eq!(wrapped.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(wrapped.get("error").and_then(|v| v.as_str()), Some("boom"));
    }

    #[tokio::test]
    async fn with_service_blocking_executes_memory_service_call() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let memory_dir = temp.path().join("memory");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&memory_dir).expect("create memory dir");

        let service = MemoryService::new_with_memory_dir(&memory_dir).expect("create service");
        let app = Arc::new(App {
            service: Mutex::new(service),
            workspace: workspace.clone(),
        });

        let output = with_service_blocking(Arc::clone(&app), move |service| {
            service.execute_command("/memory help", &workspace)
        })
        .await
        .expect("execute service call");

        assert_eq!(output.get("ok").and_then(Value::as_bool), Some(true));
    }
}
