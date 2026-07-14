use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};

const CURRENT_SCHEMA: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    pub enabled_provider_ids: BTreeSet<String>,
    #[serde(default)]
    pub known_provider_ids: BTreeSet<String>,
    #[serde(default)]
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            enabled_provider_ids: ["claude", "codex", "cursor"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            known_provider_ids: BTreeSet::new(),
            launch_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsLoadState {
    Fresh,
    Current,
    Migrated,
    RecoveredCorrupt,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O failed: {0}")]
    File(#[from] FileStoreError),
    #[error("settings encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    files: AtomicFileStore,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            files: AtomicFileStore::default(),
        }
    }

    pub async fn load(&self) -> Result<(Settings, SettingsLoadState), SettingsError> {
        let contents = match self.files.read(&self.path).await {
            Ok(contents) => contents,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let settings = Settings::default();
                self.save(&settings).await?;
                return Ok((settings, SettingsLoadState::Fresh));
            }
            Err(error) => return Err(error.into()),
        };

        match migrate(&contents) {
            Ok((settings, migrated)) => {
                if migrated {
                    self.save(&settings).await?;
                    Ok((settings, SettingsLoadState::Migrated))
                } else {
                    Ok((settings, SettingsLoadState::Current))
                }
            }
            Err(_) => {
                let settings = Settings::default();
                self.save(&settings).await?;
                Ok((settings, SettingsLoadState::RecoveredCorrupt))
            }
        }
    }

    pub async fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        let encoded = serde_json::to_vec_pretty(settings)?;
        self.files.write(&self.path, &encoded).await?;
        Ok(())
    }
}

fn migrate(contents: &[u8]) -> Result<(Settings, bool), serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(contents)?;
    let schema = value.get("schema").and_then(serde_json::Value::as_u64);
    if schema == Some(CURRENT_SCHEMA as u64) {
        return serde_json::from_value(value).map(|settings| (settings, false));
    }
    if schema == Some(1) {
        let legacy: SettingsV1 = serde_json::from_value(value)?;
        let registry = provider_registry();
        let enabled_provider_ids = registry
            .difference(&legacy.disabled_provider_ids)
            .cloned()
            .collect();
        return Ok((
            Settings {
                schema: CURRENT_SCHEMA,
                enabled_provider_ids,
                known_provider_ids: registry,
                launch_at_login: legacy.launch_at_login,
            },
            true,
        ));
    }
    Err(<serde_json::Error as serde::de::Error>::custom(
        "unsupported or missing settings schema",
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsV1 {
    #[serde(rename = "schema")]
    _schema: u32,
    #[serde(default)]
    disabled_provider_ids: BTreeSet<String>,
    #[serde(default)]
    launch_at_login: bool,
}

fn provider_registry() -> BTreeSet<String> {
    [
        "antigravity",
        "claude",
        "codex",
        "copilot",
        "cursor",
        "devin",
        "grok",
        "opencode",
        "openrouter",
        "zai",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_corrupt_and_partial_states_are_deterministic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let (fresh, state) = store.load().await.unwrap();
        assert_eq!(state, SettingsLoadState::Fresh);
        assert!(fresh.enabled_provider_ids.contains("claude"));

        tokio::fs::write(&path, b"not-json").await.unwrap();
        let (_, state) = store.load().await.unwrap();
        assert_eq!(state, SettingsLoadState::RecoveredCorrupt);

        tokio::fs::write(&path, br#"{"schema":2}"#).await.unwrap();
        let (_, state) = store.load().await.unwrap();
        assert_eq!(state, SettingsLoadState::RecoveredCorrupt);
    }

    #[tokio::test]
    async fn v1_disabled_list_migrates_to_complete_enabled_list() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        tokio::fs::write(
            &path,
            br#"{"schema":1,"disabledProviderIds":["grok"],"launchAtLogin":true}"#,
        )
        .await
        .unwrap();

        let (settings, state) = SettingsStore::new(path).load().await.unwrap();

        assert_eq!(state, SettingsLoadState::Migrated);
        assert!(!settings.enabled_provider_ids.contains("grok"));
        assert!(settings.enabled_provider_ids.contains("claude"));
        assert!(settings.launch_at_login);
    }
}
