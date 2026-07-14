use std::path::PathBuf;

use base64::Engine;
use rusqlite::params;
use serde_json::Value;

use crate::contracts::ErrorCategory;
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;
use crate::platform::sqlite::private_snapshot;

use super::super::support::{ProviderError, number, text};

const ACCESS_TOKEN_KEY: &str = "cursorAuth/accessToken";
const REFRESH_TOKEN_KEY: &str = "cursorAuth/refreshToken";
const MEMBERSHIP_KEY: &str = "cursorAuth/stripeMembershipType";
const REFRESH_WINDOW_SECONDS: i64 = 5 * 60;

pub struct CursorCredentials {
    pub access_token: Option<SecretBytes>,
    pub refresh_token: Option<SecretBytes>,
    pub membership_type: Option<String>,
}

impl CursorCredentials {
    pub fn needs_refresh(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.access_token
            .as_ref()
            .and_then(token_expiry)
            .is_none_or(|expiry| expiry.timestamp() - now.timestamp() <= REFRESH_WINDOW_SECONDS)
    }

    pub fn subject(&self) -> Option<String> {
        self.access_token.as_ref().and_then(token_subject)
    }
}

#[derive(Debug, Clone)]
pub struct CursorAuthStore {
    state_db_path: PathBuf,
}

impl CursorAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Self {
        Self::new(
            paths
                .roaming_app_data
                .join("Cursor/User/globalStorage/state.vscdb"),
        )
    }

    pub fn new(state_db_path: PathBuf) -> Self {
        Self { state_db_path }
    }

    pub async fn has_usable_credentials(&self) -> bool {
        self.load().await.is_ok()
    }

    pub async fn load(&self) -> Result<CursorCredentials, ProviderError> {
        if !tokio::fs::try_exists(&self.state_db_path)
            .await
            .unwrap_or(false)
        {
            return Err(ProviderError::new(
                ErrorCategory::NotLoggedIn,
                "Not logged in. Sign in via Cursor or run `agent login`.",
            ));
        }
        let snapshot = private_snapshot(self.state_db_path.clone())
            .await
            .map_err(|_| {
                ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "Cursor login database could not be read.",
                )
            })?;
        let connection = snapshot.open().map_err(|_| {
            ProviderError::new(
                ErrorCategory::CredentialAccess,
                "Cursor login database could not be read.",
            )
        })?;
        let access = read_value(&connection, ACCESS_TOKEN_KEY)?;
        let refresh = read_value(&connection, REFRESH_TOKEN_KEY)?;
        if access.is_none() && refresh.is_none() {
            return Err(ProviderError::new(
                ErrorCategory::NotLoggedIn,
                "Not logged in. Sign in via Cursor or run `agent login`.",
            ));
        }
        Ok(CursorCredentials {
            access_token: access.map(|value| SecretBytes::new(value.into_bytes())),
            refresh_token: refresh.map(|value| SecretBytes::new(value.into_bytes())),
            membership_type: read_value(&connection, MEMBERSHIP_KEY)?
                .map(|value| value.to_lowercase()),
        })
    }
}

fn read_value(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<String>, ProviderError> {
    let value = connection.query_row(
        "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
        params![key],
        |row| row.get::<_, String>(0),
    );
    match value {
        Ok(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(_) => Err(ProviderError::new(
            ErrorCategory::CredentialAccess,
            "Cursor login database could not be queried.",
        )),
    }
}

fn token_payload(token: &SecretBytes) -> Option<Value> {
    let token = std::str::from_utf8(token.expose()).ok()?;
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn token_expiry(token: &SecretBytes) -> Option<chrono::DateTime<chrono::Utc>> {
    let payload = token_payload(token)?;
    chrono::DateTime::from_timestamp(number(payload.get("exp"))? as i64, 0)
}

fn token_subject(token: &SecretBytes) -> Option<String> {
    let payload = token_payload(token)?;
    text(payload.get("sub")).map(str::to_owned)
}
