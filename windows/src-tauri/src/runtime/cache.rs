use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::contracts::{ProviderSnapshot, Validate};
use crate::platform::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};

pub const SNAPSHOT_TTL: Duration = Duration::from_secs(5 * 60);
const CACHE_SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotCacheError {
    #[error("snapshot cache I/O failed: {0}")]
    File(#[from] FileStoreError),
    #[error("snapshot cache is invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotPayload {
    schema: u32,
    snapshots: HashMap<String, ProviderSnapshot>,
}

#[derive(Debug, Default)]
struct SnapshotState {
    snapshots: HashMap<String, ProviderSnapshot>,
    session_writes: HashSet<String>,
}

#[derive(Clone)]
pub struct SnapshotCache {
    path: PathBuf,
    files: AtomicFileStore,
    state: Arc<Mutex<SnapshotState>>,
    persist_gate: Arc<Mutex<()>>,
}

impl SnapshotCache {
    pub async fn load(path: PathBuf) -> Result<Self, SnapshotCacheError> {
        let files = AtomicFileStore::default();
        let snapshots = match files.read(&path).await {
            Ok(contents) => decode_payload(&contents)?,
            Err(FileStoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                HashMap::new()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            files,
            state: Arc::new(Mutex::new(SnapshotState {
                snapshots,
                session_writes: HashSet::new(),
            })),
            persist_gate: Arc::new(Mutex::new(())),
        })
    }

    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            files: AtomicFileStore::default(),
            state: Arc::new(Mutex::new(SnapshotState::default())),
            persist_gate: Arc::new(Mutex::new(())),
        }
    }

    pub async fn displayed(&self, provider_id: &str) -> Option<ProviderSnapshot> {
        self.state.lock().await.snapshots.get(provider_id).cloned()
    }

    pub async fn fresh(&self, provider_id: &str, now: DateTime<Utc>) -> Option<ProviderSnapshot> {
        let state = self.state.lock().await;
        if !state.session_writes.contains(provider_id) {
            return None;
        }
        let snapshot = state.snapshots.get(provider_id)?;
        let refreshed_at = DateTime::parse_from_rfc3339(&snapshot.refreshed_at)
            .ok()?
            .with_timezone(&Utc);
        let age = now.signed_duration_since(refreshed_at);
        if age.num_seconds() >= 0 && age.to_std().ok()? < SNAPSHOT_TTL {
            Some(snapshot.clone())
        } else {
            None
        }
    }

    pub async fn store(&self, mut snapshot: ProviderSnapshot) -> Result<(), SnapshotCacheError> {
        snapshot
            .validate()
            .map_err(|error| SnapshotCacheError::Invalid(error.to_string()))?;
        if snapshot.error_category.is_some() {
            return Ok(());
        }

        {
            let mut state = self.state.lock().await;
            if snapshot.usage_history.is_none() {
                snapshot.usage_history = state
                    .snapshots
                    .get(&snapshot.provider_id)
                    .and_then(|previous| previous.usage_history.clone());
            }
            state.session_writes.insert(snapshot.provider_id.clone());
            state
                .snapshots
                .insert(snapshot.provider_id.clone(), snapshot);
        }
        let _persist_gate = self.persist_gate.lock().await;
        let payload = SnapshotPayload {
            schema: CACHE_SCHEMA,
            snapshots: self.state.lock().await.snapshots.clone(),
        };
        let encoded = serde_json::to_vec(&payload)
            .map_err(|error| SnapshotCacheError::Invalid(error.to_string()))?;
        self.files.write(&self.path, &encoded).await?;
        Ok(())
    }
}

fn decode_payload(
    contents: &[u8],
) -> Result<HashMap<String, ProviderSnapshot>, SnapshotCacheError> {
    let payload: SnapshotPayload = serde_json::from_slice(contents)
        .map_err(|error| SnapshotCacheError::Invalid(error.to_string()))?;
    if payload.schema != CACHE_SCHEMA {
        return Err(SnapshotCacheError::Invalid(format!(
            "unsupported cache schema {}",
            payload.schema
        )));
    }
    for (provider_id, snapshot) in &payload.snapshots {
        snapshot
            .validate()
            .map_err(|error| SnapshotCacheError::Invalid(error.to_string()))?;
        if provider_id != &snapshot.provider_id {
            return Err(SnapshotCacheError::Invalid(
                "snapshot key does not match providerId".to_owned(),
            ));
        }
    }
    Ok(payload.snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ProviderSnapshot;

    fn snapshot(refreshed_at: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            provider_id: "fixture".to_owned(),
            display_name: "Fixture".to_owned(),
            plan: None,
            lines: Vec::new(),
            refreshed_at: refreshed_at.to_owned(),
            usage_history: None,
            warning: None,
            error_category: None,
        }
    }

    #[tokio::test]
    async fn persisted_snapshot_displays_but_never_suppresses_first_session_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshots.json");
        let cache = SnapshotCache::empty(path.clone());
        cache.store(snapshot("2026-07-14T12:00:00Z")).await.unwrap();

        let next_session = SnapshotCache::load(path).await.unwrap();
        let now = "2026-07-14T12:01:00Z".parse().unwrap();

        assert!(next_session.displayed("fixture").await.is_some());
        assert!(next_session.fresh("fixture", now).await.is_none());
    }

    #[tokio::test]
    async fn session_write_is_fresh_for_exactly_five_minutes() {
        let directory = tempfile::tempdir().unwrap();
        let cache = SnapshotCache::empty(directory.path().join("snapshots.json"));
        cache.store(snapshot("2026-07-14T12:00:00Z")).await.unwrap();

        assert!(
            cache
                .fresh("fixture", "2026-07-14T12:04:59Z".parse().unwrap())
                .await
                .is_some()
        );
        assert!(
            cache
                .fresh("fixture", "2026-07-14T12:05:00Z".parse().unwrap())
                .await
                .is_none()
        );
    }
}
