use crate::types::{MemoryCategory, ScopeTarget};
use crate::utils::split_first_token;

#[derive(Debug, Clone)]
pub struct AddArgs {
    pub scope_target: ScopeTarget,
    pub category: MemoryCategory,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ExportArgs {
    pub format: ExportFormat,
    pub include_all_scopes: bool,
    pub output_path_raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Markdown,
}

impl ExportFormat {
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }
}

#[derive(Debug, Clone)]
pub enum MemoryCommand {
    Help,
    Refresh,
    Sync,
    Add(AddArgs),
    Show,
    List {
        limit: Option<usize>,
        cursor: Option<String>,
    },
    Search {
        query: String,
        limit: Option<usize>,
        cursor: Option<String>,
    },
    Delete {
        id_or_prefix: String,
    },
    Pin {
        id_or_prefix: String,
        enabled: bool,
    },
    Auto {
        mode: AutoMode,
    },
    Stats,
    Export(ExportArgs),
}

#[derive(Debug, Clone, Copy)]
pub enum AutoMode {
    On,
    Off,
    Status,
}

pub const COMMAND_HELP: &str = r"Persistent memory commands:

/memory add [--global|--project] [--category <category>] <text>
/memory show
/memory list [--limit <n>] [--cursor <token>]
/memory search <query> [--limit <n>] [--cursor <token>]
/memory delete <id-or-prefix>
/memory pin <id-or-prefix> on|off
/memory auto [on|off|status]
/memory stats
/memory export [--all] [json|md] [path]
/memory refresh
/memory sync
/memory help
";

pub fn parse_add_args(raw: &str) -> Result<AddArgs, String> {
    let mut scope_target = ScopeTarget::Project;
    let mut category = MemoryCategory::Other;
    let mut remaining = raw.trim();

    loop {
        let (token, rest) = split_first_token(remaining);
        if !token.starts_with("--") {
            break;
        }

        match token {
            "--global" => {
                scope_target = ScopeTarget::Global;
                remaining = rest;
            }
            "--project" => {
                scope_target = ScopeTarget::Project;
                remaining = rest;
            }
            "--category" => {
                let (category_token, category_rest) = split_first_token(rest);
                if category_token.is_empty() {
                    return Err("Missing value for --category.".to_string());
                }
                category = category_token.parse::<MemoryCategory>()?;
                remaining = category_rest;
            }
            unknown => {
                return Err(format!("Unknown option '{unknown}'."));
            }
        }
    }

    let text = remaining.trim().to_string();
    if text.is_empty() {
        return Err(
            "Usage: /memory add [--global|--project] [--category <category>] <text>".to_string(),
        );
    }

    Ok(AddArgs {
        scope_target,
        category,
        text,
    })
}

#[must_use]
pub fn parse_export_args(raw: &str) -> ExportArgs {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut format = ExportFormat::Json;
    let mut include_all_scopes = false;
    let mut format_set = false;
    let mut output_path_raw = String::new();

    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if token == "--" {
            output_path_raw = tokens[index + 1..].join(" ").trim().to_string();
            break;
        }

        if token == "--all" {
            include_all_scopes = true;
            index += 1;
            continue;
        }

        if !format_set {
            match token {
                "json" => {
                    format = ExportFormat::Json;
                    format_set = true;
                    index += 1;
                    continue;
                }
                "md" => {
                    format = ExportFormat::Markdown;
                    format_set = true;
                    index += 1;
                    continue;
                }
                _ => {}
            }
        }

        output_path_raw = tokens[index..].join(" ").trim().to_string();
        break;
    }

    ExportArgs {
        format,
        include_all_scopes,
        output_path_raw,
    }
}

fn parse_limit_cursor(tokens: &[&str]) -> Result<(Option<usize>, Option<String>), String> {
    let mut limit = None;
    let mut cursor = None;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index] {
            "--limit" => {
                let value = tokens
                    .get(index + 1)
                    .ok_or_else(|| "Missing value for --limit".to_string())?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| "--limit must be a positive integer".to_string())?;
                if parsed == 0 {
                    return Err("--limit must be > 0".to_string());
                }
                limit = Some(parsed);
                index += 2;
            }
            "--cursor" => {
                let value = tokens
                    .get(index + 1)
                    .ok_or_else(|| "Missing value for --cursor".to_string())?;
                cursor = Some((*value).to_string());
                index += 2;
            }
            unexpected => {
                return Err(format!("Unknown option '{unexpected}'."));
            }
        }
    }

    Ok((limit, cursor))
}

