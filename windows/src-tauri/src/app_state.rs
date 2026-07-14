use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::contracts::ProviderSnapshot;
use crate::platform::environment::SystemEnvironment;
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::{DpapiSecretStore, SecretBytes};
use crate::providers::api_key::{ApiKeyStatus, ProtectedApiKeyStore};
use crate::runtime::cache::SnapshotCache;
use crate::runtime::settings::{Settings, SettingsStore, provider_order};

const SETTINGS_FILE: &str = "settings.json";
const SNAPSHOTS_FILE: &str = "snapshots.json";

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub refresh: bool,
    pub screenshot_export: bool,
    pub notifications: bool,
    pub launch_at_login: bool,
    pub global_shortcut: bool,
    pub external_links: bool,
    pub command_line: bool,
    pub updates: bool,
    pub logs: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresentation {
    pub id: &'static str,
    pub display_name: &'static str,
    pub quick_link: &'static str,
    pub supports_api_key: bool,
    pub api_key_status: Option<ApiKeyStatus>,
    pub api_key_warning: Option<&'static str>,
    pub snapshot: Option<ProviderSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub schema: &'static str,
    pub settings: Settings,
    pub providers: Vec<ProviderPresentation>,
    pub capabilities: PlatformCapabilities,
}

pub struct AppState {
    settings: RwLock<Settings>,
    settings_store: SettingsStore,
    settings_write_gate: Mutex<()>,
    snapshots: SnapshotCache,
    api_keys: HashMap<&'static str, ProtectedApiKeyStore>,
}

impl AppState {
    pub async fn load(paths: WindowsPaths) -> Result<Self, String> {
        let data_root = paths.openusage_data();
        let settings_store = SettingsStore::new(data_root.join(SETTINGS_FILE));
        let (settings, load_state) = settings_store.load().await.map_err(friendly_error)?;
        if !matches!(
            load_state,
            crate::runtime::settings::SettingsLoadState::Current
        ) {
            eprintln!("OpenUsage settings load state: {load_state:?}");
        }
        let snapshot_path = data_root.join(SNAPSHOTS_FILE);
        let snapshots = match SnapshotCache::load(snapshot_path.clone()).await {
            Ok(cache) => cache,
            Err(error) => {
                eprintln!("OpenUsage snapshot cache was ignored: {error}");
                SnapshotCache::empty(snapshot_path)
            }
        };
        let secrets = DpapiSecretStore::new(data_root.join("secrets"));
        let environment = Arc::new(SystemEnvironment);
        let api_keys = HashMap::from([
            (
                "openrouter",
                ProtectedApiKeyStore::new(
                    "openrouter",
                    &["OPENROUTER_API_KEY", "OPENROUTER_KEY"],
                    vec![
                        paths.user_profile.join(".config/openusage/openrouter.json"),
                        paths.user_profile.join(".config/openrouter/key.json"),
                    ],
                    environment.clone(),
                    secrets.clone(),
                ),
            ),
            (
                "zai",
                ProtectedApiKeyStore::new(
                    "zai",
                    &["ZAI_API_KEY", "GLM_API_KEY"],
                    vec![
                        paths.user_profile.join(".config/openusage/zai.json"),
                        paths.user_profile.join(".config/zai/key.json"),
                    ],
                    environment,
                    secrets,
                ),
            ),
        ]);
        Ok(Self {
            settings: RwLock::new(settings),
            settings_store,
            settings_write_gate: Mutex::new(()),
            snapshots,
            api_keys,
        })
    }

