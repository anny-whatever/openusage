use std::io::Write;
use std::path::{Path, PathBuf};

const DEFAULT_LOG_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RedactingLog {
    path: PathBuf,
    max_bytes: u64,
}

impl RedactingLog {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            max_bytes: DEFAULT_LOG_LIMIT,
        }
    }

    pub fn with_max_bytes(path: PathBuf, max_bytes: u64) -> Self {
        Self { path, max_bytes }
    }

    pub fn append(&self, message: &str) -> Result<(), std::io::Error> {
        let sanitized = redact(message);
        if sanitized.is_empty() {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("log path has no parent"))?;
        std::fs::create_dir_all(parent)?;
        self.rotate_if_needed((sanitized.len() + 1) as u64)?;
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(log, "{sanitized}")?;
        log.flush()?;
        Ok(())
    }

    fn rotate_if_needed(&self, incoming_bytes: u64) -> Result<(), std::io::Error> {
        let current_bytes = std::fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if current_bytes.saturating_add(incoming_bytes) <= self.max_bytes {
            return Ok(());
        }
        let archive = archive_path(&self.path);
        if archive.exists() {
            std::fs::remove_file(&archive)?;
        }
        if self.path.exists() {
            std::fs::rename(&self.path, archive)?;
        }
        Ok(())
    }
}

fn archive_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.1", path.display()))
}

pub fn redact(message: &str) -> String {
    message
        .split_whitespace()
        .map(redact_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_token(token: &str) -> String {
    let lowercase = token.to_ascii_lowercase();
    if lowercase.starts_with("authorization:")
        || lowercase.starts_with("bearer")
        || lowercase.contains("access_token")
        || lowercase.contains("refresh_token")
        || lowercase.contains("api_key")
        || lowercase.contains("apikey")
    {
        return "[REDACTED]".to_owned();
    }
    if looks_like_windows_user_path(token) {
        return redact_windows_user_path(token);
    }
    if looks_like_unc_path(token) {
        return "\\\\[REDACTED]".to_owned();
    }
    token.chars().take(2048).collect()
}

fn looks_like_windows_user_path(token: &str) -> bool {
    let lowercase = token.to_ascii_lowercase();
    lowercase.contains(":\\users\\") || lowercase.contains(":/users/")
}

fn redact_windows_user_path(token: &str) -> String {
    let normalized = token.replace('/', "\\");
    let components = normalized.split('\\').collect::<Vec<_>>();
    if components.len() < 4 {
        return "[REDACTED-PATH]".to_owned();
    }
    format!("{}\\Users\\[REDACTED]", components[0])
}

fn looks_like_unc_path(token: &str) -> bool {
    token.starts_with("\\\\") || token.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_and_user_paths() {
        let message = "Authorization:Bearer-secret access_token=abc C:\\Users\\alice\\auth.json \\\\server\\share";
        let sanitized = redact(message);

        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("alice"));
        assert!(!sanitized.contains("server"));
    }

    #[test]
    fn rotation_keeps_only_current_and_one_archive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("openusage.log");
        let log = RedactingLog::with_max_bytes(path.clone(), 8);

        log.append("first").unwrap();
        log.append("second").unwrap();
        log.append("third").unwrap();

        assert!(path.exists());
        assert!(archive_path(&path).exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
