use std::path::PathBuf;

use serde_json::Value;

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, text};

const AUTH_FILE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CopilotAuthStore {
    files: Vec<PathBuf>,
    reader: AtomicFileStore,
}

impl CopilotAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Self {
        Self::new(vec![
            paths.roaming_app_data.join("github-copilot/apps.json"),
            paths.roaming_app_data.join("github-copilot/hosts.json"),
            paths.roaming_app_data.join("GitHub CLI/hosts.yml"),
        ])
    }

    pub fn new(files: Vec<PathBuf>) -> Self {
        Self {
            files,
            reader: AtomicFileStore::with_max_file_bytes(AUTH_FILE_LIMIT),
        }
    }

    pub async fn load(&self) -> Result<SecretBytes, ProviderError> {
        let mut saw_unreadable = false;
        for path in &self.files {
            let contents = match self.reader.read(path).await {
                Ok(contents) => contents,
                Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    continue;
                }
                Err(_) => {
                    saw_unreadable = true;
                    continue;
                }
            };
            let extension = path.extension().and_then(std::ffi::OsStr::to_str);
            let token = if extension == Some("json") {
                editor_token(&contents)
            } else {
                gh_token(&contents)
            };
            if let Some(token) = token {
                return Ok(token);
            }
        }
        if saw_unreadable {
            Err(ProviderError::new(
                ErrorCategory::CredentialAccess,
                "Copilot credentials could not be read.",
            ))
        } else {
            Err(ProviderError::not_logged_in("gh auth login"))
        }
    }

    pub async fn has_usable_credentials(&self) -> bool {
        self.load().await.is_ok()
    }
}

fn editor_token(contents: &[u8]) -> Option<SecretBytes> {
    let root: Value = serde_json::from_slice(contents).ok()?;
    let hosts = root.as_object()?;
    hosts.iter().find_map(|(host, value)| {
        (host == "github.com" || host.starts_with("github.com:"))
            .then(|| text(value.get("oauth_token")))
            .flatten()
            .map(|token| SecretBytes::new(token.as_bytes().to_vec()))
    })
}

fn gh_token(contents: &[u8]) -> Option<SecretBytes> {
    let source = std::str::from_utf8(contents).ok()?;
    let mut in_github = false;
    for line in source.lines() {
        if !line.starts_with(char::is_whitespace) {
            in_github = line.trim_start().starts_with("github.com:");
            continue;
        }
        if !in_github {
            continue;
        }
        if let Some(value) = line.trim().strip_prefix("oauth_token:") {
            let token = value.trim().trim_matches(['\'', '"']);
            if !token.is_empty() {
                return Some(SecretBytes::new(token.as_bytes().to_vec()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_host_scope_prevents_enterprise_token_leak() {
        let yaml = b"enterprise.example:\n  oauth_token: enterprise-secret\ngithub.com:\n  oauth_token: github-secret\n";
        assert_eq!(gh_token(yaml).unwrap().expose(), b"github-secret");
    }
}
