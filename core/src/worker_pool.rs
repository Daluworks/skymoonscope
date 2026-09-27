use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

type Task = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Receiver half of the pool's queue, shared by every worker.
type SharedReceiver = Arc<Mutex<mpsc::Receiver<Task>>>;

/// How often the supervisor checks whether its workers are still alive.
const SUPERVISOR_TICK: Duration = Duration::from_secs(1);
/// Delay before the first worker restart; it doubles per consecutive failure.
const RESTART_BACKOFF_BASE: Duration = Duration::from_secs(1);
/// Upper bound for the restart delay, so a permanently broken worker cannot
/// turn the supervisor into a hot restart loop.
const RESTART_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// A worker that stays up this long counts as recovered and forgets the
/// backoff it accumulated while it was failing.
const RESTART_BACKOFF_RESET_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub enum TaskQueueError {
    #[error("event worker queue is full")]
    Full,
    #[error("event worker queue is closed")]
    Closed,
}

/// Health bookkeeping for a single worker slot.
#[derive(Debug)]
struct WorkerSlot {
    /// `None` while the slot is down, waiting out its restart backoff.
    handle: Option<JoinHandle<()>>,
    /// Consecutive restarts that were not separated by a healthy run.
    failures: u32,
    /// Instant before which the next restart must not happen.
    restart_at: Option<tokio::time::Instant>,
    /// When the current worker was (re)started.
    started_at: tokio::time::Instant,
}

impl WorkerSlot {
    /// Wraps a freshly started worker.
    fn start(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
            failures: 0,
            restart_at: None,
            started_at: tokio::time::Instant::now(),
        }
    }

    /// Reaps the worker if it has finished, handing back its join handle so
    /// the caller can observe *how* it ended (panic vs. clean exit).
    ///
    /// While the worker is still alive this also clears the accumulated
    /// backoff once it has been healthy for [`RESTART_BACKOFF_RESET_AFTER`],
    /// so a pool that crashes once a day does not keep restarting with a
    /// long-since-saturated delay.
    fn take_finished(&mut self, now: tokio::time::Instant) -> Option<JoinHandle<()>> {
        let finished = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.is_finished());

        if !finished {
            if self.failures > 0
                && now.saturating_duration_since(self.started_at) >= RESTART_BACKOFF_RESET_AFTER
            {
                self.failures = 0;
            }
            return None;
        }

        self.handle.take()
    }
}

/// A dedicated thread pool configuration for heavy contract event processing.
/// This prevents CPU-intensive parsing from blocking the main HTTP async runtime.
///
/// A supervisor task keeps the pool at its configured size: a worker that
/// panics or returns early is observed with `JoinHandle::is_finished()`, reaped
/// and respawned with exponential backoff instead of silently shrinking the
/// pool until no queued job can make progress (issue #017).
#[derive(Clone)]
pub struct EventWorkerPool {
    runtime: Arc<Runtime>,
    sender: mpsc::Sender<Task>,
}

impl EventWorkerPool {
    /// Initializes a new dedicated Tokio runtime for event processing.
    ///
    /// # Arguments
    /// * `worker_threads` - The number of OS threads to allocate to this pool.
    pub fn new(worker_threads: usize) -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .thread_name("event-parser-worker")
            .enable_all()
            .build()?;

        // A zero-length channel panics, so keep at least one queue slot: a
        // pool created with no workers then applies back-pressure to callers
        // instead of panicking at construction time.
        let (sender, receiver) = mpsc::channel(worker_threads.max(1));
        let receiver: SharedReceiver = Arc::new(Mutex::new(receiver));

        let slots = (0..worker_threads)
            .map(|_| WorkerSlot::start(runtime.spawn(worker_loop(Arc::clone(&receiver)))))
            .collect();

        // The supervisor runs on this runtime and respawns workers with
        // `tokio::spawn`, so it needs no `Runtime` handle of its own: dropping
        // the last `EventWorkerPool` clone shuts the runtime down and takes
        // every task — supervisor included — with it.
        runtime.spawn(supervise(slots, receiver));

