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
    .remedy("omit --publication-assets to read the approved PDFs through DS assets, or map exactly the required asset ids to absolute held paths; no extra or omitted assets are allowed")
    .detail(json!({
        "required": wanted,
        "missing": wanted.difference(&given).collect::<Vec<_>>(),
        "unexpected": given.difference(&wanted).collect::<Vec<_>>(),
    })))
}

/// The approved PDFs read through DS assets when the caller holds none: one
/// governed read per preserved page, each checked against the adopted setup's
/// own digest pin before the render sees it. The private directory lives as
/// long as the returned guard, i.e. through the reporter call.
pub(crate) struct Acquired {
    _dir: tempfile::TempDir,
    pub map: Value,
    pub receipt: Value,
}

pub(crate) fn acquire_publication_assets(
    preserved_pages: &[PreservedPage],
    read: impl Fn(&str) -> Result<Vec<u8>, Failure>,
) -> Result<Acquired, Failure> {
    use sha2::{Digest, Sha256};
    let dir = tempfile::Builder::new()
        .prefix("ds-mv-approved-")
        .tempdir()
        .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
    let mut map = serde_json::Map::new();
    let mut receipt = Vec::new();
    for page in preserved_pages {
        if map.contains_key(&page.asset_id) {
            continue;
        }
        let bytes = read(&page.asset_id)?;
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        if digest != page.sha256 {
            return Err(Failure::invalid(
                "mv_print_asset_missing",
                format!(
                    "approved page asset {} no longer has the bytes the adopted setup pins",
                    page.asset_id
                ),
            )
            .remedy("restore the approved PDF in DS assets or intentionally adopt a setup revision that pins the current bytes")
            .detail(json!({"asset_id": page.asset_id, "expected": page.sha256, "actual": digest})));
        }
        // Asset ids are ds-brain identifiers; never let one name a path.
        let safe: String = page
            .asset_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.path().join(format!("{}-{safe}.pdf", map.len()));
        std::fs::write(&path, &bytes)
            .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        map.insert(page.asset_id.clone(), json!(path.display().to_string()));
        receipt.push(json!({"asset_id": page.asset_id, "sha256": digest, "bytes": bytes.len()}));
    }
    Ok(Acquired {
        _dir: dir,
        map: Value::Object(map),
        receipt: json!({"source": "ds_assets", "assets": receipt}),
    })
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "assets_acquire_tests.rs"]
mod acquire_tests;
