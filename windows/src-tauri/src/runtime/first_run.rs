use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use tokio::sync::Mutex;

const DETECTION_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CredentialPresence {
    Available,
    Missing,
    Inaccessible,
}

#[async_trait]
pub trait CredentialProbe: Send + Sync {
    async fn presence(&self) -> CredentialPresence;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnablementSnapshot {
    pub enabled: BTreeSet<String>,
    pub known: BTreeSet<String>,
    revision: u64,
}

#[derive(Clone)]
pub struct ProviderEnablement {
    state: Arc<Mutex<EnablementSnapshot>>,
}

impl ProviderEnablement {
    pub fn new(enabled: BTreeSet<String>, known: BTreeSet<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(EnablementSnapshot {
                enabled,
                known,
                revision: 0,
            })),
        }
    }

    pub async fn snapshot(&self) -> EnablementSnapshot {
        self.state.lock().await.clone()
    }

    pub async fn set_by_user(&self, provider_id: &str, enabled: bool) {
        let mut state = self.state.lock().await;
        if enabled {
            state.enabled.insert(provider_id.to_owned());
        } else {
            state.enabled.remove(provider_id);
        }
        state.known.insert(provider_id.to_owned());
        state.revision = state.revision.saturating_add(1);
    }

    async fn seed_fallback(&self, registry: &BTreeSet<String>) -> u64 {
        let mut state = self.state.lock().await;
        if state.known.is_empty() {
            state.enabled = ["claude", "codex", "cursor"]
                .into_iter()
                .map(str::to_owned)
                .collect();
            state.known = registry.clone();
        }
        state.revision
    }

    async fn seed_detected(&self, detected: BTreeSet<String>, expected_revision: u64) {
        let mut state = self.state.lock().await;
        if state.revision == expected_revision {
            state.enabled = detected;
        }
    }
}

pub async fn seed_first_run(
    enablement: &ProviderEnablement,
    probes: BTreeMap<String, Arc<dyn CredentialProbe>>,
) -> EnablementSnapshot {
    let registry = probes.keys().cloned().collect();
    let revision = enablement.seed_fallback(&registry).await;
    let detected = stream::iter(probes)
        .map(|(provider_id, probe)| async move { (provider_id, probe.presence().await) })
        .buffer_unordered(DETECTION_CONCURRENCY)
        .filter_map(|(provider_id, presence)| async move {
            (presence == CredentialPresence::Available).then_some(provider_id)
        })
        .collect::<BTreeSet<_>>()
        .await;
    enablement.seed_detected(detected, revision).await;
    enablement.snapshot().await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Probe {
        presence: CredentialPresence,
        delay_ms: u64,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl CredentialProbe for Probe {
        async fn presence(&self) -> CredentialPresence {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.presence
        }
    }

    fn probes(delay_ms: u64) -> (BTreeMap<String, Arc<dyn CredentialProbe>>, Arc<AtomicUsize>) {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let probes = (0..10)
            .map(|index| {
                let probe: Arc<dyn CredentialProbe> = Arc::new(Probe {
                    presence: if index == 2 {
                        CredentialPresence::Available
                    } else {
                        CredentialPresence::Missing
                    },
                    delay_ms,
                    active: active.clone(),
                    maximum: maximum.clone(),
                });
                (format!("provider-{index}"), probe)
            })
            .collect();
        (probes, maximum)
    }

    #[tokio::test]
    async fn first_run_detection_enables_only_locally_available_providers() {
        let enablement = ProviderEnablement::new(BTreeSet::new(), BTreeSet::new());

        let (probes, maximum) = probes(1);
        let result = seed_first_run(&enablement, probes).await;

        assert_eq!(result.enabled, ["provider-2".to_owned()].into());
        assert_eq!(result.known.len(), 10);
        assert!(maximum.load(Ordering::SeqCst) <= DETECTION_CONCURRENCY);
    }

    #[tokio::test]
    async fn user_toggle_during_detection_wins() {
        let enablement = ProviderEnablement::new(BTreeSet::new(), BTreeSet::new());
        let (probes, _) = probes(100);
        let seed_task = {
            let enablement = enablement.clone();
            async move { seed_first_run(&enablement, probes).await }
        };
        let toggle_task = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            enablement.set_by_user("provider-9", true).await;
        };

        let (result, ()) = tokio::join!(seed_task, toggle_task);

        assert!(result.enabled.contains("provider-9"));
    }
}
