//! The one tolerance a weak link gets.
//!
//! Field networks blink: a timeout, a reset connection, a gateway answering
//! 503 for the second it takes a backend to come back. A blink is not an
//! answer, so one blink must not end a sign-in, a token refresh, or a dataset
//! download. The pauses are the kernel's one retry curve
//! (`ds_command_kernel::retry`: 2, 6 and 15 seconds, then the last blink is
//! returned as it came) — the same pauses the resumable transfer and the
//! gateway response policy wait — so every native caller retries by the same
//! rule instead of each inventing one.
//!
//! What is retried is decided by the caller's own transient classification and
//! only ever for requests that are safe to send again. A definitive answer (a
//! 401/403, a validation refusal) and a non-idempotent write are never retried.
//!
//! A blink is quick. A failed attempt that waited longer than the schedule's
//! stall bound ([`WEAK_NETWORK_STALL`]) ran into a deadline, and asking again
//! would wait that whole deadline again: three more 120-second waits turned
//! one stalled transformer read into an eight-minute hang, and a curve wrapped
//! around a call that had already run its own curve multiplied the waits
//! again, so one `auth_transient` refusal took 10 to 30 minutes to arrive. A
//! stall is returned as it came, within one deadline, for the caller to name.
use std::time::{Duration, Instant};

use ds_command_kernel::retry::PAUSES_MS;

/// The pauses between attempts when the network blinks: the desktop's own
/// tolerance for a weak link, so a batch of eighty sheets does not lose
/// seventy rows to one dropped second.
pub const WEAK_NETWORK_DELAYS: [Duration; 3] = [
    Duration::from_millis(PAUSES_MS[0]),
    Duration::from_millis(PAUSES_MS[1]),
    Duration::from_millis(PAUSES_MS[2]),
];

/// The longest a failed attempt may wait and still count as a blink.
///
/// It sits above the native connect deadline (10 seconds), so a refused or
/// dropped connect is still asked again, and below the sum of the field pauses
/// (23 seconds), so a call that already spent a whole curve is itself a stall
/// to any curve wrapped around it: nested curves never multiply.
pub const WEAK_NETWORK_STALL: Duration = Duration::from_secs(20);

/// A bounded retry schedule: one pause before each retry, no retry after the
/// last pause is spent, and no retry of an attempt that stalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeakNetwork {
    delays: &'static [Duration],
    stall: Duration,
}

impl WeakNetwork {
    /// The field policy every native caller uses.
    pub const FIELD: Self = Self::new(&WEAK_NETWORK_DELAYS);

    /// A schedule with these pauses and the field stall bound. Hosts use
    /// [`Self::FIELD`]; a test uses zero-length pauses to prove the retry
    /// without waiting for it.
    pub const fn new(delays: &'static [Duration]) -> Self {
        Self {
            delays,
            stall: WEAK_NETWORK_STALL,
        }
    }

    /// This schedule with another stall bound. Hosts keep the field bound; a
    /// test shrinks it to prove a stall is not asked again without waiting
    /// for a real deadline.
    pub const fn with_stall(self, stall: Duration) -> Self {
        Self {
            delays: self.delays,
            stall,
        }
    }

    /// The most retries this schedule spends after the first attempt.
    pub const fn max_retries(&self) -> usize {
        self.delays.len()
    }

    /// The longest a failed attempt may wait and still be asked again.
    pub const fn stall(&self) -> Duration {
        self.stall
    }

    /// Run `attempt` until its outcome is not a blink, the attempt stalled,
    /// or the pauses run out.
    ///
    /// Returns the last outcome — success, a definitive refusal, a stall, or
    /// the final blink — with the number of retries spent on the way, so a
    /// caller can say honestly that it retried.
    pub fn run<T>(&self, mut attempt: impl FnMut() -> T, blinked: impl Fn(&T) -> bool) -> (T, u8) {
        let mut retries = 0_usize;
        loop {
            let started = Instant::now();
            let outcome = attempt();
            let stalled = started.elapsed() >= self.stall;
            if retries >= self.delays.len() || stalled || !blinked(&outcome) {
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
    fn a_blink_that_stalled_is_returned_without_asking_again() {
        // A read that waited out its deadline: asked again, it would wait the
        // whole deadline again, three more times.
        let stalls = WeakNetwork::new(&[Duration::ZERO; 3]).with_stall(Duration::from_millis(40));
        let calls = Cell::new(0);
        let (outcome, retries) = stalls.run(
            || {
                calls.set(calls.get() + 1);
                std::thread::sleep(Duration::from_millis(50));
                Err::<(), _>("timed out")
            },
            |outcome| outcome.is_err(),
        );
        assert_eq!((outcome, retries, calls.get()), (Err("timed out"), 0, 1));

        // The same schedule still asks a quick blink again.
        let calls = Cell::new(0);
        let (outcome, retries) = stalls.run(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 2 {
                    Err("reset")
                } else {
                    Ok("room")
                }
            },
            |outcome| outcome.is_err(),
        );
        assert_eq!((outcome, retries, calls.get()), (Ok("room"), 1, 2));
    }

    #[test]
    fn a_curve_wrapped_around_a_spent_curve_does_not_multiply_it() {
        // The field proportions in miniature: the inner pauses sum past the
        // outer stall bound, as 2 + 6 + 15 seconds sum past 20.
        static INNER_PAUSES: [Duration; 3] = [Duration::from_millis(5); 3];
        let inner = WeakNetwork::new(&INNER_PAUSES);
        let outer = WeakNetwork::new(&[Duration::ZERO; 3]).with_stall(Duration::from_millis(12));
        let calls = Cell::new(0);
        let (outcome, outer_retries) = outer.run(
            || {
                inner
                    .run(
                        || {
                            calls.set(calls.get() + 1);
                            Err::<(), _>("unreachable")
                        },
                        |outcome| outcome.is_err(),
                    )
                    .0
            },
            |outcome| outcome.is_err(),
        );
        // One inner curve of four attempts, never four curves of four.
        assert_eq!(
            (outcome, outer_retries, calls.get()),
            (Err("unreachable"), 0, 4)
        );
    }

    #[test]
    fn the_field_stall_bound_sits_below_one_spent_curve() {
        assert_eq!(WeakNetwork::FIELD.stall(), WEAK_NETWORK_STALL);
        assert_eq!(WEAK_NETWORK_STALL.as_secs(), 20);
        let spent: Duration = WEAK_NETWORK_DELAYS.iter().sum();
        assert!(spent > WeakNetwork::FIELD.stall(), "{spent:?}");
        // Every host and test schedule keeps the field bound unless it asks.
        assert_eq!(NOW.stall(), WEAK_NETWORK_STALL);
    }

    #[test]
    fn the_field_policy_is_the_kernels_one_retry_curve() {
        assert_eq!(WeakNetwork::FIELD.max_retries(), 3);
        assert_eq!(WEAK_NETWORK_DELAYS.map(|delay| delay.as_secs()), [2, 6, 15]);
        for (retry, delay) in (1u32..).zip(WEAK_NETWORK_DELAYS) {
            assert_eq!(
                u64::try_from(delay.as_millis()).unwrap(),
                ds_command_kernel::retry::pause_ms(retry)
            );
        }
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
