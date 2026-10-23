//! Host-visible concurrency settings and the OS-bounded worker allowance.
//! Pool allocation and task execution remain with the calculation runtime.

/// The highest value the setting accepts.
pub const MAX_CONCURRENCY: usize = 1024;

/// How many cities may run at once.
///
/// One means one city at a time (nested chart work shares the CPU pool) and
/// which must produce byte-identical output to any other value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Concurrency(usize);

impl Concurrency {
    /// Clamp a user-supplied setting into the supported range.
    pub fn new(requested: usize) -> Self {
        Self(requested.clamp(1, MAX_CONCURRENCY))
    }

    /// Strictly serial.
    pub fn serial() -> Self {
        Self(1)
    }

    /// The value in force.
    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for Concurrency {
    fn default() -> Self {
        Self(compute_parallelism())
    }
}

/// OS-available workers, optionally reduced by a positive `DS_SOLAR_WORKER_THREADS`.
#[cfg(not(target_arch = "wasm32"))]
pub fn compute_parallelism() -> usize {
    worker_parallelism(
        std::thread::available_parallelism().map_or(1, usize::from),
        std::env::var("DS_SOLAR_WORKER_THREADS").ok().as_deref(),
    )
}

/// Browser workers execute one ordered computation at a time.
#[cfg(target_arch = "wasm32")]
pub fn compute_parallelism() -> usize {
    1
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn worker_parallelism(available: usize, requested: Option<&str>) -> usize {
    requested
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(available)
        .min(available)
        .max(1)
}

#[cfg(test)]
#[path = "scheduling_tests.rs"]
mod tests;
