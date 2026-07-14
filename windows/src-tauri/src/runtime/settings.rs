use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

use super::settings_model::CURRENT_SCHEMA;
pub use super::settings_model::{
    MetricLayout, Settings, SettingsError, provider_order, provider_registry,
};
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsLoadState {
    Fresh,
    Current,
    Migrated,
    RecoveredCorrupt,
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
        match migrate(&contents).and_then(|(settings, migrated)| {
            settings.validate().map_err(|error| {
                <serde_json::Error as serde::de::Error>::custom(error.to_string())
            })?;
            Ok((settings, migrated))
        }) {
            Ok((settings, migrated)) => {
                if migrated {
                    self.save(&settings).await?;
                }
                Ok((
                    settings,
                    if migrated {
                        SettingsLoadState::Migrated
                    } else {
                        SettingsLoadState::Current
                    },
                ))
            }
            Err(_) => {
                let settings = Settings::default();
                self.save(&settings).await?;
                Ok((settings, SettingsLoadState::RecoveredCorrupt))
            }
        }
    }

    pub async fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        settings.validate()?;
        self.files
            .write(&self.path, &serde_json::to_vec_pretty(settings)?)
            .await?;
        Ok(())
    }
}

fn migrate(contents: &[u8]) -> Result<(Settings, bool), serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(contents)?;
    match value.get("schema").and_then(serde_json::Value::as_u64) {
        Some(schema) if schema == CURRENT_SCHEMA as u64 => {
            serde_json::from_value(value).map(|settings| (settings, false))
        }
        Some(2) => {
            let legacy: SettingsV2 = serde_json::from_value(value)?;
            let settings = Settings {
                enabled_provider_ids: legacy.enabled_provider_ids,
                known_provider_ids: legacy.known_provider_ids,
                launch_at_login: legacy.launch_at_login,
                ..Settings::default()
            };
            Ok((settings, true))
        }
        Some(1) => {
            let legacy: SettingsV1 = serde_json::from_value(value)?;
            let settings = Settings {
                enabled_provider_ids: provider_registry()
                    .difference(&legacy.disabled_provider_ids)
                    .cloned()
                    .collect(),
                launch_at_login: legacy.launch_at_login,
                ..Settings::default()
            };
            Ok((settings, true))
        }
        _ => Err(<serde_json::Error as serde::de::Error>::custom(
            "unsupported schema",
        )),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsV2 {
    #[serde(rename = "schema")]
    _schema: u32,
    enabled_provider_ids: BTreeSet<String>,
    #[serde(default)]
    known_provider_ids: BTreeSet<String>,
    #[serde(default)]
    launch_at_login: bool,
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
        assert_eq!(
            store.load().await.unwrap().1,
            SettingsLoadState::RecoveredCorrupt
        );
        tokio::fs::write(&path, br#"{"schema":3}"#).await.unwrap();
        assert_eq!(
            store.load().await.unwrap().1,
            SettingsLoadState::RecoveredCorrupt
        );
    }

    #[tokio::test]
    async fn v2_choices_migrate_without_resetting_user_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        tokio::fs::write(&path, br#"{"schema":2,"enabledProviderIds":["grok"],"knownProviderIds":["grok"],"launchAtLogin":true}"#).await.unwrap();
        let (settings, state) = SettingsStore::new(path).load().await.unwrap();
        assert_eq!(state, SettingsLoadState::Migrated);
        assert_eq!(
            settings.enabled_provider_ids,
            BTreeSet::from(["grok".to_owned()])
        );
        assert!(settings.launch_at_login);
    }

    #[tokio::test]
    async fn semantically_invalid_current_settings_recover_to_safe_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut settings = Settings::default();
        settings.provider_order.pop();
        tokio::fs::write(&path, serde_json::to_vec(&settings).unwrap())
            .await
            .unwrap();

        let (recovered, state) = SettingsStore::new(path).load().await.unwrap();

        assert_eq!(state, SettingsLoadState::RecoveredCorrupt);
        assert_eq!(recovered.provider_order, provider_order());
    }

    #[test]
    fn invalid_layouts_are_rejected_before_persistence() {
        let mut settings = Settings::default();
        settings.metric_layouts.insert(
            "claude".to_owned(),
            MetricLayout {
                hidden_metric_ids: BTreeSet::from(["spend".to_owned()]),
                starred_metric_ids: BTreeSet::from(["spend".to_owned()]),
                ..MetricLayout::default()
            },
        );
        assert!(settings.validate().is_err());
    }
}
