//! The bound on reading one asset's bytes into memory, as a typed refusal.
//!
//! The client core refuses a larger asset with a plain sentence under its
//! invalid-input kind, which would surface as `auth_input_invalid` - the code
//! for a malformed project id - while ingest accepts far larger files.

use ds_cli_contract::outcome::Failure;
use serde_json::json;

/// The bound the client core enforces on a whole-asset read.
pub const READ_BOUND_BYTES: u64 = 32 * 1024 * 1024;

const SENTENCE: &str = "the asset is above the 32 MiB read bound";

/// The typed refusal for the client core's read-bound sentence, if it is one.
pub fn refusal(message: &str) -> Option<Failure> {
    message.starts_with(SENTENCE).then(|| {
        Failure::invalid(
            "asset_too_large",
            "the asset is above the 32 MiB bound for reading its bytes into memory",
        )
        .detail(json!({ "read_bound_bytes": READ_BOUND_BYTES, "asset_bytes": null }))
    })
}
