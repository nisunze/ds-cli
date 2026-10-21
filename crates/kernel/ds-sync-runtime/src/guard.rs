//! Process-safe boundary for native sync hosts.
//!
//! A host may schedule this work on its own blocking executor, but the kernel
//! owns the failure contract. Durable sync state remains in the store and a
//! panic is returned to the caller as an inspectable failure.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run one native sync operation and contain an unwinding panic at the shared
/// runtime boundary.
pub fn run_contained<T>(
    operation: &'static str,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| {
        Err(format!(
            "native sync runtime panicked during {operation}; work retained for recovery"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_success_from_the_native_operation() {
        assert_eq!(run_contained("test operation", || Ok(42)), Ok(42));
    }

    #[test]
    fn contains_a_panic_with_a_stable_recovery_error() {
        let error = run_contained("test operation", || -> Result<(), String> {
            panic!("fixture panic")
        })
        .expect_err("the panic is contained");
        assert_eq!(
            error,
            "native sync runtime panicked during test operation; work retained for recovery"
        );
    }
}
