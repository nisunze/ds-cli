//! The held PDFs an adopted MV setup names, checked before anything slow runs.

use std::collections::BTreeSet;

use ds_cli_contract::outcome::Failure;
use ds_command_kernel::printing::mv::PreservedPage;
use serde_json::{Value, json};

/// The publication-assets map must name exactly the setup's preserved pages:
/// no omitted and no extra asset. The reporter enforces the same rule after
/// context acquisition and render setup; checking it here refuses in
/// milliseconds instead of after the long native work.
pub(super) fn check_publication_assets(
    preserved_pages: &[PreservedPage],
    supplied: &Value,
) -> Result<(), Failure> {
    let wanted: BTreeSet<&str> = preserved_pages
        .iter()
        .map(|page| page.asset_id.as_str())
        .collect();
    let given: BTreeSet<&str> = supplied
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if wanted == given {
        return Ok(());
    }
    Err(Failure::invalid(
        "mv_print_asset_missing",
        "publication assets must be exactly the approved preserved_pdf assets named by the adopted setup",
    )
    .remedy("acquire each approved PDF through DS assets and pass --publication-assets mapping exactly the required asset ids to absolute held paths; no extra or omitted assets are allowed")
    .detail(json!({
        "required": wanted,
        "missing": wanted.difference(&given).collect::<Vec<_>>(),
        "unexpected": given.difference(&wanted).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
