use std::path::PathBuf;

use serde_json::Value;

use crate::contracts::ErrorCategory;
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, number, text};

const CREDENTIAL_FILE: &str = ".credentials.json";
const CREDENTIAL_LIMIT: u64 = 128 * 1024;
const REFRESH_WINDOW_MS: f64 = 5.0 * 60.0 * 1000.0;

pub struct ClaudeCredentials {
    pub access_token: SecretBytes,
    pub refresh_token: Option<SecretBytes>,
    pub expires_at_ms: Option<f64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub scopes: Vec<String>,
    path: PathBuf,
}

impl ClaudeCredentials {
    pub fn has_profile_scope(&self) -> bool {
        self.scopes.iter().any(|scope| scope == "user:profile")
    }

    pub fn needs_refresh(&self, now_ms: f64) -> bool {
        self.expires_at_ms
            .is_some_and(|expires_at| expires_at - now_ms <= REFRESH_WINDOW_MS)
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeAuthStore {
    credential_path: PathBuf,
    files: AtomicFileStore,
}

impl ClaudeAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Result<Self, ProviderError> {
        let override_path = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
        if override_path
            .as_ref()
            .is_some_and(|path| path.to_string_lossy().contains(','))
        {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "CLAUDE_CONFIG_DIR must name one native Windows directory.",
            ));
        }
        let home = override_path.unwrap_or_else(|| paths.user_profile.join(".claude"));
        Ok(Self::new(home.join(CREDENTIAL_FILE)))
    }

    pub fn new(credential_path: PathBuf) -> Self {
        Self {
            credential_path,
            files: AtomicFileStore::with_max_file_bytes(CREDENTIAL_LIMIT),
        }
    }

    pub fn projects_root(&self) -> PathBuf {
        self.credential_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("projects")
    }

    pub async fn has_usable_credentials(&self) -> bool {
        self.load().await.is_ok()
    }

    pub async fn load(&self) -> Result<ClaudeCredentials, ProviderError> {
        let contents = match self.files.read(&self.credential_path).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProviderError::not_logged_in("claude"));
            }
            Err(error) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    format!("Claude credentials could not be read: {error}"),
                ));
            }
        };
        let root: Value = serde_json::from_slice(&contents).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Claude credentials are malformed.",
            )
        })?;
        let oauth = root
            .get("claudeAiOauth")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProviderError::new(
                    ErrorCategory::AuthInvalid,
                    "Claude credentials contain no OAuth login.",
                )
            })?;
        let access_token =
            text(oauth.get("accessToken")).ok_or_else(|| ProviderError::not_logged_in("claude"))?;
        Ok(ClaudeCredentials {
            access_token: SecretBytes::new(access_token.as_bytes().to_vec()),
            refresh_token: text(oauth.get("refreshToken"))
                .map(|token| SecretBytes::new(token.as_bytes().to_vec())),
            expires_at_ms: number(oauth.get("expiresAt")),
            subscription_type: text(oauth.get("subscriptionType")).map(str::to_owned),
            rate_limit_tier: text(oauth.get("rateLimitTier")).map(str::to_owned),
            scopes: oauth
                .get("scopes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            path: self.credential_path.clone(),
        })
    }

    pub async fn save_refreshed(
        &self,
        previous: &ClaudeCredentials,
        access_token: &SecretBytes,
        refresh_token: Option<&SecretBytes>,
        expires_at_ms: f64,
    ) -> Result<(), ProviderError> {
        let contents = self.files.read(&previous.path).await.map_err(|_| {
            ProviderError::new(
                ErrorCategory::CredentialAccess,
                "Claude credentials changed during refresh.",
            )
        })?;
        let mut root: Value = serde_json::from_slice(&contents).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Claude credentials are malformed.",
            )
        })?;
        let oauth = root
            .get_mut("claudeAiOauth")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                ProviderError::new(
                    ErrorCategory::AuthInvalid,
                    "Claude credentials are malformed.",
                )
            })?;
        if text(oauth.get("accessToken")).map(str::as_bytes) != Some(previous.access_token.expose())
        {
            return Err(ProviderError::new(
                ErrorCategory::AuthExpired,
                "Claude login changed during refresh. Refresh again.",
            ));
        }
        oauth.insert(
            "accessToken".to_owned(),
            Value::String(secret_text(access_token)?),
        );
        if let Some(refresh_token) = refresh_token {
            oauth.insert(
                "refreshToken".to_owned(),
                Value::String(secret_text(refresh_token)?),
            );
        }
        oauth.insert("expiresAt".to_owned(), Value::from(expires_at_ms));
        let encoded = serde_json::to_vec_pretty(&root).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Claude credentials could not be encoded.",
            )
        })?;
        self.files
            .write(&previous.path, &encoded)
            .await
            .map_err(|error| {
                ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    format!("Claude credentials could not be saved: {error}"),
                )
            })
    }
}

fn secret_text(secret: &SecretBytes) -> Result<String, ProviderError> {
    std::str::from_utf8(secret.expose())
        .map(str::to_owned)
        .map_err(|_| ProviderError::new(ErrorCategory::AuthInvalid, "Stored token is invalid."))
}