pub fn parse_memory_command(raw_input: &str) -> Result<MemoryCommand, String> {
    let mut trimmed = raw_input.trim();

    if let Some(rest) = trimmed.strip_prefix("/memory") {
        trimmed = rest.trim();
    } else if let Some(rest) = trimmed.strip_prefix("memory") {
        trimmed = rest.trim();
    }

    if trimmed.is_empty() {
        return Ok(MemoryCommand::Help);
    }

    let (subcommand_raw, rest) = split_first_token(trimmed);
    let subcommand = subcommand_raw.to_lowercase();

    match subcommand.as_str() {
        "help" => Ok(MemoryCommand::Help),
        "refresh" => Ok(MemoryCommand::Refresh),
        "sync" => Ok(MemoryCommand::Sync),
        "add" => Ok(MemoryCommand::Add(parse_add_args(rest)?)),
        "show" => Ok(MemoryCommand::Show),
        "list" => {
            let tokens = rest.split_whitespace().collect::<Vec<_>>();
            let (limit, cursor) = parse_limit_cursor(&tokens)?;
            Ok(MemoryCommand::List { limit, cursor })
        }
        "search" => {
            let tokens = rest.split_whitespace().collect::<Vec<_>>();
            if tokens.is_empty() {
                return Err(
                    "Usage: /memory search <query> [--limit <n>] [--cursor <token>]".to_string(),
                );
            }
            let mut query_tokens = Vec::new();
            let mut option_start = tokens.len();
            for (index, token) in tokens.iter().enumerate() {
                if token.starts_with("--") {
                    option_start = index;
                    break;
                }
                query_tokens.push(*token);
            }
            if query_tokens.is_empty() {
                return Err(
                    "Usage: /memory search <query> [--limit <n>] [--cursor <token>]".to_string(),
                );
            }
            let query = query_tokens.join(" ");
            let (limit, cursor) = parse_limit_cursor(&tokens[option_start..])?;
            Ok(MemoryCommand::Search {
                query,
                limit,
                cursor,
            })
        }
        "delete" => {
            let id_or_prefix = rest.trim().to_string();
            if id_or_prefix.is_empty() {
                return Err("Usage: /memory delete <id-or-prefix>".to_string());
            }
            Ok(MemoryCommand::Delete { id_or_prefix })
        }
        "pin" => {
            let (id_or_prefix, state) = split_first_token(rest);
            if id_or_prefix.is_empty() || state.is_empty() {
                return Err("Usage: /memory pin <id-or-prefix> on|off".to_string());
            }
            let enabled = match state.to_lowercase().as_str() {
                "on" => true,
                "off" => false,
                _ => return Err("Usage: /memory pin <id-or-prefix> on|off".to_string()),
            };
            Ok(MemoryCommand::Pin {
                id_or_prefix: id_or_prefix.to_string(),
                enabled,
            })
        }
        "auto" => {
            let mode = match rest.trim().to_lowercase().as_str() {
                "" | "status" => AutoMode::Status,
                "on" => AutoMode::On,
                "off" => AutoMode::Off,
                _ => return Err("Usage: /memory auto [on|off|status]".to_string()),
            };
            Ok(MemoryCommand::Auto { mode })
        }
        "stats" => Ok(MemoryCommand::Stats),
        "export" => Ok(MemoryCommand::Export(parse_export_args(rest))),
        _ => Err(format!("Unknown subcommand: {subcommand}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AutoMode, ExportFormat, MemoryCommand, parse_add_args, parse_export_args,
        parse_memory_command,
    };

    #[test]
    fn parse_add_category() {
        let parsed = parse_add_args("--global --category preference Use pnpm").expect("add args");
        assert_eq!(parsed.scope_target.as_str(), "global");
        assert_eq!(parsed.category.as_str(), "preference");
        assert_eq!(parsed.text, "Use pnpm");
    }

    #[test]
    fn parse_export_defaults() {
        let parsed = parse_export_args("");
        assert_eq!(parsed.format, ExportFormat::Json);
        assert!(!parsed.include_all_scopes);
        assert_eq!(parsed.output_path_raw, "");
    }

    #[test]
    fn parse_memory_auto() {
        let command = parse_memory_command("/memory auto status").expect("parse command");
        assert!(matches!(
            command,
            MemoryCommand::Auto {
                mode: AutoMode::Status
            }
        ));
    }
}
