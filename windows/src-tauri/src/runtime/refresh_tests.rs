use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::contracts::ProviderSnapshot;

use super::cache::SnapshotCache;
use super::refresh::{
    ProviderFailure, ProviderRuntime, RefreshCoordinator, bounded_wake_channel, request_wake,
};

struct FakeRuntime {
    provider_id: String,
    calls: AtomicUsize,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    fail: bool,
}

#[async_trait]
impl ProviderRuntime for FakeRuntime {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn refresh(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(50)) => {},
            () = cancellation.cancelled() => {
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Err(ProviderFailure {
                    category: "other".to_owned(),
                    message: "refresh was cancelled".to_owned(),
                });
            }
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.fail {
            return Err(ProviderFailure {
                category: "network".to_owned(),
                message: "fixture failure".to_owned(),
            });
        }
        Ok(ProviderSnapshot {
            provider_id: self.provider_id.clone(),
            display_name: self.provider_id.clone(),
            plan: None,
            lines: Vec::new(),
            refreshed_at: chrono::DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339(),
            usage_history: None,
            warning: None,
            error_category: None,
        })
    }
}

fn runtime(provider_id: &str, fail: bool) -> Arc<FakeRuntime> {
    Arc::new(FakeRuntime {
        provider_id: provider_id.to_owned(),
        calls: AtomicUsize::new(0),
        active: Arc::new(AtomicUsize::new(0)),
        maximum: Arc::new(AtomicUsize::new(0)),
        fail,
    })
}

#[tokio::test]
async fn duplicate_forced_refreshes_are_coalesced() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = runtime("fixture", false);
    let coordinator = RefreshCoordinator::new(
        vec![runtime.clone()],
        SnapshotCache::empty(directory.path().join("cache.json")),
    );

    let (first, second) = tokio::join!(
        coordinator.refresh("fixture", true, CancellationToken::new()),
        coordinator.refresh("fixture", true, CancellationToken::new())
    );

    first.unwrap();
    second.unwrap();
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failure_keeps_last_good_and_backs_off() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.json");
    let good = runtime("fixture", false);
    let initial = RefreshCoordinator::new(vec![good], SnapshotCache::empty(path.clone()));
    initial
        .refresh("fixture", true, CancellationToken::new())
        .await
        .unwrap();
    let cache = SnapshotCache::load(path).await.unwrap();
    let failing = runtime("fixture", true);
    let coordinator = RefreshCoordinator::new(vec![failing.clone()], cache);

    let first = coordinator
        .refresh("fixture", false, CancellationToken::new())
        .await
        .unwrap();
    let second = coordinator
        .refresh("fixture", false, CancellationToken::new())
        .await
        .unwrap();

    assert!(first.snapshot.is_some());
    assert!(first.failure.is_some());
    assert!(second.snapshot.is_some());
    assert_eq!(failing.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refresh_all_never_exceeds_global_concurrency() {
    let directory = tempfile::tempdir().unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let runtimes = (0..8)
        .map(|index| {
            Arc::new(FakeRuntime {
                provider_id: format!("provider-{index}"),
                calls: AtomicUsize::new(0),
                active: active.clone(),
                maximum: maximum.clone(),
                fail: false,
            }) as Arc<dyn ProviderRuntime>
        })
        .collect::<Vec<_>>();
    let provider_ids = (0..8).map(|index| format!("provider-{index}")).collect();
    let coordinator = RefreshCoordinator::with_concurrency(
        runtimes,
        SnapshotCache::empty(directory.path().join("cache.json")),
        2,
    );

    coordinator
        .refresh_all(provider_ids, true, CancellationToken::new())
        .await;

    assert_eq!(maximum.load(Ordering::SeqCst), 2);
}

#[test]
fn wake_storm_is_bounded_to_one_pending_signal() {
    let (sender, mut receiver) = bounded_wake_channel();
    for _ in 0..1000 {
        request_wake(&sender);
    }

    assert!(receiver.try_recv().is_ok());
    assert!(receiver.try_recv().is_err());
}
