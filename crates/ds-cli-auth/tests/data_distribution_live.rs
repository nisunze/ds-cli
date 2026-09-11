//! Live proof of the seeding door, run on demand and never in CI.
//!
//! It needs a generated development catalogue (the same shape
//! `run-linux-server.sh` builds through `scripts/run/server-profile.mjs`) in
//! `DS_NATIVE_CLIENT_PROFILE_BUNDLE`, and a signed-in native identity for the
//! lane under `DS_LIVE_LANE` (default `stable`) with a selected project:
//!
//! ```text
//! DS_NATIVE_CLIENT_PROFILE_BUNDLE=/abs/catalog.json \
//!   cargo test -p ds-cli-auth --test data_distribution_live -- --ignored --nocapture
//! ```
//!
//! What it proves: the fixed `POST /api/v1/data-distribution` call, sent with
//! the lane's own credential and `X-User-Email`, satisfies ds-brain's
//! `requireProjectAccess` and `listVectorTiles` for the selected project, and
//! the answer decodes as a catalogue. It prints the row count and, per row,
//! whether a published bundle is downloadable — the facts a seeder plans from.

use ds_cli_auth::{DataDistributionRequest, data_distribution};

#[test]
#[ignore = "needs DS_NATIVE_CLIENT_PROFILE_BUNDLE and a signed-in native identity with a selected project"]
fn a_signed_in_identity_reads_its_reference_catalogue() {
    let lane = std::env::var("DS_LIVE_LANE").unwrap_or_else(|_| "stable".to_owned());
    let rows = data_distribution(&lane, &DataDistributionRequest::ListDatasets {}).unwrap_or_else(
        |failure| {
            panic!(
                "list_datasets refused on {lane}: {} — {}{}",
                failure.code(),
                failure.message(),
                failure
                    .remedy_text()
                    .map(|remedy| format!(" (remedy: {remedy})"))
                    .unwrap_or_default()
            )
        },
    );
    let rows = rows.as_array().expect("a catalogue is an array");
    println!("list_datasets ({lane}): {} catalogue row(s)", rows.len());
    for row in rows {
        println!(
            "  layer={} country={} version={} downloadable={} download_bytes={} unavailable={}",
            row["layer"],
            row["country"],
            row["version"]
                .as_str()
                .map(|v| &v[..v.len().min(12)])
                .unwrap_or("?"),
            row["download_url"].is_string(),
            row["download_bytes"],
            row["unavailable"]
        );
    }
}