        Ok(Self {
            runtime: Arc::new(runtime),
            sender,
        })
    }

    /// Spawns an async task on the dedicated event worker pool.
    pub fn spawn<F>(
        &self,
        future: F,
    ) -> Result<tokio::task::JoinHandle<F::Output>, TaskQueueError>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (result_sender, result_receiver) = oneshot::channel();
        let task = async move {
            let result = future.await;
            let _ = result_sender.send(result);
        };
        self.sender
            .try_send(Box::pin(task))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => TaskQueueError::Full,
                mpsc::error::TrySendError::Closed(_) => TaskQueueError::Closed,
            })?;

        Ok(self.runtime.spawn(async move {
            result_receiver
                .await
                .expect("event worker task ended before returning its result")
        }))
    }

    /// Spawns a blocking (CPU-heavy) task on the dedicated event worker pool.
    /// Use this for strict, heavy synchronous parsing logic.
    pub fn spawn_blocking<F, R>(
        &self,
        func: F,
    ) -> Result<tokio::task::JoinHandle<R>, TaskQueueError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.spawn(async move { func() })
    }
}

/// Pulls tasks off the shared queue and runs them to completion.
///
/// The queue lock is held only while waiting for the *next* task, so workers
/// execute concurrently instead of serialising behind each other. A task that
/// panics unwinds through this loop and takes the worker with it — that is
/// deliberate: the supervisor replaces the worker, and the panic is still
/// reported rather than swallowed.
async fn worker_loop(receiver: SharedReceiver) {
    loop {
        let task = receiver.lock().await.recv().await;
        match task {
            Some(task) => task.await,
            None => break,
        }
    }
}

