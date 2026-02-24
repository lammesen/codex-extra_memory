use std::path::PathBuf;

#[must_use]
pub fn resolve_codex_home() -> PathBuf {
    if let Ok(value) = std::env::var("CODEX_HOME")
        && !value.trim().is_empty()
    {
        return PathBuf::from(value);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

#[must_use]
pub fn get_memory_dir() -> PathBuf {
    resolve_codex_home().join("memory")
}

#[must_use]
pub fn get_database_path() -> PathBuf {
    get_memory_dir().join("memory.sqlite")
}

#[must_use]
pub fn get_config_path() -> PathBuf {
    get_memory_dir().join("config.json")
}
