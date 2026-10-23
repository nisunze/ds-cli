//! One CPU allowance for all Solar children owned by this native process.
use crate::cancel::CancelToken;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Budget {
    capacity: usize,
    used: Mutex<usize>,
    available: Condvar,
}
impl Budget {
    fn acquire(&self, workers: usize, cancel: &CancelToken) -> Result<Permit<'_>, String> {
        self.acquire_before(workers, cancel, None)
    }
    fn acquire_before(
        &self,
        workers: usize,
        cancel: &CancelToken,
        deadline: Option<Instant>,
    ) -> Result<Permit<'_>, String> {
        let workers = workers.clamp(1, self.capacity);
        let mut used = self
            .used
            .lock()
            .map_err(|_| "Solar worker budget is poisoned")?;
        loop {
            if cancel.is_cancelled() {
                return Err("Solar calculation cancelled while waiting for worker capacity".into());
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err("solar_process_deadline_exceeded_while_waiting_for_workers".into());
            }
            if workers <= self.capacity - *used {
                *used += workers;
                return Ok(Permit {
                    budget: self,
                    workers,
                });
            }
            // The token is deliberately just a shared flag. This bounded local
            // wait observes cancellation without an API call or progress file.
            used = self
                .available
                .wait_timeout(used, Duration::from_millis(50))
                .map_err(|_| "Solar worker budget is poisoned")?
                .0;
        }
    }
}

pub(crate) struct Permit<'a> {
    budget: &'a Budget,
    workers: usize,
}
impl Permit<'_> {
    pub(crate) fn workers(&self) -> usize {
        self.workers
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut used) = self.budget.used.lock() {
            *used -= self.workers;
            self.budget.available.notify_all();
        }
    }
}

fn budget() -> &'static Budget {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    BUDGET.get_or_init(|| Budget {
        capacity: crate::compute_parallelism(),
        used: Mutex::new(0),
        available: Condvar::new(),
    })
}
pub(crate) fn acquire(workers: usize, cancel: &CancelToken) -> Result<Permit<'static>, String> {
    budget().acquire(workers, cancel)
}
pub(crate) fn acquire_until(
    workers: usize,
    cancel: &CancelToken,
    deadline: Instant,
) -> Result<Permit<'static>, String> {
    budget().acquire_before(workers, cancel, Some(deadline))
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
