use anyhow::Result;
use clap::Parser;
use codex_extra_memory_core::MemoryService;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "codex-memory")]
#[command(about = "Codex extra memory CLI")]
struct Cli {
    #[arg(long)]
    cwd: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    human: bool,

    #[arg(trailing_var_arg = true)]
    input: Vec<String>,
}

fn render_human(value: &Value) -> String {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return format!("Error: {error}");
    }

    if let Some(text) = value
        .get("data")
        .and_then(|data| data.get("rendered"))
        .and_then(Value::as_str)
    {
        return text.to_string();
    }

    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = cli
        .cwd
        .unwrap_or_else(|| std::env::current_dir().expect("resolve cwd"));

    let command_input = if cli.input.is_empty() {
        "/memory help".to_string()
    } else {
        let joined = cli.input.join(" ").trim().to_string();
        if joined.starts_with("/memory") || joined.starts_with("memory") {
            joined
        } else {
            format!("memory {joined}")
        }
    };

    let mut service = MemoryService::new()?;
    let output = service.execute_command(&command_input, &cwd)?;

    if cli.human {
        println!("{}", render_human(&output));
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    if output.get("ok").and_then(Value::as_bool) == Some(false) {
        std::process::exit(1);
    }

    Ok(())
}
