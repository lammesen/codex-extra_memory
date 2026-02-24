use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{Array, DocumentMut, Item, Table, value};

const MIN_CODEX_VERSION: &str = "0.104.0";
const MANAGED_SERVER_NAME: &str = "codex_extra_memory";
const MANAGED_ROOT_DIR: &str = "codex-extra-memory";

#[derive(Debug, Parser)]
#[command(name = "codex-extra-memory-installer")]
#[command(about = "Install/uninstall codex-extra-memory MCP integration")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install {
        #[arg(long)]
        workspace: Option<PathBuf>,
        #[arg(long)]
        config_path: Option<PathBuf>,
        #[arg(long, default_value = "codex-extra-memory-mcp")]
        mcp_command: String,
        #[arg(long, default_value_t = 20)]
        startup_timeout_sec: u64,
        #[arg(long, default_value_t = 90)]
        tool_timeout_sec: u64,
    },
    Uninstall {
        #[arg(long)]
        config_path: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
    Check,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedManifest {
    schema_version: u32,
    installed_at_unix_ms: u128,
    codex_home: String,
    config_path: String,
    managed_mcp_server: String,
    metadata: BTreeMap<String, String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install {
            workspace,
            config_path,
            mcp_command,
            startup_timeout_sec,
            tool_timeout_sec,
        } => install(
            workspace,
            config_path,
            &mcp_command,
            startup_timeout_sec,
            tool_timeout_sec,
        ),
        Commands::Uninstall { config_path, yes } => uninstall(config_path, yes),
        Commands::Check => check(),
    }
}

fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn check() -> Result<()> {
    let codex_version = read_codex_version()?;
    let minimum = Version::parse(MIN_CODEX_VERSION)?;
    if codex_version < minimum {
        return Err(anyhow!(
            "codex version {codex_version} is below minimum {MIN_CODEX_VERSION}"
        ));
    }

    let codex_home = resolve_codex_home();
    println!("codex version: {codex_version}");
    println!("codex home: {}", codex_home.display());
    println!("check: ok");
    Ok(())
}

fn install(
    workspace: Option<PathBuf>,
    config_path: Option<PathBuf>,
    mcp_command: &str,
    startup_timeout_sec: u64,
    tool_timeout_sec: u64,
) -> Result<()> {
    enforce_min_codex_version()?;

    let codex_home = resolve_codex_home();
    fs::create_dir_all(&codex_home)?;

    let workspace = workspace.unwrap_or(std::env::current_dir()?);
    let config_path = config_path.unwrap_or_else(|| codex_home.join("config.toml"));
    let mut doc = load_or_create_toml(&config_path)?;

    configure_mcp_server(
        &mut doc,
        mcp_command,
        &workspace,
        startup_timeout_sec,
        tool_timeout_sec,
    );

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&config_path, doc.to_string())?;

    let managed_root = codex_home.join(MANAGED_ROOT_DIR);
    fs::create_dir_all(&managed_root)?;
    let manifest = ManagedManifest {
        schema_version: 1,
        installed_at_unix_ms: now_unix_ms(),
        codex_home: codex_home.display().to_string(),
        config_path: config_path.display().to_string(),
        managed_mcp_server: MANAGED_SERVER_NAME.to_string(),
        metadata: BTreeMap::from([
            (
                "minimum_codex_version".to_string(),
                MIN_CODEX_VERSION.to_string(),
            ),
            (
                "workspace_default".to_string(),
                workspace.display().to_string(),
            ),
        ]),
    };
    write_manifest(&managed_root.join("manifest.json"), &manifest)?;

    println!("Installed codex-extra-memory.");
    println!("- Updated config: {}", config_path.display());
    println!("- MCP server key: {MANAGED_SERVER_NAME}");
    println!("Restart Codex to load MCP configuration changes.");

    Ok(())
}

