use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubCopilotAuth {
    #[serde(default)]
    pub api_base_url: Option<String>,
    pub github_access_token: String,
    pub copilot_access_token: String,
    pub copilot_token_expires_at: Option<DateTime<Utc>>,
}

fn get_copilot_auth_file(codex_home: &Path) -> PathBuf {
    codex_home.join("github-copilot-auth.json")
}

pub fn load_github_copilot_auth(codex_home: &Path) -> std::io::Result<Option<GitHubCopilotAuth>> {
    let auth_file = get_copilot_auth_file(codex_home);
    let mut file = match std::fs::File::open(auth_file) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let auth = serde_json::from_str(&contents).map_err(std::io::Error::other)?;
    Ok(Some(auth))
}

pub fn save_github_copilot_auth(
    codex_home: &Path,
    auth: &GitHubCopilotAuth,
) -> std::io::Result<()> {
    let auth_file = get_copilot_auth_file(codex_home);
    if let Some(parent) = auth_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(auth).map_err(std::io::Error::other)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(auth_file)?;
    file.write_all(json.as_bytes())?;
    file.flush()?;
    Ok(())
}

pub fn delete_github_copilot_auth(codex_home: &Path) -> std::io::Result<bool> {
    let auth_file = get_copilot_auth_file(codex_home);
    match std::fs::remove_file(auth_file) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_github_copilot_auth_creates_missing_parent_dir() {
        let tempdir = tempfile::tempdir().expect("temp dir");
        let codex_home = tempdir.path().join("missing").join("copilot-home");
        let auth = GitHubCopilotAuth {
            api_base_url: Some("https://api.business.githubcopilot.com".to_string()),
            github_access_token: "github-token".to_string(),
            copilot_access_token: "copilot-token".to_string(),
            copilot_token_expires_at: None,
        };

        save_github_copilot_auth(&codex_home, &auth).expect("save should create parent dir");

        assert!(codex_home.join("github-copilot-auth.json").exists());
    }
}
