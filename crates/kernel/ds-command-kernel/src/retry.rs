//! The one retry curve: how long any caller waits before sending a request
//! again after the network blinked.
//!
//! Field networks blink — a timeout, a reset connection, a gateway answering
//! 503 for the second it takes a backend to come back — and a blink is not an
//! answer. Three pauses of 2, 6 and 15 seconds are the tolerance a weak link
//! gets: the report export's, then every native call's through
//! `ds_client_core::WeakNetwork`. The resumable transfer and the gateway
//! response policy read the same pauses, and a caller whose own bound allows
//! more attempts waits the last pause again. There is no second curve.
//!
//! Pure: no clock and no sleep, so the transfer machine and the response
//! policy compute from it in WASM and the native client sleeps it.

/// The pauses before the first, second and third retry, in milliseconds.
pub const PAUSES_MS: [u64; 3] = [2_000, 6_000, 15_000];

/// The pause before retry `retry` (the first retry is 1; 0 reads as 1). Past
/// the last pause the last one holds.
pub const fn pause_ms(retry: u32) -> u64 {
    let last = PAUSES_MS.len() - 1;
    let index = if retry == 0 { 0 } else { retry as usize - 1 };
    PAUSES_MS[if index > last { last } else { index }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_pauses_then_the_last_one_holds() {
        let curve: Vec<u64> = (0..=8).map(pause_ms).collect();
        assert_eq!(
            curve,
            [2_000, 2_000, 6_000, 15_000, 15_000, 15_000, 15_000, 15_000, 15_000]
        );
        assert_eq!(pause_ms(u32::MAX), 15_000);
    }
}
