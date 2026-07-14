use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, PrivateFileStore};
use crate::platform::environment::EnvironmentReader;
use crate::platform::secret::{DpapiSecretStore, SecretBytes, SecretStatus};

use super::support::{ProviderError, text};

const COMPATIBILITY_FILE_LIMIT: u64 = 64 * 1024;
const MAX_API_KEY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiKeyStatus {
    Missing,
    FromEnvironment,
    Stored,
    OverrideActive,
}

#[derive(Clone)]
pub struct ProtectedApiKeyStore {
    name: &'static str,
    environment_names: &'static [&'static str],
    compatibility_paths: Vec<PathBuf>,
    environment: Arc<dyn EnvironmentReader>,
    secrets: DpapiSecretStore,
    files: AtomicFileStore,
}

impl ProtectedApiKeyStore {
    pub fn new(
        name: &'static str,
        environment_names: &'static [&'static str],
        compatibility_paths: Vec<PathBuf>,
        environment: Arc<dyn EnvironmentReader>,
        secrets: DpapiSecretStore,
    ) -> Self {
        Self {
            name,
            environment_names,
            compatibility_paths,
            environment,
            secrets,
            files: AtomicFileStore::with_max_file_bytes(COMPATIBILITY_FILE_LIMIT),
        }
    }

    pub async fn status(&self) -> Result<ApiKeyStatus, ProviderError> {
        let stored = self
            .secrets
            .status(self.name)
            .await
            .map_err(storage_error)?
            == SecretStatus::Stored;
        let external =
            self.compatibility_key().await?.is_some() || self.environment_key().is_some();
        Ok(match (stored, external) {
            (true, true) => ApiKeyStatus::OverrideActive,
            (true, false) => ApiKeyStatus::Stored,
            (false, true) => ApiKeyStatus::FromEnvironment,
            (false, false) => ApiKeyStatus::Missing,
        })
    }

    pub async fn load(&self) -> Result<Option<SecretBytes>, ProviderError> {
        if let Some(secret) = self.secrets.load(self.name).await.map_err(storage_error)? {
            return Ok(Some(secret));
        }
        if let Some(secret) = self.compatibility_key().await? {
            return Ok(Some(secret));
        }
        Ok(self.environment_key())
    }

    pub async fn save(&self, secret: SecretBytes) -> Result<ApiKeyStatus, ProviderError> {
        if secret.expose().is_empty() || secret.expose().len() > MAX_API_KEY_BYTES {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "API key must not be empty or exceed 16 KiB.",
            ));
        }
        self.secrets
            .save(self.name, secret)
            .await
            .map_err(storage_error)?;
        self.status().await
    }

    pub async fn delete(&self) -> Result<ApiKeyStatus, ProviderError> {
        self.secrets
            .delete(self.name)
            .await
            .map_err(storage_error)?;
        self.status().await
    }

    fn environment_key(&self) -> Option<SecretBytes> {
        self.environment_names
            .iter()
            .find_map(|name| self.environment.value(name))
            .filter(|secret| {
                !secret.expose().is_empty() && secret.expose().len() <= MAX_API_KEY_BYTES
            })
    }

    async fn compatibility_key(&self) -> Result<Option<SecretBytes>, ProviderError> {
        for path in &self.compatibility_paths {
            let contents = match self.files.read(path).await {
                Ok(contents) => contents,
                Err(crate::platform::atomic_file::FileStoreError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(_) => {
                    return Err(ProviderError::new(
                        ErrorCategory::CredentialAccess,
                        "API key compatibility file could not be read.",
                    ));
                }
            };
            if let Some(secret) = parse_compatibility_key(&contents) {
                return Ok(Some(secret));
            }
        }
        Ok(None)
    }
}

fn parse_compatibility_key(contents: &[u8]) -> Option<SecretBytes> {
    if contents.is_empty() || contents.len() > MAX_API_KEY_BYTES {
        return None;
    }
    if let Ok(value) = serde_json::from_slice::<Value>(contents) {
        let key = ["apiKey", "api_key", "key"]
            .iter()
            .find_map(|field| text(value.get(field)))?;
        return Some(SecretBytes::new(key.as_bytes().to_vec()));
    }
    let key = std::str::from_utf8(contents).ok()?.trim();
    (!key.is_empty() && !key.contains('{')).then(|| SecretBytes::new(key.as_bytes().to_vec()))
}

fn storage_error(_: impl std::fmt::Display) -> ProviderError {
    ProviderError::new(
        ErrorCategory::CredentialAccess,
        "Protected API key storage is unavailable.",
    )
}
