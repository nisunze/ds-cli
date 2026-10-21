//! Splicing refusal lists at compile time.
//!
//! A command's `REFUSALS` must enumerate every code it can actually emit —
//! that is the property `refusal_coverage.rs` holds the whole surface to. In
//! this domain the codes come from three places: loading sources, building
//! the request, and the command's own work. Restating them in each command
//! would be three copies of one fact, and the copies would drift the first
//! time a remedy was reworded.
//!
//! So each module owns its own list and commands splice. The length is a
//! const generic rather than inferred, which means a spliced list whose parts
//! no longer sum to the declared size fails the build with the assertion
//! below instead of silently carrying an empty placeholder into a help
//! screen.

use ds_cli_contract::spec::Refusal;

const PLACEHOLDER: Refusal = Refusal {
    code: "",
    when: "",
    remedy: "",
};

/// Concatenate refusal lists into one fixed-size array, in order.
///
/// `N` must equal the total length of `parts`. It is not inferred on purpose:
/// stating it is what makes a mismatch a compile-time failure rather than a
/// command that quietly advertises a blank refusal.
pub const fn splice<const N: usize>(parts: &[&[Refusal]]) -> [Refusal; N] {
    let mut out = [PLACEHOLDER; N];
    let mut written = 0;
    let mut part_index = 0;

    while part_index < parts.len() {
        let part = parts[part_index];
        let mut index = 0;
        while index < part.len() {
            out[written] = part[index];
            written += 1;
            index += 1;
        }
        part_index += 1;
    }

    assert!(
        written == N,
        "spliced refusal list does not match its declared length"
    );
    out
}
