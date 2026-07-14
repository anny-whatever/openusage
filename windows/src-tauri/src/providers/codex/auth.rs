use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, number, text};

const AUTH_FILE: &str = "auth.json";
const AUTH_FILE_LIMIT: u64 = 256 * 1024;
const REFRESH_WINDOW_SECONDS: i64 = 5 * 60;
const FALLBACK_REFRESH_AGE_SECONDS: i64 = 8 * 24 * 60 * 60;

pub struct CodexCredentials {
    pub access_token: SecretBytes,
    pub refresh_token: Option<SecretBytes>,
    pub id_token: Option<SecretBytes>,
    pub account_id: Option<SecretBytes>,
    pub last_refresh: Option<String>,
    path: PathBuf,
}

impl CodexCredentials {
    pub fn needs_refresh(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        if let Some(expires_at) = access_token_expiry(&self.access_token) {
            return expires_at.timestamp() - now.timestamp() <= REFRESH_WINDOW_SECONDS;
        }
        self.last_refresh
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|last| now.timestamp() - last.timestamp() > FALLBACK_REFRESH_AGE_SECONDS)
    }
}

#[derive(Debug, Clone)]
pub struct CodexAuthStore {
    paths: Vec<PathBuf>,
    files: AtomicFileStore,
}

impl CodexAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Result<Self, ProviderError> {
        let candidates = match std::env::var_os("CODEX_HOME") {
            Some(home) if !home.is_empty() => vec![PathBuf::from(home).join(AUTH_FILE)],
            Some(_) => {
                return Err(ProviderError::new(
                    ErrorCategory::AuthInvalid,
                    "CODEX_HOME must not be empty.",
                ));
            }
            None => vec![
                paths.user_profile.join(".config/codex").join(AUTH_FILE),
                paths.user_profile.join(".codex").join(AUTH_FILE),
            ],
        };
        Ok(Self::new(candidates))
    }

    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            files: AtomicFileStore::with_max_file_bytes(AUTH_FILE_LIMIT),
        }
    }

    pub fn history_roots(&self) -> Vec<PathBuf> {
        self.paths
            .iter()
            .filter_map(|path| path.parent().map(Path::to_owned))
            .collect()
    }

    pub async fn has_usable_credentials(&self) -> bool {
        self.load().await.is_ok()
    }

    pub async fn load(&self) -> Result<CodexCredentials, ProviderError> {
        self.load_all()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::not_logged_in("codex"))
    }

    pub async fn load_all(&self) -> Result<Vec<CodexCredentials>, ProviderError> {
        let mut first_error = None;
        let mut credentials = Vec::new();
        for path in &self.paths {
            match self.load_path(path).await {
                Ok(candidate) => credentials.push(candidate),
                Err(error) if error.category == ErrorCategory::NotLoggedIn => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            };
        }
        if credentials.is_empty() {
            Err(first_error.unwrap_or_else(|| ProviderError::not_logged_in("codex")))
        } else {
            Ok(credentials)
        }
    }

    pub async fn reload(
        &self,
        previous: &CodexCredentials,
    ) -> Result<CodexCredentials, ProviderError> {
        self.load_path(&previous.path).await
    }

    async fn load_path(&self, path: &Path) -> Result<CodexCredentials, ProviderError> {
        let contents = match self.files.read(path).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProviderError::not_logged_in("codex"));
            }
            Err(error) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    format!("Codex credentials could not be read: {error}"),
                ));
            }
        };
        let root: Value = serde_json::from_slice(&contents).map_err(|_| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Codex auth data is invalid.")
        })?;
        let tokens = root.get("tokens").and_then(Value::as_object);
        let access = tokens.and_then(|tokens| text(tokens.get("access_token")));
        if access.is_none() && text(root.get("OPENAI_API_KEY")).is_some() {
            return Err(ProviderError::new(
                ErrorCategory::NotAvailable,
                "Usage is not available for API-key authentication.",
            ));
        }
        let access = access.ok_or_else(|| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Codex auth data is invalid.")
        })?;
        Ok(CodexCredentials {
            access_token: SecretBytes::new(access.as_bytes().to_vec()),
            refresh_token: tokens
                .and_then(|tokens| text(tokens.get("refresh_token")))
                .map(|value| SecretBytes::new(value.as_bytes().to_vec())),
            id_token: tokens
                .and_then(|tokens| text(tokens.get("id_token")))
                .map(|value| SecretBytes::new(value.as_bytes().to_vec())),
            account_id: tokens
                .and_then(|tokens| text(tokens.get("account_id")))
                .map(|value| SecretBytes::new(value.as_bytes().to_vec())),
            last_refresh: text(root.get("last_refresh")).map(str::to_owned),
            path: path.to_owned(),
        })
    }

    pub async fn save_refreshed(
        &self,
        previous: &CodexCredentials,
        access_token: &SecretBytes,
        refresh_token: Option<&SecretBytes>,
        id_token: Option<&SecretBytes>,
    ) -> Result<(), ProviderError> {
        let contents = self.files.read(&previous.path).await.map_err(|_| {
            ProviderError::new(
                ErrorCategory::CredentialAccess,
                "Codex auth changed during refresh.",
            )
        })?;
        let mut root: Value = serde_json::from_slice(&contents).map_err(|_| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Codex auth data is invalid.")
        })?;
        let tokens = root
            .get_mut("tokens")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                ProviderError::new(ErrorCategory::AuthInvalid, "Codex auth data is invalid.")
            })?;
        if text(tokens.get("access_token")).map(str::as_bytes)
            != Some(previous.access_token.expose())
        {
            return Err(ProviderError::new(
                ErrorCategory::AuthExpired,
                "Codex login changed during refresh. Refresh again.",
            ));
        }
        insert_secret(tokens, "access_token", access_token)?;
        if let Some(refresh_token) = refresh_token {
            insert_secret(tokens, "refresh_token", refresh_token)?;
        }
        if let Some(id_token) = id_token {
            insert_secret(tokens, "id_token", id_token)?;
        }
        root.as_object_mut()
            .expect("auth root was validated as an object")
            .insert(
                "last_refresh".to_owned(),
                Value::String(
                    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
                        .to_rfc3339(),
                ),
            );
        let encoded = serde_json::to_vec_pretty(&root).map_err(|_| {
            ProviderError::new(ErrorCategory::Other, "Codex auth could not be encoded.")
        })?;
        self.files
            .write(&previous.path, &encoded)
            .await
            .map_err(|error| {
                ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    format!("Codex auth could not be saved: {error}"),
                )
            })
    }
}

fn insert_secret(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: &SecretBytes,
) -> Result<(), ProviderError> {
    let value = std::str::from_utf8(value.expose()).map_err(|_| {
        ProviderError::new(ErrorCategory::AuthInvalid, "Stored Codex token is invalid.")
    })?;
    object.insert(key.to_owned(), Value::String(value.to_owned()));
    Ok(())
}

fn access_token_expiry(token: &SecretBytes) -> Option<chrono::DateTime<chrono::Utc>> {
    let token = std::str::from_utf8(token.expose()).ok()?;
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    let seconds = number(value.get("exp"))? as i64;
    chrono::DateTime::from_timestamp(seconds, 0)
}
