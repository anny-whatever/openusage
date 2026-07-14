use std::collections::HashMap;
use std::sync::{Arc, Mutex as StandardMutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, stream};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

use crate::contracts::ProviderSnapshot;

use super::cache::SnapshotCache;

const DEFAULT_CONCURRENCY: usize = 4;
const FAILURE_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderFailure {
    pub category: String,
    pub message: String,
}

#[async_trait]
pub trait ProviderRuntime: Send + Sync {
    fn provider_id(&self) -> &str;
    async fn refresh(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderFailure>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutcome {
    pub snapshot: Option<ProviderSnapshot>,
    pub failure: Option<ProviderFailure>,
    pub from_cache: bool,
}

impl RefreshOutcome {
    fn cached(snapshot: ProviderSnapshot) -> Self {
        Self {
            snapshot: Some(snapshot),
            failure: None,
            from_cache: true,
        }
    }
}

#[derive(Default)]
struct SlotState {
    generation: u64,
    last_outcome: Option<RefreshOutcome>,
    failed_at: Option<Instant>,
}

struct RefreshSlot {
    gate: Mutex<()>,
    state: StandardMutex<SlotState>,
}

impl RefreshSlot {
    fn new() -> Self {
        Self {
            gate: Mutex::new(()),
            state: StandardMutex::new(SlotState::default()),
        }
    }
}

#[derive(Clone)]
pub struct RefreshCoordinator {
    runtimes: Arc<HashMap<String, Arc<dyn ProviderRuntime>>>,
    slots: Arc<HashMap<String, Arc<RefreshSlot>>>,
    semaphore: Arc<Semaphore>,
    cache: SnapshotCache,
}

impl RefreshCoordinator {
    pub fn new(runtimes: Vec<Arc<dyn ProviderRuntime>>, cache: SnapshotCache) -> Self {
        Self::with_concurrency(runtimes, cache, DEFAULT_CONCURRENCY)
    }

    pub fn with_concurrency(
        runtimes: Vec<Arc<dyn ProviderRuntime>>,
        cache: SnapshotCache,
        concurrency: usize,
    ) -> Self {
        assert!(concurrency > 0, "refresh concurrency must be positive");
        let runtimes = runtimes
            .into_iter()
            .map(|runtime| (runtime.provider_id().to_owned(), runtime))
            .collect::<HashMap<_, _>>();
        let slots = runtimes
            .keys()
            .map(|provider_id| (provider_id.clone(), Arc::new(RefreshSlot::new())))
            .collect();
        Self {
            runtimes: Arc::new(runtimes),
            slots: Arc::new(slots),
            semaphore: Arc::new(Semaphore::new(concurrency)),
            cache,
        }
    }

    pub async fn refresh(
        &self,
        provider_id: &str,
        force: bool,
        cancellation: CancellationToken,
    ) -> Result<RefreshOutcome, ProviderFailure> {
        let runtime = self
            .runtimes
            .get(provider_id)
            .ok_or_else(|| ProviderFailure {
                category: "not_available".to_owned(),
                message: "provider is not registered".to_owned(),
            })?;
        let slot = self
            .slots
            .get(provider_id)
            .expect("every runtime has a slot");
        let observed_generation = slot.state.lock().unwrap().generation;

        let gate = tokio::select! {
            gate = slot.gate.lock() => gate,
            () = cancellation.cancelled() => return Err(cancelled_failure()),
        };
        let _gate = gate;
        if let Some(coalesced) = coalesced_outcome(slot, observed_generation) {
            return Ok(coalesced);
        }
        if !force {
            let now = chrono::DateTime::<Utc>::from(std::time::SystemTime::now());
            if let Some(snapshot) = self.cache.fresh(provider_id, now).await {
                return Ok(complete_slot(slot, RefreshOutcome::cached(snapshot), None));
            }
            if let Some(outcome) = backed_off_outcome(slot) {
                return Ok(complete_slot(slot, outcome, None));
            }
        }

        let permit = tokio::select! {
            permit = self.semaphore.acquire() => permit.expect("semaphore remains open"),
            () = cancellation.cancelled() => return Err(cancelled_failure()),
        };
        let result = runtime.refresh(cancellation.clone()).await;
        drop(permit);
        if cancellation.is_cancelled() {
            return Err(cancelled_failure());
        }
        let outcome = match result {
            Ok(snapshot) => match self.cache.store(snapshot.clone()).await {
                Ok(()) => RefreshOutcome {
                    snapshot: Some(snapshot),
                    failure: None,
                    from_cache: false,
                },
                Err(error) => RefreshOutcome {
                    snapshot: Some(snapshot),
                    failure: Some(ProviderFailure {
                        category: "other".to_owned(),
                        message: format!("snapshot persistence failed: {error}"),
                    }),
                    from_cache: false,
                },
            },
            Err(failure) => RefreshOutcome {
                snapshot: self.cache.displayed(provider_id).await,
                failure: Some(failure),
                from_cache: false,
            },
        };
        let failed_at = outcome.failure.as_ref().map(|_| Instant::now());
        Ok(complete_slot(slot, outcome, failed_at))
    }

    pub async fn refresh_all(
        &self,
        provider_ids: Vec<String>,
        force: bool,
        cancellation: CancellationToken,
    ) -> HashMap<String, Result<RefreshOutcome, ProviderFailure>> {
        stream::iter(provider_ids)
            .map(|provider_id| {
                let coordinator = self.clone();
                let cancellation = cancellation.child_token();
                async move {
                    let result = coordinator.refresh(&provider_id, force, cancellation).await;
                    (provider_id, result)
                }
            })
            .buffer_unordered(self.semaphore.available_permits().max(1))
            .collect()
            .await
    }
}

fn coalesced_outcome(slot: &RefreshSlot, observed_generation: u64) -> Option<RefreshOutcome> {
    let state = slot.state.lock().unwrap();
    (state.generation > observed_generation)
        .then(|| state.last_outcome.clone())
        .flatten()
}

fn backed_off_outcome(slot: &RefreshSlot) -> Option<RefreshOutcome> {
    let state = slot.state.lock().unwrap();
    let failed_at = state.failed_at?;
    if failed_at.elapsed() < FAILURE_BACKOFF {
        state.last_outcome.clone()
    } else {
        None
    }
}

fn complete_slot(
    slot: &RefreshSlot,
    outcome: RefreshOutcome,
    failed_at: Option<Instant>,
) -> RefreshOutcome {
    let mut state = slot.state.lock().unwrap();
    state.generation = state.generation.saturating_add(1);
    state.failed_at = failed_at.or(if outcome.failure.is_none() {
        None
    } else {
        state.failed_at
    });
    state.last_outcome = Some(outcome.clone());
    outcome
}

fn cancelled_failure() -> ProviderFailure {
    ProviderFailure {
        category: "other".to_owned(),
        message: "refresh was cancelled".to_owned(),
    }
}

pub fn bounded_wake_channel() -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
    mpsc::channel(1)
}

pub fn request_wake(sender: &mpsc::Sender<()>) {
    let _ = sender.try_send(());
}
