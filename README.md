# codex-extra-memory

Rust codex-native memory extension with MCP + CLI, central SQLite persistence, and managed `AGENTS.md` sync.

## What it provides

- `memory_command` tool with Pi-style parsing (`/memory ...` and `memory ...`).
- Typed MCP tools:
  - `memory_add`
  - `memory_list`
  - `memory_search`
  - `memory_delete`
  - `memory_pin`
  - `memory_auto`
  - `memory_stats`
  - `memory_export`
  - `memory_refresh`
  - `memory_sync_agents`
  - `memory_capture_candidates`
- CLI binary: `codex-memory`
- Storage at `$CODEX_HOME/memory/`:
  - `memory.sqlite`
  - `config.json`
- AGENTS sync with managed markers:
  - `<!-- codex-extra-memory:start v1 -->`
  - `<!-- codex-extra-memory:end -->`
- LLM compaction via OpenAI Responses API (`OPENAI_API_KEY`) with deterministic fallback.

## Workspace layout

- `crates/codex-extra-memory-core`: DB, parser, sync, capture, compaction logic.
- `crates/codex-extra-memory-mcp`: MCP stdio server (`mcpkit`).
- `crates/codex-extra-memory-cli`: local CLI.
- `crates/codex-extra-memory-installer`: Codex config installer/uninstaller.

## Build and test

```bash
cargo test
cargo build --release
```

## Install from crates.io (CI publishes on every push to `main`)

Each push to `main` publishes a unique prerelease version in this format:

```text
<base-version>-ci.<github-run-number>.<short-sha>
```

Install a published version with Cargo:

```bash
cargo install codex-memory --version <published-version>
cargo install codex-extra-memory-mcp --version <published-version>
```

Use an explicit `--version` because CI publishes prerelease versions.

## Install into Codex config

```bash
./install/install.sh --workspace /absolute/path/to/workspace
```

Installer writes an MCP server entry under:

```toml
[mcp_servers.codex_extra_memory]
command = "codex-extra-memory-mcp"
args = ["--workspace", "/absolute/path/to/workspace"]
required = true
enabled = true
startup_timeout_sec = 20
tool_timeout_sec = 90
enabled_tools = [
  "memory_command",
  "memory_add",
  "memory_list",
  "memory_search",
  "memory_delete",
  "memory_pin",
  "memory_auto",
  "memory_stats",
  "memory_export",
  "memory_refresh",
  "memory_sync_agents",
  "memory_capture_candidates",
]
```

Restart Codex after install.

## CLI examples

```bash
codex-memory memory add --category preference Use pnpm
codex-memory /memory list --limit 10
codex-memory memory search "typescript strict" --limit 5
codex-memory memory sync
```

## AGENTS sync semantics

`memory_sync_agents` updates workspace `AGENTS.md`. Per Codex behavior, AGENTS changes are applied on the next run/session start.

## Workspace safety semantics

- MCP `cwd` values are constrained to the configured workspace root.
- `memory_export` output paths must be relative to the workspace root.
- Invalid `config.json` files are backed up as `config.invalid-<timestamp>.json.bak` before defaults are regenerated.

## OpenAI API for LLM compaction

Set:

```bash
export OPENAI_API_KEY=...
```

If unavailable/failing, sync falls back to deterministic compaction.
