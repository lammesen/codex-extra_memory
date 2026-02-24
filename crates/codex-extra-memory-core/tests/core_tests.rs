use codex_extra_memory_core::MemoryService;
use codex_extra_memory_core::commands::{ExportFormat, parse_add_args, parse_export_args};
use codex_extra_memory_core::types::ScopeTarget;
use serde_json::{Value, json};
use std::fs;

fn data(value: &Value) -> &Value {
    value.get("data").expect("response data field")
}

#[test]
fn parse_add_and_export_args_match_pi_behavior() {
    let parsed = parse_add_args("--global --category preference Use pnpm").expect("parse add");
    assert!(matches!(parsed.scope_target, ScopeTarget::Global));
    assert_eq!(parsed.category.as_str(), "preference");
    assert_eq!(parsed.text, "Use pnpm");

    let export = parse_export_args("--all md ./exports/memory.md");
    assert!(export.include_all_scopes);
    assert_eq!(export.format, ExportFormat::Markdown);
    assert_eq!(export.output_path_raw, "./exports/memory.md");

    let sentinel = parse_export_args("-- --all md");
    assert!(!sentinel.include_all_scopes);
    assert_eq!(sentinel.format, ExportFormat::Json);
    assert_eq!(sentinel.output_path_raw, "--all md");
}

#[test]
fn add_dedupe_and_search_work() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory_dir = temp.path().join("memory");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");

    let mut service = MemoryService::new_with_memory_dir(&memory_dir).expect("service");

    let first = service
        .add_memory(
            "User prefers pnpm".to_string(),
            Some(ScopeTarget::Project),
            None,
            &workspace,
            "test",
        )
        .expect("add first");
    assert_eq!(
        data(&first).get("result").and_then(Value::as_str),
        Some("added")
    );

    let dup = service
        .add_memory(
            "  user   prefers   pnpm  ".to_string(),
            Some(ScopeTarget::Project),
            None,
            &workspace,
            "test",
        )
        .expect("add duplicate");
    assert_eq!(
        data(&dup).get("result").and_then(Value::as_str),
        Some("deduped")
    );

    let search = service
        .search_memories(&workspace, "pnpm".to_string(), Some(10), None)
        .expect("search");
    let items = data(&search)
        .get("page")
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
        .expect("search items");
    assert_eq!(items.len(), 1);
}

#[test]
fn sync_agents_inserts_and_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory_dir = temp.path().join("memory");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");

    let agents_path = workspace.join("AGENTS.md");
    fs::write(&agents_path, "# Team Instructions\n\nDo not edit intro.\n").expect("write agents");

    let mut service = MemoryService::new_with_memory_dir(&memory_dir).expect("service");
    service
        .add_memory(
            "Always run unit tests before final answer".to_string(),
            Some(ScopeTarget::Project),
            None,
            &workspace,
            "test",
        )
        .expect("add memory");

    let sync1 = service.sync_agents(&workspace).expect("sync #1");
    let changed_1 = data(&sync1)
        .get("changed")
        .and_then(Value::as_bool)
        .expect("changed value");
    assert!(changed_1);

    let synced_text = fs::read_to_string(&agents_path).expect("read synced agents");
    assert!(synced_text.contains("codex-extra-memory:start v1"));
    assert!(synced_text.contains("Do not edit intro."));

    let sync2 = service.sync_agents(&workspace).expect("sync #2");
    let changed_2 = data(&sync2)
        .get("changed")
        .and_then(Value::as_bool)
        .expect("changed value");
    assert!(!changed_2);
}

#[test]
fn capture_candidates_persists_when_enabled() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory_dir = temp.path().join("memory");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");

    let mut service = MemoryService::new_with_memory_dir(&memory_dir).expect("service");

    let event = json!({
        "messages": [
            {"role": "user", "content": "please remember that I prefer rust over typescript"},
            {"role": "assistant", "content": "Memory: use concise bullet points"}
        ]
    });

    let result = service
        .capture_candidates(&workspace, event, true)
        .expect("capture candidates");

    assert_eq!(data(&result).get("added").and_then(Value::as_u64), Some(2));

    let listed = service
        .list_memories(&workspace, Some(20), None)
        .expect("list memories");
    let item_count = data(&listed)
        .get("page")
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
        .expect("items array")
        .len();
    assert!(item_count >= 2);
}

#[test]
fn export_memories_accepts_default_and_relative_workspace_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory_dir = temp.path().join("memory");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");

    let service = MemoryService::new_with_memory_dir(&memory_dir).expect("service");

    let default_export = service
        .export_memories(&workspace, ExportFormat::Json, false, String::new())
        .expect("default export");
    assert_eq!(
        default_export.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    let default_path = data(&default_export)
        .get("path")
        .and_then(Value::as_str)
        .expect("default path");
    let canonical_workspace = workspace.canonicalize().expect("canonical workspace");
    let canonical_default_path = std::path::Path::new(default_path)
        .canonicalize()
        .expect("canonical default path");
    assert!(canonical_default_path.starts_with(&canonical_workspace));

    let relative_export = service
        .export_memories(
            &workspace,
            ExportFormat::Markdown,
            false,
            "./exports/memory.md".to_string(),
        )
        .expect("relative export");
    assert_eq!(
        relative_export.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(workspace.join("exports").join("memory.md").exists());
}

#[test]
fn export_memories_rejects_paths_outside_workspace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory_dir = temp.path().join("memory");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");

    let service = MemoryService::new_with_memory_dir(&memory_dir).expect("service");

    let escaped = service
        .export_memories(
            &workspace,
            ExportFormat::Json,
            false,
            "../outside.json".to_string(),
        )
        .expect("escaped export");
    assert_eq!(escaped.get("ok").and_then(Value::as_bool), Some(false));

    let absolute_path = temp.path().join("absolute-outside.json");
    let absolute = service
        .export_memories(
            &workspace,
            ExportFormat::Json,
            false,
            absolute_path.to_string_lossy().to_string(),
        )
        .expect("absolute export");
    assert_eq!(absolute.get("ok").and_then(Value::as_bool), Some(false));
    assert!(!absolute_path.exists());
}