/// Watches the worker set and restarts workers that died.
///
/// Health is checked with `JoinHandle::is_finished()` once per
/// [`SUPERVISOR_TICK`]. A dead worker is reaped so its panic is reported, and
/// its replacement is started as soon as the exponential backoff for its
/// consecutive failures has elapsed. Because the supervisor wakes on a fixed
/// tick, restart delays are effectively rounded up to whole ticks.
async fn supervise(slots: Vec<WorkerSlot>, receiver: SharedReceiver) {
    let mut slots = slots;

    let mut ticker = tokio::time::interval(SUPERVISOR_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick fires immediately and the workers were just started, so
    // there is nothing to inspect yet.
    ticker.tick().await;

    loop {
        ticker.tick().await;
        let now = tokio::time::Instant::now();

        for (index, slot) in slots.iter_mut().enumerate() {
            let Some(finished) = slot.take_finished(now) else {
                continue;
            };

            match finished.await {
                Ok(()) => warn!(worker = index, "event worker exited; restarting"),
                Err(join_error) if join_error.is_panic() => error!(
                    worker = index,
                    error = %join_error,
                    "event worker panicked; restarting after backoff"
                ),
                Err(join_error) => error!(
                    worker = index,
                    error = %join_error,
                    "event worker did not finish cleanly; restarting after backoff"
                ),
            }

            slot.failures = slot.failures.saturating_add(1);
            let delay = restart_backoff(slot.failures);
            slot.restart_at = Some(now + delay);
            warn!(
                worker = index,
                consecutive_failures = slot.failures,
                delay_ms = delay.as_millis(),
                "scheduling event worker restart"
            );
        }

        for (index, slot) in slots.iter_mut().enumerate() {
            if slot.handle.is_some() {
                continue;
            }
            if matches!(slot.restart_at, Some(at) if now < at) {
                continue;
            }

            slot.handle = Some(tokio::spawn(worker_loop(Arc::clone(&receiver))));
            slot.started_at = now;
            slot.restart_at = None;
            info!(worker = index, "event worker restarted");
        }
    }
}

/// Exponential backoff for consecutive restarts, capped at
/// [`RESTART_BACKOFF_MAX`].
fn restart_backoff(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(16);
    RESTART_BACKOFF_BASE
        .saturating_mul(1u32 << exponent)
        .min(RESTART_BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_pool_initialization() {
        let pool_result = EventWorkerPool::new(2);
        assert!(
            pool_result.is_ok(),
            "Worker pool should initialize successfully"
        );
    }

    #[test]
    fn test_restart_backoff_grows_exponentially_and_is_capped() {
        assert_eq!(restart_backoff(0), RESTART_BACKOFF_BASE);
        assert_eq!(restart_backoff(1), RESTART_BACKOFF_BASE);
        assert_eq!(restart_backoff(2), RESTART_BACKOFF_BASE * 2);
        assert_eq!(restart_backoff(3), RESTART_BACKOFF_BASE * 4);
        assert_eq!(restart_backoff(4), RESTART_BACKOFF_BASE * 8);
        assert_eq!(restart_backoff(5), RESTART_BACKOFF_BASE * 16);
        assert_eq!(restart_backoff(6), RESTART_BACKOFF_MAX);
        assert_eq!(restart_backoff(u32::MAX), RESTART_BACKOFF_MAX);
    }

    #[test]
    fn test_backoff_is_forgotten_once_a_worker_has_been_healthy() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create test runtime");

        runtime.block_on(async {
            let mut slot = WorkerSlot {
                handle: None,
                failures: 5,
                restart_at: None,
                started_at: tokio::time::Instant::now(),
            };

            // Still inside the healthy window: the failure count is preserved.
            let _ = slot.take_finished(tokio::time::Instant::now());
            assert_eq!(slot.failures, 5);

            // Past the threshold with a live worker: backoff starts over.
            slot.handle = Some(tokio::spawn(async {}));
            let later = tokio::time::Instant::now() + RESTART_BACKOFF_RESET_AFTER;
            let _ = slot.take_finished(later);
            assert_eq!(slot.failures, 0);
        });
    }

    #[test]
    fn test_pool_spawns_async_task() {
        let pool = EventWorkerPool::new(2).expect("Failed to create worker pool");

        let result = pool.runtime.block_on(async {
            let handle = pool.spawn(async { 100 + 42 }).unwrap();
            handle.await.unwrap()
        });

        assert_eq!(
            result, 142,
            "Async task should execute and return correctly"
        );
    }

    #[test]
    fn test_pool_spawns_blocking_task() {
        let pool = EventWorkerPool::new(2).expect("Failed to create worker pool");

        let result = pool.runtime.block_on(async {
            let handle = pool.spawn_blocking(|| {
                // Simulate a heavy CPU-bound parsing task
                let mut sum = 0;
                for i in 1..=1000 {
                    sum += i;
                }
                sum
            }).unwrap();
            handle.await.unwrap()
        });

        assert_eq!(
            result, 500500,
            "Blocking CPU task should compute correctly off-thread"
        );
    }

    #[test]
    fn test_pool_rejects_tasks_when_queue_is_full() {
        let pool = EventWorkerPool::new(1).expect("Failed to create worker pool");

        pool.runtime.block_on(async {
            let (started_sender, started_receiver) = oneshot::channel();
            let (release_sender, release_receiver) = oneshot::channel();
            let first = pool
                .spawn(async move {
                    let _ = started_sender.send(());
                    let _ = release_receiver.await;
                })
                .unwrap();
            started_receiver.await.unwrap();
            let _queued = pool.spawn(async {}).unwrap();

            assert!(matches!(pool.spawn(async {}), Err(TaskQueueError::Full)));
            let _ = release_sender.send(());
            first.await.unwrap();
        });
    }

    #[test]
    fn test_pool_respawns_a_worker_that_panicked() {
        let pool = EventWorkerPool::new(1).expect("Failed to create worker pool");

        pool.runtime.block_on(async {
            // The pool has exactly one worker and this task takes it down.
            let dying = pool.spawn(async { panic!("worker boom") }).unwrap();
            assert!(
                dying.await.is_err(),
                "a panicking task must surface as a JoinError"
            );

            // Before the fix the pool stayed empty forever, so this task would
            // sit in the queue until the process exited.
            let recovered = tokio::time::timeout(
                Duration::from_secs(30),
                pool.spawn(async { "worker is back" }).unwrap(),
            )
            .await
            .expect("the supervisor must respawn the dead worker");

            assert_eq!(recovered.unwrap(), "worker is back");
        });
    }
}
