use std::path::PathBuf;

use base64::Engine;

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, number, text};

const FILE_LIMIT: u64 = 512 * 1024;
const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const REFRESH_WINDOW_SECONDS: i64 = 5 * 60;

pub struct GrokCredential {
    pub access: SecretBytes,
    pub refresh: Option<SecretBytes>,
    pub client_id: String,
    pub entry_key: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct GrokAuthStore {
    path: PathBuf,
    files: AtomicFileStore,
}

impl GrokAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Result<Self, ProviderError> {
        let root = match std::env::var_os("GROK_HOME") {
            Some(root) if !root.is_empty() => PathBuf::from(root),
            Some(_) => return Err(invalid_auth()),
            None => paths.user_profile.join(".grok"),
        };
        Ok(Self::new(root.join("auth.json")))
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            files: AtomicFileStore::with_max_file_bytes(FILE_LIMIT),
        }
    }
    pub fn log_path(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("logs/unified.jsonl")
    }

    pub async fn load_all(&self) -> Result<Vec<GrokCredential>, ProviderError> {
        let contents = match self.files.read(&self.path).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProviderError::not_logged_in("grok login"));
            }
            Err(_) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "Grok credentials could not be read.",
                ));
            }
        };
        let root: serde_json::Value =
            serde_json::from_slice(&contents).map_err(|_| invalid_auth())?;
        let entries = root.as_object().ok_or_else(invalid_auth)?;
        let mut credentials = Vec::new();
        for (entry_key, entry) in entries {
            let Some(access) = text(entry.get("key")) else {
                continue;
            };
            let refresh = text(entry.get("refresh_token")).or_else(|| text(entry.get("refresh")));
            let client_id = text(entry.get("oidc_client_id"))
                .or_else(|| {
                    entry_key
                        .rsplit("::")
                        .next()
                        .filter(|value| !value.is_empty())
                })
                .unwrap_or(DEFAULT_CLIENT_ID)
                .to_owned();
            let expires_at = text(entry.get("expires_at"))
                .or_else(|| text(entry.get("expires")))
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&chrono::Utc))
                .or_else(|| jwt_expiry(access));
            credentials.push(GrokCredential {
                access: SecretBytes::new(access.as_bytes().to_vec()),
                refresh: refresh.map(|value| SecretBytes::new(value.as_bytes().to_vec())),
                client_id,
                entry_key: entry_key.clone(),
                expires_at,
            });
        }
        if credentials.is_empty() {
            Err(invalid_auth())
        } else {
            Ok(credentials)
        }
    }

    pub async fn save_refreshed(
        &self,
        previous: &GrokCredential,
        access: &SecretBytes,
        refresh: Option<&SecretBytes>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ProviderError> {
        let contents = self
            .files
            .read(&self.path)
            .await
            .map_err(|_| credential_error())?;
        let mut root: serde_json::Value =
            serde_json::from_slice(&contents).map_err(|_| invalid_auth())?;
        let entry = root
            .get_mut(&previous.entry_key)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(invalid_auth)?;
        if text(entry.get("key")).map(str::as_bytes) != Some(previous.access.expose()) {
            return Err(ProviderError::new(
                ErrorCategory::AuthExpired,
                "Grok login changed during refresh.",
            ));
        }
        entry.insert(
            "key".to_owned(),
            serde_json::Value::String(secret_text(access)?),
        );
        if let Some(refresh) = refresh {
            entry.insert(
                "refresh_token".to_owned(),
                serde_json::Value::String(secret_text(refresh)?),
            );
        }
        entry.insert(
            "expires_at".to_owned(),
            serde_json::Value::String(expires_at.to_rfc3339()),
        );
        let encoded = serde_json::to_vec_pretty(&root).map_err(|_| invalid_auth())?;
        self.files
            .write(&self.path, &encoded)
            .await
            .map_err(|_| credential_error())
    }
}

impl GrokCredential {
    pub fn needs_refresh(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.expires_at
            .is_some_and(|expiry| expiry.timestamp() - now.timestamp() <= REFRESH_WINDOW_SECONDS)
    }
}

fn jwt_expiry(token: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    chrono::DateTime::from_timestamp(number(value.get("exp"))? as i64, 0)
}
fn secret_text(secret: &SecretBytes) -> Result<String, ProviderError> {
    std::str::from_utf8(secret.expose())
        .map(str::to_owned)
        .map_err(|_| invalid_auth())
}
fn invalid_auth() -> ProviderError {
    ProviderError::new(ErrorCategory::AuthInvalid, "Grok credentials are invalid.")
}
fn credential_error() -> ProviderError {
    ProviderError::new(
        ErrorCategory::CredentialAccess,
        "Grok credentials could not be saved.",
    )
}
