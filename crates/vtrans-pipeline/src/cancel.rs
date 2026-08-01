//! Per-frame task cancellation coordination.
//!
//! The live pipeline must never run more than one OCR pass or more than
//! one translation at the same time, and a newer frame must supersede an
//! older in-flight pass. [`TaskSlot`] encapsulates that pattern: it owns at
//! most one spawned task together with its [`CancellationToken`], cancels
//! and joins the previous task when a new one is started, and is the single
//! place where "cancel the previous, start the next" is implemented.
//!
//! The token is created by the slot and handed to the task through the
//! `build` closure, so callers cannot accidentally use a stale token.

use std::future::Future;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{instrument, warn};

/// Owns at most one in-flight task and its cancellation token.
///
/// [`replace`](Self::replace) cancels and joins the previous task before
/// starting the next one, which guarantees that only one task is running at
/// any point in time. Dropping the slot detaches the in-flight task; call
/// [`cancel_and_join`](Self::cancel_and_join) during shutdown to terminate
/// it.
///
/// # Example
///
/// ```no_run
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use std::sync::Arc;
/// use tokio::time::{sleep, Duration};
/// use vtrans_pipeline::cancel::TaskSlot;
///
/// #[tokio::main]
/// async fn main() {
///     let cancelled = Arc::new(AtomicBool::new(false));
///     let mut slot = TaskSlot::new();
///     slot.replace({
///         let cancelled = cancelled.clone();
///         move |cancel| async move {
///             tokio::select! {
///                 () = cancel.cancelled() => cancelled.store(true, Ordering::SeqCst),
///                 _ = sleep(Duration::from_secs(60)) => {}
///             }
///         }
///     })
///     .await;
///     assert!(slot.is_running());
///
///     // Replacing the slot cancels and joins the previous task.
///     slot.replace(|_| async {}).await;
///     assert!(cancelled.load(Ordering::SeqCst));
///     assert!(!slot.is_running());
/// }
/// ```
#[derive(Debug)]
pub struct TaskSlot<T> {
    cancel: Option<CancellationToken>,
    handle: Option<JoinHandle<T>>,
}

impl<T> TaskSlot<T> {
    /// Creates an empty slot.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancel: None,
            handle: None,
        }
    }

    /// Returns `true` when a task is currently in flight.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// Returns a clone of the cancellation token of the in-flight task, if
    /// any. Useful for observing cancellation from tests.
    #[must_use]
    pub fn cancel_token(&self) -> Option<CancellationToken> {
        self.cancel.clone()
    }

    /// Starts `build(token)` as the new in-flight task.
    ///
    /// The previous task, if any, is cancelled and awaited before the new
    /// task is spawned, so at most one task runs at any time. `build`
    /// receives the fresh [`CancellationToken`] that is cancelled when the
    /// task is superseded or the slot is shut down.
    #[instrument(skip_all)]
    pub async fn replace<F, Fut>(&mut self, build: F)
    where
        F: FnOnce(CancellationToken) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.cancel_and_join().await;
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(build(cancel.clone()));
        self.cancel = Some(cancel);
        self.handle = Some(handle);
    }

    /// Cancels the in-flight task (if any) and waits for it to terminate.
    ///
    /// This is safe to call when the slot is empty. A task that ignores its
    /// cancellation token can make this await forever; provider contracts
    /// require cooperative cancellation.
    #[instrument(skip_all)]
    pub async fn cancel_and_join(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        if let Some(handle) = self.handle.take() {
            if let Err(error) = handle.await {
                warn!(error = %error, "task terminated abnormally");
            }
        }
    }
}

impl<T> Default for TaskSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Builds a closure that blocks until its token is cancelled, recording
    /// cancellation into `cancelled`.
    fn blocking_task(
        cancelled: Arc<AtomicBool>,
    ) -> impl FnOnce(CancellationToken) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
    {
        move |cancel| {
            let cancelled = cancelled.clone();
            Box::pin(async move {
                tokio::select! {
                    () = cancel.cancelled() => cancelled.store(true, Ordering::SeqCst),
                    () = tokio::time::sleep(Duration::from_secs(600)) => {}
                }
            })
        }
    }

    #[tokio::test]
    async fn replace_cancels_and_joins_previous_task() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut slot: TaskSlot<()> = TaskSlot::new();
        slot.replace(blocking_task(cancelled.clone())).await;
        assert!(slot.is_running());

        // A new task supersedes the previous one; the slot now holds the
        // new task.
        slot.replace(|_| async {}).await;
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(slot.is_running());
        slot.cancel_and_join().await;
        assert!(!slot.is_running());
        assert!(slot.cancel_token().is_none());
    }

    #[tokio::test]
    async fn cancel_and_join_terminates_running_task() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut slot: TaskSlot<()> = TaskSlot::new();
        slot.replace(blocking_task(cancelled.clone())).await;
        assert!(slot.is_running());

        slot.cancel_and_join().await;
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(!slot.is_running());
        assert!(slot.cancel_token().is_none());
    }

    #[tokio::test]
    async fn cancel_and_join_on_empty_slot_is_noop() {
        let mut slot: TaskSlot<()> = TaskSlot::new();
        slot.cancel_and_join().await;
        assert!(!slot.is_running());
    }

    #[tokio::test]
    async fn replace_of_finished_task_is_cheap() {
        let mut slot = TaskSlot::new();
        slot.replace(|_| async { "first" }).await;
        tokio::task::yield_now().await;
        // The previous task has finished; replacing joins it immediately.
        slot.replace(|_| async { "second" }).await;
        assert!(slot.is_running());
        slot.cancel_and_join().await;
        assert!(!slot.is_running());
    }

    #[tokio::test]
    async fn at_most_one_task_runs_concurrently() {
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        let mut slot: TaskSlot<()> = TaskSlot::new();

        for _ in 0..8 {
            slot.replace({
                let concurrent = concurrent.clone();
                let max_concurrent = max_concurrent.clone();
                move |_| async move {
                    let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_concurrent.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .await;
        }
        slot.cancel_and_join().await;
        assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
    }
}
