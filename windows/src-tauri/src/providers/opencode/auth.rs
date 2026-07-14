use std::path::{Path, PathBuf};

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, text};

const AUTH_LIMIT: u64 = 512 * 1024;

#[derive(Debug, Clone)]
pub struct OpenCodeAuthStore {
    pub data_directory: PathBuf,
    files: AtomicFileStore,
}

impl OpenCodeAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Result<Self, ProviderError> {
        let directory = if let Some(value) = std::env::var_os("OPENCODE_DATA_DIR") {
            PathBuf::from(value)
        } else if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(value).join("opencode")
        } else {
            paths.user_profile.join(".local/share/opencode")
        };
        if !is_native_absolute(&directory) {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "OpenCode data directory must be a native absolute Windows path.",
            ));
        }
        Ok(Self::new(directory))
    }

    pub fn new(data_directory: PathBuf) -> Self {
        Self {
            data_directory,
            files: AtomicFileStore::with_max_file_bytes(AUTH_LIMIT),
        }
    }

    pub async fn go_key(&self) -> Result<Option<SecretBytes>, ProviderError> {
        let path = self.data_directory.join("auth.json");
        let contents = match self.files.read(&path).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(_) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "OpenCode auth.json could not be read.",
                ));
            }
        };
        let body: serde_json::Value = serde_json::from_slice(&contents).map_err(|_| {
            ProviderError::new(
                ErrorCategory::CredentialAccess,
                "OpenCode auth.json is invalid.",
            )
        })?;
        Ok(
            text(body.get("opencode-go").and_then(|value| value.get("key")))
                .map(|key| SecretBytes::new(key.as_bytes().to_vec())),
        )
    }

    pub fn database_files(&self) -> Result<Vec<PathBuf>, ProviderError> {
        let entries = match std::fs::read_dir(&self.data_directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "OpenCode data directory could not be read.",
                ));
            }
        };
        let mut files = Vec::new();
        for entry in entries.take(128) {
            let entry = entry.map_err(|_| {
                ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "OpenCode data directory could not be read.",
                )
            })?;
            let file_type = entry.file_type().map_err(|_| {
                ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "OpenCode data directory could not be read.",
                )
            })?;
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if file_type.is_file() && name.starts_with("opencode") && name.ends_with(".db") {
                files.push(entry.path());
            }
        }
        files.sort();
        if files.len() > 32 {
            return Err(ProviderError::new(
                ErrorCategory::Other,
                "OpenCode database count exceeds the supported bound.",
            ));
        }
        Ok(files)
    }
}

fn is_native_absolute(path: &Path) -> bool {
    let rendered = path.to_string_lossy().to_ascii_lowercase();
    path.is_absolute() && !rendered.starts_with("/mnt/") && !rendered.starts_with("\\\\wsl")
}
