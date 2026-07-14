use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url};
use rusqlite::params;
use tokio_util::sync::CancellationToken;

use crate::contracts::{
    ErrorCategory, MetricFormat, MetricKind, MetricLine, MetricValue, ProviderSnapshot,
};
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};
use crate::platform::http::{HttpRequest, HttpTransport};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::SecretBytes;
use crate::platform::sqlite::private_snapshot;
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

use super::support::{ProviderError, number, provider_failure, snapshot, text, timestamp};

const PROVIDER_ID: &str = "devin";
const DISPLAY_NAME: &str = "Devin";
const DEFAULT_SERVER: &str = "https://server.codeium.com";
const SERVICE: &str = "exa.seat_management_pb.SeatManagementService";
const FILE_LIMIT: u64 = 256 * 1024;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const WEEK_MS: u64 = 7 * DAY_MS;

struct DevinCredential {
    key: SecretBytes,
    server: Url,
}

#[derive(Debug, Clone)]
pub struct DevinAuthStore {
    credential_file: PathBuf,
    databases: Vec<PathBuf>,
    files: AtomicFileStore,
}

impl DevinAuthStore {
    pub fn from_windows(paths: &WindowsPaths) -> Self {
        Self::new(
            paths
                .user_profile
                .join(".local/share/devin/credentials.toml"),
            vec![
                paths
                    .roaming_app_data
                    .join("Devin/User/globalStorage/state.vscdb"),
                paths
                    .roaming_app_data
                    .join("Devin - Next/User/globalStorage/state.vscdb"),
            ],
        )
    }

    pub fn new(credential_file: PathBuf, databases: Vec<PathBuf>) -> Self {
        Self {
            credential_file,
            databases,
            files: AtomicFileStore::with_max_file_bytes(FILE_LIMIT),
        }
    }

    async fn load_all(&self) -> Result<Vec<DevinCredential>, ProviderError> {
        let mut credentials = Vec::new();
        if let Some(credential) = self.load_file().await? {
            credentials.push(credential);
        }
        for database in &self.databases {
            if let Some(credential) = load_database(database).await? {
                credentials.push(credential);
            }
        }
        if credentials.is_empty() {
            Err(ProviderError::not_logged_in("devin auth login"))
        } else {
            Ok(credentials)
        }
    }

    async fn load_file(&self) -> Result<Option<DevinCredential>, ProviderError> {
        let contents = match self.files.read(&self.credential_file).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(_) => {
                return Err(ProviderError::new(
                    ErrorCategory::CredentialAccess,
                    "Devin credentials could not be read.",
                ));
            }
        };
        let text = std::str::from_utf8(&contents).map_err(|_| invalid_auth())?;
        let Some(key) = toml_value(text, "windsurf_api_key") else {
            return Ok(None);
        };
        let server = toml_value(text, "api_server_url").unwrap_or(DEFAULT_SERVER);
        let server = safe_server(server)?;
        Ok(Some(DevinCredential {
            key: SecretBytes::new(key.as_bytes().to_vec()),
            server,
        }))
    }
}

async fn load_database(path: &Path) -> Result<Option<DevinCredential>, ProviderError> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(None);
    }
    let snapshot = private_snapshot(path.to_owned())
        .await
        .map_err(|_| credential_error())?;
    let connection = snapshot.open().map_err(|_| credential_error())?;
    let value = connection.query_row(
        "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
        params!["windsurfAuthStatus"],
        |row| row.get::<_, String>(0),
    );
    let value = match value {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(_) => return Err(credential_error()),
    };
    let body: serde_json::Value = serde_json::from_str(&value).map_err(|_| invalid_auth())?;
    let key = text(body.get("apiKey")).ok_or_else(invalid_auth)?;
    Ok(Some(DevinCredential {
        key: SecretBytes::new(key.as_bytes().to_vec()),
        server: Url::parse(DEFAULT_SERVER).expect("static Devin URL"),
    }))
}

pub struct DevinProvider {
    auth: DevinAuthStore,
    http: Arc<dyn HttpTransport>,
}

impl DevinProvider {
    pub fn new(auth: DevinAuthStore, http: Arc<dyn HttpTransport>) -> Self {
        Self { auth, http }
    }
    pub async fn has_local_credentials(&self) -> bool {
        self.auth.load_all().await.is_ok()
    }

    async fn refresh_provider(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let candidates = self.auth.load_all().await?;
        let mut last_error = None;
        for credential in candidates {
            match self.attempt(&credential, cancellation.child_token()).await {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| ProviderError::not_logged_in("devin auth login")))
    }

