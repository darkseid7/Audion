use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

const IDLE: u8 = 0;
const STOPPING: u8 = 1;
const READY: u8 = 2;

// Includes time waiting for an in-flight IPC command to release SqueezeState.
pub const APP_EXIT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, PartialEq, Eq)]
pub enum ExitAction {
    Allow,
    Prevent,
    StopPlayers(i32),
}

#[derive(Default)]
pub struct AppExit {
    phase: AtomicU8,
}

impl AppExit {
    pub fn on_event(&self, event: &tauri::RunEvent) -> ExitAction {
        match event {
            tauri::RunEvent::ExitRequested { code, .. } => self.request(*code),
            _ => ExitAction::Allow,
        }
    }

    pub fn request(&self, code: Option<i32>) -> ExitAction {
        match self
            .phase
            .compare_exchange(IDLE, STOPPING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                let code = code.unwrap_or(0);
                tracing::info!(
                    exit_code = code,
                    "Stopping Squeeze players before application exit"
                );
                ExitAction::StopPlayers(code)
            }
            Err(READY) => ExitAction::Allow,
            Err(_) => ExitAction::Prevent,
        }
    }

    pub async fn stop_players(&self, cleanup: impl Future<Output = ()>, limit: Duration) {
        // A cleanup panic must not leave all future exit requests pending forever.
        match tokio::time::timeout(limit, AssertUnwindSafe(cleanup).catch_unwind()).await {
            Ok(Ok(())) => tracing::info!("Squeeze cleanup completed before application exit"),
            Ok(Err(_)) => tracing::error!("Squeeze cleanup panicked; allowing application exit"),
            Err(_) => tracing::warn!("Squeeze cleanup timed out; allowing application exit"),
        }
        self.phase.store(READY, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::oneshot;

    // Catches allowing exit before cleanup, duplicate cleanup, and a lost exit code.
    #[tokio::test]
    async fn repeated_exit_waits_for_one_cleanup_before_allowing_final_exit() {
        let coordinator = Arc::new(AppExit::default());
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = oneshot::channel();

        let code = match coordinator.request(Some(7)) {
            ExitAction::StopPlayers(code) => code,
            action => panic!("First exit must start cleanup, got {action:?}"),
        };
        let worker = coordinator.clone();
        let calls = cleanup_calls.clone();
        let cleanup = tokio::spawn(async move {
            worker
                .stop_players(
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started_tx.send(()).unwrap();
                        finish_rx.await.unwrap();
                    },
                    Duration::from_secs(1),
                )
                .await;
            code
        });

        started_rx.await.unwrap();
        assert_eq!(coordinator.request(Some(99)), ExitAction::Prevent);
        assert_eq!(coordinator.request(None), ExitAction::Prevent);
        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);
        assert!(!cleanup.is_finished());

        finish_tx.send(()).unwrap();
        assert_eq!(cleanup.await.unwrap(), 7);
        assert_eq!(coordinator.request(Some(7)), ExitAction::Allow);
        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);
    }

    // Catches a timeout applied only inside stop(), excluding the managed-state lock.
    #[tokio::test]
    async fn blocked_server_mutex_does_not_hold_exit_forever() {
        let coordinator = AppExit::default();
        assert_eq!(coordinator.request(None), ExitAction::StopPlayers(0));
        let state = crate::commands::squeeze::SqueezeState::new();
        let server = state.0.clone();
        let held = state.0.lock().await;

        let cleanup = coordinator.stop_players(
            async move {
                server.lock().await.stop().await;
            },
            Duration::from_millis(50),
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), cleanup)
                .await
                .is_ok(),
            "Exit must fall back even while another IPC command holds the server mutex"
        );
        assert_eq!(coordinator.request(Some(0)), ExitAction::Allow);
        drop(held);
    }

    // Catches a panicking cleanup task leaving all subsequent exit requests pending.
    #[tokio::test]
    async fn cleanup_panic_still_allows_final_exit() {
        let coordinator = Arc::new(AppExit::default());
        assert_eq!(coordinator.request(Some(-9)), ExitAction::StopPlayers(-9));
        let worker = coordinator.clone();
        let cleanup = tokio::spawn(async move {
            worker
                .stop_players(
                    async { panic!("test cleanup panic") },
                    Duration::from_secs(1),
                )
                .await;
        });
        assert!(
            cleanup.await.is_ok(),
            "Cleanup panic must not strand graceful exit"
        );
        assert_eq!(coordinator.request(Some(-9)), ExitAction::Allow);
    }

    // Catches non-exit events starting cleanup; hiding the window does not request exit.
    #[test]
    fn non_exit_events_do_not_start_shutdown() {
        let coordinator = AppExit::default();
        let event = tauri::RunEvent::Ready;
        assert_eq!(coordinator.on_event(&event), ExitAction::Allow);
        assert_eq!(coordinator.request(None), ExitAction::StopPlayers(0));
    }

    // Catches an initial-request race starting multiple cleanup tasks.
    #[test]
    fn concurrent_exit_requests_start_cleanup_exactly_once() {
        let coordinator = Arc::new(AppExit::default());
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let requests: Vec<_> = (0..8)
            .map(|_| {
                let coordinator = coordinator.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    coordinator.request(Some(7))
                })
            })
            .collect();
        let actions: Vec<_> = requests
            .into_iter()
            .map(|request| request.join().unwrap())
            .collect();
        assert_eq!(
            actions
                .iter()
                .filter(|action| **action == ExitAction::StopPlayers(7))
                .count(),
            1
        );
        assert_eq!(
            actions
                .iter()
                .filter(|action| **action == ExitAction::Prevent)
                .count(),
            7
        );
    }
}