fn uninstall(config_path: Option<PathBuf>, _yes: bool) -> Result<()> {
    let codex_home = resolve_codex_home();
    let config_path = config_path.unwrap_or_else(|| codex_home.join("config.toml"));

    if config_path.exists() {
        let mut doc = load_or_create_toml(&config_path)?;
        remove_managed_config(&mut doc);
        fs::write(&config_path, doc.to_string())?;
    }

    let manifest_path = codex_home.join(MANAGED_ROOT_DIR).join("manifest.json");
    if manifest_path.exists() {
        fs::remove_file(&manifest_path)?;
    }

    println!("Uninstalled codex-extra-memory managed config.");
    Ok(())
}

fn resolve_codex_home() -> PathBuf {
    if let Ok(value) = std::env::var("CODEX_HOME")
        && !value.trim().is_empty()
    {
        return PathBuf::from(value);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn enforce_min_codex_version() -> Result<()> {
    let version = read_codex_version()?;
    let minimum = Version::parse(MIN_CODEX_VERSION)?;
    if version < minimum {
        return Err(anyhow!(
            "codex version {version} is below required minimum {MIN_CODEX_VERSION}"
        ));
    }
    Ok(())
}

fn read_codex_version() -> Result<Version> {
    let output = Command::new("codex")
        .arg("--version")
        .output()
        .context("running `codex --version`")?;

    if !output.status.success() {
        return Err(anyhow!(
            "failed to execute codex --version: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let version_text = stdout
        .split_whitespace()
        .last()
        .ok_or_else(|| anyhow!("could not parse codex version output: {stdout}"))?
        .trim_start_matches("codex-cli");

    let cleaned = stdout
        .split_whitespace()
        .last()
        .ok_or_else(|| anyhow!("could not parse codex version output"))?;
    Version::parse(cleaned)
        .or_else(|_| Version::parse(version_text))
        .map_err(|err| anyhow!("invalid semver from codex output: {err}"))
}

fn load_or_create_toml(path: &Path) -> Result<DocumentMut> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    if raw.trim().is_empty() {
        Ok("".parse::<DocumentMut>()?)
    } else {
        Ok(raw.parse::<DocumentMut>()?)
    }
}

fn configure_mcp_server(
    doc: &mut DocumentMut,
    mcp_command: &str,
    workspace: &Path,
    startup_timeout_sec: u64,
    tool_timeout_sec: u64,
) {
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }

    doc["mcp_servers"][MANAGED_SERVER_NAME]["command"] = value(mcp_command);

    let mut args = Array::new();
    args.push("--workspace");
    args.push(workspace.display().to_string());
    doc["mcp_servers"][MANAGED_SERVER_NAME]["args"] = value(args);

    doc["mcp_servers"][MANAGED_SERVER_NAME]["required"] = value(true);
    doc["mcp_servers"][MANAGED_SERVER_NAME]["enabled"] = value(true);
    doc["mcp_servers"][MANAGED_SERVER_NAME]["cwd"] = value(workspace.display().to_string());
    doc["mcp_servers"][MANAGED_SERVER_NAME]["startup_timeout_sec"] =
        value(startup_timeout_sec as i64);
    doc["mcp_servers"][MANAGED_SERVER_NAME]["tool_timeout_sec"] = value(tool_timeout_sec as i64);

    let mut enabled_tools = Array::new();
    for tool in [
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
    ] {
        enabled_tools.push(tool);
    }
    doc["mcp_servers"][MANAGED_SERVER_NAME]["enabled_tools"] = value(enabled_tools);
}

fn remove_managed_config(doc: &mut DocumentMut) {
    if let Some(item) = doc.get_mut("mcp_servers")
        && let Some(table) = item.as_table_like_mut()
    {
        let _ = table.remove(MANAGED_SERVER_NAME);
    }
}

fn write_manifest(path: &Path, manifest: &ManagedManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(manifest)?),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_managed_server_only() {
        let mut doc = r#"
[mcp_servers]
codex_extra_memory = { command = "codex-extra-memory-mcp" }
custom = { command = "custom-mcp" }
"#
        .parse::<DocumentMut>()
        .expect("parse toml");

        remove_managed_config(&mut doc);
        let text = doc.to_string();
        assert!(!text.contains("codex_extra_memory"));
        assert!(text.contains("custom"));
    }
}