    async fn attempt(
        &self,
        credential: &DevinCredential,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let key = std::str::from_utf8(credential.key.expose()).map_err(|_| invalid_auth())?;
        let body = serde_json::to_vec(&serde_json::json!({ "metadata": {
            "apiKey": key, "ideName": "devin", "ideVersion": "1.108.2",
            "extensionName": "devin", "extensionVersion": "1.108.2", "locale": "en"
        }}))
        .map_err(|_| invalid_auth())?;
        let url = credential
            .server
            .join(&format!("{SERVICE}/GetUserStatus"))
            .map_err(|_| invalid_auth())?;
        let response = self
            .http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url,
                    headers: vec![(
                        reqwest::header::CONTENT_TYPE,
                        reqwest::header::HeaderValue::from_static("application/json"),
                    )],
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Devin could not be reached.")
            })?;
        if response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Devin login is invalid.",
            ));
        }
        if !response.status.is_success() {
            return Err(ProviderError::http(response.status));
        }
        let (plan, lines) = map_usage(&response.body)?;
        Ok(snapshot(PROVIDER_ID, DISPLAY_NAME, plan, lines, None, None))
    }
}

fn map_usage(contents: &[u8]) -> Result<(Option<String>, Vec<MetricLine>), ProviderError> {
    let root: serde_json::Value =
        serde_json::from_slice(contents).map_err(|_| invalid_response())?;
    let status = root
        .get("userStatus")
        .and_then(|value| value.get("planStatus"))
        .ok_or_else(invalid_response)?;
    let plan_info = status.get("planInfo");
    let plan = plan_info
        .and_then(|value| text(value.get("planName")))
        .map(str::to_owned)
        .or_else(|| Some("Unknown".to_owned()));
    let hide_daily = plan_info
        .and_then(|value| value.get("hideDailyQuota"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let mut lines = Vec::new();
    let daily = number(status.get("dailyQuotaRemainingPercent"));
    if !hide_daily && let Some(remaining) = daily {
        lines.push(quota_line(
            "Daily quota",
            remaining,
            timestamp(status.get("dailyQuotaResetAtUnix")),
            DAY_MS,
        ));
    }
    if let Some(remaining) = number(status.get("weeklyQuotaRemainingPercent"))
        .or_else(|| hide_daily.then_some(daily).flatten())
    {
        lines.push(quota_line(
            "Weekly quota",
            remaining,
            timestamp(status.get("weeklyQuotaResetAtUnix")),
            WEEK_MS,
        ));
    }
    if let Some(micros) = number(status.get("overageBalanceMicros")) {
        lines.push(MetricLine::Values {
            label: "Extra usage balance".to_owned(),
            values: vec![MetricValue {
                number: micros.max(0.0) / 1_000_000.0,
                kind: MetricKind::Dollars,
                label: None,
                estimated: false,
            }],
            color_hex: None,
            expiries_at: Vec::new(),
            unknown_models: Vec::new(),
        });
    }
    if lines.is_empty() {
        return Err(invalid_response());
    }
    Ok((plan, lines))
}

fn quota_line(label: &str, remaining: f64, resets_at: Option<String>, period: u64) -> MetricLine {
    MetricLine::Progress {
        label: label.to_owned(),
        used: (100.0 - remaining).clamp(0.0, 100.0),
        limit: 100.0,
        format: MetricFormat::Percent,
        resets_at,
        period_duration_ms: Some(period),
        color_hex: None,
    }
}

fn toml_value<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    source.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value.split('#').next()?.trim().trim_matches(['\'', '"']);
        (!value.is_empty()).then_some(value)
    })
}

fn safe_server(value: &str) -> Result<Url, ProviderError> {
    let normalized = format!("{}/", value.trim_end_matches('/'));
    let url = Url::parse(&normalized).map_err(|_| invalid_auth())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(invalid_auth());
    }
    Ok(url)
}
fn invalid_auth() -> ProviderError {
    ProviderError::new(ErrorCategory::AuthInvalid, "Devin credentials are invalid.")
}
fn credential_error() -> ProviderError {
    ProviderError::new(
        ErrorCategory::CredentialAccess,
        "Devin login database could not be read.",
    )
}
fn invalid_response() -> ProviderError {
    ProviderError::new(ErrorCategory::Decoding, "Devin quota response is invalid.")
}

#[async_trait]
impl ProviderRuntime for DevinProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }
    async fn refresh(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderFailure> {
        self.refresh_provider(cancellation)
            .await
            .map_err(provider_failure)
    }
}

#[cfg(test)]
mod tests;
