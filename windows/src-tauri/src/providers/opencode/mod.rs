mod auth;
mod scanner;
mod windows;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::contracts::{ErrorCategory, MetricLine, ProviderSnapshot};
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::OpenCodeAuthStore;

use super::support::{ProviderError, history_metrics, provider_failure, snapshot};

const PROVIDER_ID: &str = "opencode";
const DISPLAY_NAME: &str = "OpenCode";

pub struct OpenCodeProvider {
    auth: OpenCodeAuthStore,
}

impl OpenCodeProvider {
    pub fn new(auth: OpenCodeAuthStore) -> Self {
        Self { auth }
    }

    pub async fn has_local_credentials(&self) -> bool {
        match self.auth.go_key().await {
            Ok(Some(_)) | Err(_) => true,
            Ok(None) => self
                .auth
                .database_files()
                .is_ok_and(|files| !files.is_empty()),
        }
    }

    async fn refresh_provider(&self) -> Result<ProviderSnapshot, ProviderError> {
        let (has_go_key, auth_error) = match self.auth.go_key().await {
            Ok(key) => (key.is_some(), None),
            Err(error) => (false, Some(error)),
        };
        let auth_warning = auth_error
            .as_ref()
            .map(|_| "OpenCode auth.json is temporarily unavailable.".to_owned());
        let databases = self.auth.database_files()?;
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
        let scan = scanner::scan(databases, now).await?;
        let Some(scan) = scan else {
            if has_go_key {
                return Ok(snapshot(
                    PROVIDER_ID,
                    DISPLAY_NAME,
                    Some("Go".to_owned()),
                    windows::meter_lines(&[], None, now),
                    None,
                    auth_warning,
                ));
            }
            if let Some(error) = auth_error {
                return Err(error);
            }
            return Err(ProviderError::new(
                ErrorCategory::NotLoggedIn,
                "OpenCode was not detected. Log in with OpenCode Go or use OpenCode locally first.",
            ));
        };
        let mut lines = Vec::new();
        let show_go = has_go_key || !scan.go_rows.is_empty();
        if show_go {
            lines.extend(windows::meter_lines(&scan.go_rows, scan.go_anchor_ms, now));
        }
        let mut usage_history = None;
        if !scan.daily.is_empty() {
            let (history_lines, normalized) =
                history_metrics(&scan.daily, now, "From your OpenCode logs.");
            lines.extend(history_lines);
            usage_history = Some(normalized);
        }
        if lines.is_empty() {
            lines.push(MetricLine::Badge {
                label: "Status".to_owned(),
                text: "No usage data".to_owned(),
                color_hex: None,
                subtitle: None,
            });
        }
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            show_go.then(|| "Go".to_owned()),
            lines,
            usage_history,
            auth_warning,
        ))
    }
}

#[async_trait]
impl ProviderRuntime for OpenCodeProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }
    async fn refresh(&self, _: CancellationToken) -> Result<ProviderSnapshot, ProviderFailure> {
        self.refresh_provider().await.map_err(provider_failure)
    }
}

#[cfg(test)]
mod tests;
