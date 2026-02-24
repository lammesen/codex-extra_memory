use crate::types::ScopeInfo;
use crate::utils::sha256;
use std::path::{Path, PathBuf};
use std::process::Command;

fn normalize_git_remote_identifier(remote: &str) -> String {
    let trimmed = remote.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    if let Some((host, repo)) = trimmed
        .strip_prefix("git@")
        .and_then(|rest| rest.split_once(':'))
    {
        let normalized_repo = repo.trim_start_matches('/').trim_end_matches(".git");
        return format!("https://{}/{normalized_repo}", host.to_lowercase());
    }

    if let Ok(url) = url::Url::parse(trimmed) {
        let host = url.host_str().unwrap_or_default().to_lowercase();
        let repo = url.path().trim_start_matches('/').trim_end_matches(".git");
        if repo.is_empty() {
            return format!("https://{host}");
        }
        return format!("https://{host}/{repo}");
    }

    trimmed.trim_end_matches(".git").to_string()
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[must_use]
pub fn detect_project_scope(workspace_dir: &Path) -> ScopeInfo {
    let cwd = workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| workspace_dir.to_path_buf());

    let mut root = cwd.clone();
    let mut kind = "path".to_string();
    let mut identifier = cwd.to_string_lossy().to_string();

    if let Some(git_root) = git_stdout(&cwd, &["rev-parse", "--show-toplevel"]) {
        let root_path = PathBuf::from(git_root);
        root.clone_from(&root_path);
        identifier = root_path.to_string_lossy().to_string();

        if let Some(remote) = git_stdout(&root, &["config", "--get", "remote.origin.url"]) {
            kind = "git".to_string();
            identifier = normalize_git_remote_identifier(&remote);
        }
    }

    let scope_hash = sha256(&format!("{kind}:{identifier}"));

    ScopeInfo {
        scope: format!("project:{scope_hash}"),
        kind,
        identifier,
        root: root.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_git_remote_identifier;

    #[test]
    fn normalizes_git_remote_variants() {
        assert_eq!(
            normalize_git_remote_identifier("git@github.com:OpenAI/codex.git"),
            "https://github.com/OpenAI/codex"
        );
        assert_eq!(
            normalize_git_remote_identifier("https://github.com/OpenAI/codex.git"),
            "https://github.com/OpenAI/codex"
        );
    }
}