    pub async fn bootstrap(&self) -> Result<AppBootstrap, String> {
        let settings = self.settings.read().await.clone();
        let mut providers = Vec::with_capacity(PROVIDERS.len());
        for provider_id in provider_order() {
            let descriptor =
                descriptor(&provider_id).expect("settings registry matches UI registry");
            let (api_key_status, api_key_warning) = match self.api_keys.get(descriptor.id) {
                Some(store) => match store.status().await {
                    Ok(status) => (Some(status), None),
                    Err(error) => {
                        eprintln!("OpenUsage API key status failed: {error}");
                        (
                            None,
                            Some("Protected key status is temporarily unavailable."),
                        )
                    }
                },
                None => (None, None),
            };
            providers.push(ProviderPresentation {
                id: descriptor.id,
                display_name: descriptor.display_name,
                quick_link: descriptor.quick_link,
                supports_api_key: self.api_keys.contains_key(descriptor.id),
                api_key_status,
                api_key_warning,
                snapshot: self.snapshots.displayed(descriptor.id).await,
            });
        }
        Ok(AppBootstrap {
            schema: "openusage.app-bootstrap.v1",
            settings,
            providers,
            capabilities: PlatformCapabilities::default(),
        })
    }

    pub async fn save_settings(&self, settings: Settings) -> Result<AppBootstrap, String> {
        settings.validate().map_err(friendly_error)?;
        let _write_gate = self.settings_write_gate.lock().await;
        self.settings_store
            .save(&settings)
            .await
            .map_err(friendly_error)?;
        *self.settings.write().await = settings;
        self.bootstrap().await
    }

    pub async fn save_api_key(
        &self,
        provider_id: &str,
        mut key: String,
    ) -> Result<ApiKeyStatus, String> {
        let store = self.api_key_store(provider_id)?;
        let secret = SecretBytes::new(std::mem::take(&mut key).into_bytes());
        store.save(secret).await.map_err(friendly_error)
    }

    pub async fn delete_api_key(&self, provider_id: &str) -> Result<ApiKeyStatus, String> {
        self.api_key_store(provider_id)?
            .delete()
            .await
            .map_err(friendly_error)
    }

    fn api_key_store(&self, provider_id: &str) -> Result<&ProtectedApiKeyStore, String> {
        self.api_keys
            .get(provider_id)
            .ok_or_else(|| "This provider does not support app-managed API keys.".to_owned())
    }
}

struct ProviderDescriptor {
    id: &'static str,
    display_name: &'static str,
    quick_link: &'static str,
}

const PROVIDERS: [ProviderDescriptor; 10] = [
    ProviderDescriptor {
        id: "claude",
        display_name: "Claude",
        quick_link: "https://claude.ai/settings/usage",
    },
    ProviderDescriptor {
        id: "codex",
        display_name: "Codex",
        quick_link: "https://chatgpt.com/codex/settings/usage",
    },
    ProviderDescriptor {
        id: "cursor",
        display_name: "Cursor",
        quick_link: "https://cursor.com/dashboard",
    },
    ProviderDescriptor {
        id: "antigravity",
        display_name: "Antigravity",
        quick_link: "https://antigravity.google",
    },
    ProviderDescriptor {
        id: "copilot",
        display_name: "GitHub Copilot",
        quick_link: "https://github.com/settings/billing",
    },
    ProviderDescriptor {
        id: "devin",
        display_name: "Devin",
        quick_link: "https://app.devin.ai/settings/usage",
    },
    ProviderDescriptor {
        id: "grok",
        display_name: "Grok",
        quick_link: "https://grok.com",
    },
    ProviderDescriptor {
        id: "opencode",
        display_name: "OpenCode",
        quick_link: "https://opencode.ai",
    },
    ProviderDescriptor {
        id: "openrouter",
        display_name: "OpenRouter",
        quick_link: "https://openrouter.ai/settings/credits",
    },
    ProviderDescriptor {
        id: "zai",
        display_name: "Z.ai",
        quick_link: "https://z.ai/manage-apikey/apikey-list",
    },
];

fn descriptor(provider_id: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS.iter().find(|provider| provider.id == provider_id)
}

fn friendly_error(error: impl std::fmt::Display) -> String {
    eprintln!("OpenUsage application operation failed: {error}");
    "OpenUsage could not complete that operation. Check the local log for details.".to_owned()
}
