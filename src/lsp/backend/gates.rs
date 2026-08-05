//! The two shared wakeup shapes the backend's async coordination is built
//! from: `ReadyGate` (register-before-recheck bounded wait) and
//! `DebouncedLatest` (generation-captured settle-window debounce). Each
//! proves its race-freedom invariant ONCE, here — a new wait/debounce site
//! composes these instead of re-spelling the discipline.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// A one-way latch + `Notify` for "some background completion will land".
/// Waiters arm via `armed_wait`, which registers interest on the `Notify`
/// BEFORE the final re-check — the lost-wakeup proof: an `open()` that fires
/// between the caller's first check and the await lands on the
/// already-registered `Notified` and wakes it, so a completion can never
/// slip through the gap. Callers keep their own probe (the external "done"
/// condition — doc present, map entry removed); the internal latch covers
/// the gate's own `open()`.
#[derive(Default)]
pub(super) struct ReadyGate {
    open: AtomicBool,
    notify: tokio::sync::Notify,
}

impl ReadyGate {
    /// Latch open and wake every armed waiter. Idempotent.
    pub(super) fn open(&self) {
        self.open.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub(super) fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    /// Arm a wait: returns the future to (bounded-)await when the gate is
    /// still closed and `probe` says the condition hasn't landed; `None`
    /// when no wait is needed. Interest is registered before `probe` runs —
    /// see the type doc for why that order is load-bearing.
    pub(super) fn armed_wait(
        &self,
        probe: impl FnOnce() -> bool,
    ) -> Option<impl std::future::Future<Output = ()> + '_> {
        let waited = self.notify.notified();
        if self.is_open() || probe() {
            return None;
        }
        Some(waited)
    }
}

/// Generation-captured settle-window debounce: every `fire` bumps the
/// generation and schedules its work after `settle`; only the fire that is
/// still the LATEST when its window elapses runs, so a burst collapses to
/// one execution. Long jobs re-probe `Latest::still` at their own
/// checkpoints (e.g. after an off-thread build) to drop superseded results.
#[derive(Default)]
pub(super) struct DebouncedLatest {
    generation: AtomicU64,
}

impl DebouncedLatest {
    /// Bump the generation and spawn `work` on `handle` after the settle
    /// window, iff no newer `fire` superseded it. `handle` is explicit
    /// because fires come from off-runtime threads too (the resolver
    /// thread's refresh callback).
    pub(super) fn fire<F, Fut>(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        settle: std::time::Duration,
        work: F,
    ) where
        F: FnOnce(Latest) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let gate = Arc::clone(self);
        handle.spawn(async move {
            tokio::time::sleep(settle).await;
            let latest = Latest { gate, generation };
            if !latest.still() {
                return; // a newer fire superseded this one
            }
            work(latest).await;
        });
    }
}

/// The surviving fire's re-check handle.
pub(super) struct Latest {
    gate: Arc<DebouncedLatest>,
    generation: u64,
}

impl Latest {
    /// Still the newest fire? Re-probe after any long await inside the work.
    pub(super) fn still(&self) -> bool {
        self.gate.generation.load(Ordering::Relaxed) == self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ReadyGate: the lost-wakeup proof, written once ----

    #[tokio::test]
    async fn open_before_arming_needs_no_wait() {
        let gate = ReadyGate::default();
        gate.open();
        assert!(gate.armed_wait(|| false).is_none(), "latched gate never waits");
    }

    #[tokio::test]
    async fn probe_hit_needs_no_wait() {
        let gate = ReadyGate::default();
        assert!(gate.armed_wait(|| true).is_none(), "landed condition never waits");
    }

    #[tokio::test]
    async fn open_after_arming_wakes_the_armed_future() {
        // The race the register-before-recheck order closes: open() fires
        // AFTER the waiter armed but BEFORE it awaits — the wakeup must not
        // be lost.
        let gate = Arc::new(ReadyGate::default());
        let waited = gate.armed_wait(|| false).expect("closed gate arms a wait");
        gate.open();
        tokio::time::timeout(std::time::Duration::from_secs(1), waited)
            .await
            .expect("an open() between arm and await must wake the waiter");
    }

    #[tokio::test]
    async fn concurrent_open_wakes_a_blocked_waiter() {
        let gate = Arc::new(ReadyGate::default());
        let waited = gate.armed_wait(|| false).expect("arms");
        let opener = {
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                gate.open();
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(1), waited)
            .await
            .expect("waiter wakes when the background completion lands");
        opener.await.unwrap();
    }

    // ---- DebouncedLatest ----

    #[tokio::test]
    async fn burst_collapses_to_the_latest_fire() {
        use std::sync::atomic::AtomicUsize;
        let debounce = Arc::new(DebouncedLatest::default());
        let runs = Arc::new(AtomicUsize::new(0));
        let handle = tokio::runtime::Handle::current();
        for _ in 0..10 {
            let runs = Arc::clone(&runs);
            debounce.fire(&handle, std::time::Duration::from_millis(30), move |_l| async move {
                runs.fetch_add(1, Ordering::Relaxed);
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(runs.load(Ordering::Relaxed), 1, "10 rapid fires run once");
    }

    #[tokio::test]
    async fn still_flips_when_a_newer_fire_lands_mid_work() {
        let debounce = Arc::new(DebouncedLatest::default());
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        let handle = tokio::runtime::Handle::current();
        let d2 = Arc::clone(&debounce);
        debounce.fire(&handle, std::time::Duration::from_millis(10), move |latest| async move {
            // A newer fire lands while this work runs; its own settle window
            // is still open, so THIS body observes the supersession.
            d2.generation.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(latest.still());
        });
        let still = tokio::time::timeout(std::time::Duration::from_secs(1), rx)
            .await
            .expect("work ran")
            .expect("sent");
        assert!(!still, "a mid-work supersession must be observable via still()");
    }

    #[tokio::test]
    async fn spaced_fires_each_run() {
        use std::sync::atomic::AtomicUsize;
        let debounce = Arc::new(DebouncedLatest::default());
        let runs = Arc::new(AtomicUsize::new(0));
        let handle = tokio::runtime::Handle::current();
        for _ in 0..2 {
            let runs = Arc::clone(&runs);
            debounce.fire(&handle, std::time::Duration::from_millis(10), move |_l| async move {
                runs.fetch_add(1, Ordering::Relaxed);
            });
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
        assert_eq!(runs.load(Ordering::Relaxed), 2, "settled fires each run");
    }
}
