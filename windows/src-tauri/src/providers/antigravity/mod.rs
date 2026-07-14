use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::contracts::{ErrorCategory, MetricFormat, MetricLine, ProviderSnapshot};
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

use super::support::{ProviderError, number, provider_failure, timestamp};

const PROVIDER_ID: &str = "antigravity";
const BUCKETS: [(&str, &str, u64); 4] = [
    ("gemini-5h", "Session", 5 * 60 * 60 * 1000),
    ("gemini-weekly", "Weekly", 7 * 24 * 60 * 60 * 1000),
    ("3p-5h", "Claude", 5 * 60 * 60 * 1000),
    ("3p-weekly", "Claude Weekly", 7 * 24 * 60 * 60 * 1000),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct AntigravityProvider;

impl AntigravityProvider {
    pub fn is_local_process_running(&self) -> Result<bool, ProviderError> {
        process_running(&["language_server.exe", "agy.exe"])
    }
}

#[async_trait]
impl ProviderRuntime for AntigravityProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    async fn refresh(&self, _: CancellationToken) -> Result<ProviderSnapshot, ProviderFailure> {
        let running = self.is_local_process_running().map_err(provider_failure)?;
        let error = if running {
            ProviderError::new(
                ErrorCategory::NotAvailable,
                "Antigravity is running, but its authenticated loopback endpoint cannot yet be discovered safely on Windows.",
            )
        } else {
            ProviderError::new(
                ErrorCategory::NotLoggedIn,
                "Start Antigravity or run `agy` and try again.",
            )
        };
        Err(provider_failure(error))
    }
}

pub fn map_quota_summary(contents: &[u8]) -> Result<Vec<MetricLine>, ProviderError> {
    let root: serde_json::Value = serde_json::from_slice(contents).map_err(|_| {
        ProviderError::new(
            ErrorCategory::Decoding,
            "Antigravity quota response is invalid.",
        )
    })?;
    let container = root.get("response").unwrap_or(&root);
    let groups = container
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ProviderError::new(
                ErrorCategory::Decoding,
                "Antigravity quota response is invalid.",
            )
        })?;
    let values = groups
        .iter()
        .filter_map(|group| group.get("buckets")?.as_array())
        .flatten();
    let mut lines = Vec::new();
    for (bucket_id, label, period) in BUCKETS {
        let Some(bucket) = values.clone().find(|bucket| {
            bucket.get("bucketId").and_then(serde_json::Value::as_str) == Some(bucket_id)
        }) else {
            continue;
        };
        let Some(remaining) = number(bucket.get("remainingFraction")) else {
            continue;
        };
        lines.push(MetricLine::Progress {
            label: label.to_owned(),
            used: ((1.0 - remaining.clamp(0.0, 1.0)) * 100.0).round(),
            limit: 100.0,
            format: MetricFormat::Percent,
            resets_at: timestamp(bucket.get("resetTime")),
            period_duration_ms: Some(period),
            color_hex: None,
        });
    }
    Ok(lines)
}

#[cfg(windows)]
fn process_running(names: &[&str]) -> Result<bool, ProviderError> {
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ProviderError::new(
            ErrorCategory::Other,
            "Windows process discovery failed.",
        ));
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    let mut current = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while current {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if names
            .iter()
            .any(|name| executable.eq_ignore_ascii_case(name))
        {
            found = true;
            break;
        }
        current = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    Ok(found)
}

#[cfg(not(windows))]
fn process_running(_: &[&str]) -> Result<bool, ProviderError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_fixture_maps_four_exact_buckets() {
        let lines = map_quota_summary(include_bytes!(
            "../../../../../Tests/Fixtures/ProviderParity/v1/providers/antigravity/quota-summary.json"
        )).unwrap();
        assert_eq!(
            lines.iter().map(MetricLine::label).collect::<Vec<_>>(),
            ["Session", "Weekly", "Claude", "Claude Weekly"]
        );
    }

    #[test]
    fn malformed_summary_fails_loudly() {
        assert_eq!(
            map_quota_summary(b"{}").unwrap_err().category,
            ErrorCategory::Decoding
        );
    }
}
