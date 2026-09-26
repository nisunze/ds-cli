//! The one tolerance a weak link gets.
//!
//! Field networks blink: a timeout, a reset connection, a gateway answering
//! 503 for the second it takes a backend to come back. A blink is not an
//! answer, so one blink must not end a sign-in, a token refresh, or a dataset
//! download. This is the policy report export already applied (three pauses of
//! 2, 6 and 15 seconds, then the last blink is returned as it came), moved
//! here so every native caller retries by the same rule instead of each
//! inventing one.
//!
//! What is retried is decided by the caller's own transient classification and
//! only ever for requests that are safe to send again. A definitive answer (a
//! 401/403, a validation refusal) and a non-idempotent write are never retried.
use std::time::Duration;

/// The pauses between attempts when the network blinks: the desktop's own
/// tolerance for a weak link, so a batch of eighty sheets does not lose
/// seventy rows to one dropped second.
pub const WEAK_NETWORK_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(6),
    Duration::from_secs(15),
];

/// A bounded retry schedule: one pause before each retry, and no retry after
/// the last pause is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeakNetwork {
    delays: &'static [Duration],
}

impl WeakNetwork {
    /// The field policy every native caller uses.
    pub const FIELD: Self = Self::new(&WEAK_NETWORK_DELAYS);

    /// A schedule with these pauses. Hosts use [`Self::FIELD`]; a test uses
    /// zero-length pauses to prove the retry without waiting for it.
    pub const fn new(delays: &'static [Duration]) -> Self {
        Self { delays }
    }

    /// The most retries this schedule spends after the first attempt.
    pub const fn max_retries(&self) -> usize {
        self.delays.len()
    }

    /// Run `attempt` until its outcome is not a blink or the pauses run out.
    ///
    /// Returns the last outcome — success, a definitive refusal, or the final
    /// blink — with the number of retries spent on the way, so a caller can
    /// say honestly that it retried.
    pub fn run<T>(&self, mut attempt: impl FnMut() -> T, blinked: impl Fn(&T) -> bool) -> (T, u8) {
        let mut retries = 0_usize;
        loop {
            let outcome = attempt();
            if retries >= self.delays.len() || !blinked(&outcome) {
                return (outcome, u8::try_from(retries).unwrap_or(u8::MAX));
            }
            std::thread::sleep(self.delays[retries]);
            retries += 1;
        }
    }
}

/// An HTTP status that says "not now" rather than "no": a request timeout,
/// too early, too many requests, or any server-side failure. The same set the
/// authentication classifier already treats as transient.
pub const fn transient_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const NOW: WeakNetwork = WeakNetwork::new(&[Duration::ZERO; 3]);

    #[test]
    fn a_blink_is_retried_but_an_answer_is_not() {
        let calls = Cell::new(0);
        let (outcome, retries) = NOW.run(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err("blink")
                } else {
                    Ok("room")
                }
            },
            |outcome| outcome.is_err(),
        );
        assert_eq!((outcome, retries, calls.get()), (Ok("room"), 2, 3));

        let calls = Cell::new(0);
        let (outcome, retries) = NOW.run(
            || {
                calls.set(calls.get() + 1);
                Err::<(), _>("refused")
            },
            |_| false,
        );
        assert_eq!((outcome, retries, calls.get()), (Err("refused"), 0, 1));
    }

    #[test]
    fn four_blinks_in_a_row_is_an_outage_and_the_last_blink_comes_back() {
        let calls = Cell::new(0);
        let (outcome, retries) = NOW.run(
            || {
                calls.set(calls.get() + 1);
                Err::<(), _>(calls.get())
            },
            |outcome| outcome.is_err(),
        );
        assert_eq!((outcome, retries, calls.get()), (Err(4), 3, 4));
    }

    #[test]
    fn the_field_policy_is_the_report_export_schedule() {
        assert_eq!(WeakNetwork::FIELD.max_retries(), 3);
        assert_eq!(WEAK_NETWORK_DELAYS.map(|delay| delay.as_secs()), [2, 6, 15]);
    }

    #[test]
    fn only_not_now_statuses_are_transient() {
        for status in [408, 425, 429, 500, 502, 503, 504] {
            assert!(transient_status(status), "{status}");
        }
        for status in [200, 201, 400, 401, 403, 404, 409, 410, 422] {
            assert!(!transient_status(status), "{status}");
        }
    }
}
