//! Explicit headless Rwanda village refresh for named DS Grid structures.

use std::collections::BTreeSet;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{GridCommand, RwandaAdminRefreshRow, preview_rwanda_admin_refresh};
use ds_grid_model::RwandaAdminLocation;
use ds_rwanda_admin_bounds::{AdminBoundsIndex, AdminDetail, released_digest_matches};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::mutation::{self, Planned, Target};
use crate::structure::resolve_structure;

const INDEX_ARG: Arg = Arg::value(
    "index",
    "<path>",
    "Exact local Rwanda village .dsab or .geojson.zst authority file.",
).required();
const DIGEST_ARG: Arg = Arg::value(
    "index-sha256",
    "<sha256>",
    "SHA-256 of the installed index bytes; must match a reviewed release digest.",
).required();
const SELECTION_ARG: Arg = Arg::value(
    "selection",
    "<json-path>",
    "Bounded JSON selection: structures [{id, village_code?}]. An omitted code uses verified model coordinates.",
).required();
const TOKEN_ARG: Arg = Arg::value(
    "token",
    "<sha256>",
    "Preview token returned by the same selection, model revision, index and conflict choice.",
);
const ACCEPT_ARG: Arg = Arg::switch(
    "accept-existing-conflicts",
    "Explicitly replace old staking names that differ from the verified hierarchy.",
);

const OWN: &[Refusal] = &[
    Refusal {
        code: "index_unavailable",
        when: "the named local index cannot be read as a bounded regular file",
        remedy: "install the reviewed Rwanda admin index and pass its exact path",
    },
    Refusal {
        code: "index_digest_mismatch",
        when: "the index bytes differ from the supplied and released SHA-256",
        remedy: "use the reviewed release asset and its matching SHA-256",
    },
    Refusal {
        code: "index_invalid",
        when: "the index bytes fail parsing or do not contain 14,920 villages",
        remedy: "replace the index with the released verified asset",
    },
    Refusal {
        code: "selection_invalid",
        when: "the selection file is malformed, empty, duplicated or too large",
        remedy: "name 1..5000 unique structures in a JSON structures array",
    },
    Refusal {
        code: "village_unknown",
        when: "an exact village code is absent or a coordinate has no unique village",
        remedy: "review the structure location and use a known eight-digit village code",
    },
    Refusal {
        code: "model_crs_unsupported",
        when: "coordinate lookup has no verified model CRS to WGS84 transform",
        remedy: "declare a supported model CRS or supply exact village codes",
    },
    Refusal {
        code: "preview_changed",
        when: "apply token differs from the current preview",
        remedy: "run --dry-run again and inspect the new preview",
    },
    Refusal {
        code: "admin_conflict",
        when: "existing staking names disagree with the verified index",
        remedy: "review preview conflicts and explicitly accept them if justified",
    },
];
const REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut i = 0;
    while i < OWN.len() {
        all[i] = OWN[i];
        i += 1;
    }
    let mut j = 0;
    while j < mutation::REFUSALS.len() {
        all[OWN.len() + j] = mutation::REFUSALS[j];
        j += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.structure.admin-refresh",
    path: &["dsgrid", "structure", "admin-refresh"],
    contract: 1,
    summary: "Preview or apply exact Rwanda village facts for named structures.",
    purpose: "Resolve each exact village code, or a structure coordinate with a proved CRS transform, against reviewed local Rwanda index bytes. Preview names existing conflicts and returns a revision-bound token. Applying that token writes all selected staking names and hierarchy provenance in one model revision. No refresh occurs on model open.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        INDEX_ARG,
        DIGEST_ARG,
        SELECTION_ARG,
        TOKEN_ARG,
        ACCEPT_ARG,
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "The exact source revision and index SHA-256, resolved hierarchy and coordinate evidence per selected structure, conflict list, preview token, and apply revision/artifact receipt.",
    examples: &[
        Example {
            command: "ds dsgrid structure admin-refresh --package model.dsgrid --selection selected.json --index rwanda_villages.geojson.zst --index-sha256 381363ec19272091faf78f8f18b96187be5eb0634b8a047fef4edaab003a8b31 --dry-run --output json",
            note: "Preview an explicit bounded selection.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["administrative", "staking location"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    structures: Vec<SelectedStructure>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedStructure {
    id: String,
    #[serde(default)]
    village_code: Option<String>,
}

fn read_selection(path: &str) -> Result<Selection, Failure> {
    let metadata = std::fs::metadata(path).map_err(|e| {
        Failure::invalid("selection_invalid", format!("cannot read selection: {e}"))
    })?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 {
        return Err(Failure::invalid("selection_invalid", "selection is not a bounded regular file"));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        Failure::invalid("selection_invalid", format!("cannot read selection: {e}"))
    })?;
    let selection: Selection = serde_json::from_slice(&bytes).map_err(|e| {
        Failure::invalid("selection_invalid", format!("selection JSON is invalid: {e}"))
    })?;
    if selection.structures.is_empty() || selection.structures.len() > 5000 {
        return Err(Failure::invalid("selection_invalid", "select 1..5000 structures"));
    }
    Ok(selection)
}

fn load_index(path: &str, required_digest: &str) -> Result<AdminBoundsIndex, Failure> {
    let metadata = std::fs::metadata(path).map_err(|e| {
        Failure::invalid("index_unavailable", format!("cannot read index: {e}"))
    })?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err(Failure::invalid("index_unavailable", "index is not a bounded regular file"));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        Failure::invalid("index_unavailable", format!("cannot read index: {e}"))
    })?;
    let observed = format!("{:x}", Sha256::digest(&bytes));
    if observed != required_digest || !released_digest_matches(&observed) {
        return Err(Failure::invalid(
            "index_digest_mismatch",
            "index bytes do not match the supplied reviewed release digest",
        ).detail(json!({ "expected": required_digest, "observed": observed })));
    }
    let index = AdminBoundsIndex::load_from_bytes_for_path(Path::new(path), &bytes)
        .map_err(|e| Failure::invalid("index_invalid", format!("index parse failed: {e}")))?;
    if index.village_count() != 14_920 {
        return Err(Failure::invalid(
            "index_invalid",
            format!("index has {} villages; expected 14,920", index.village_count()),
        ));
    }
    Ok(index)
}

fn exact_village(index: &AdminBoundsIndex, code: &str) -> Result<AdminDetail, Failure> {
    if code.len() != 8 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Failure::invalid("village_unknown", "village code must have eight digits"));
    }
    let detail = index.detail(code).ok_or_else(|| {
        Failure::invalid("village_unknown", format!("village code {code} is absent from the index"))
    })?;
    if detail.level != "village" || detail.village_code.as_deref() != Some(code) {
        return Err(Failure::invalid("index_invalid", "village hierarchy has an inconsistent leaf"));
    }
    Ok(detail)
}

fn required(value: Option<String>, field: &str) -> Result<String, Failure> {
    value.ok_or_else(|| Failure::invalid("index_invalid", format!("village has no {field}")))
}

fn location_from_detail(detail: AdminDetail, digest: &str) -> Result<RwandaAdminLocation, Failure> {
    Ok(RwandaAdminLocation {
        province_code: required(detail.province_code, "province code")?,
        province: required(detail.province, "province")?,
        district_code: required(detail.district_code, "district code")?,
        district: required(detail.district, "district")?,
        sector_code: required(detail.sector_code, "sector code")?,
        sector: required(detail.sector, "sector")?,
        cell_code: required(detail.cell_code, "cell code")?,
        cell: required(detail.cell, "cell")?,
        village_code: required(detail.village_code, "village code")?,
        village: required(detail.village, "village")?,
        authority_sha256: digest.to_owned(),
        authority: "rwanda-admin-bounds/v1".to_owned(),
    })
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let target = Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let index_path = inputs.require("index")?;
    let digest = inputs.require("index-sha256")?;
    let index = load_index(index_path, digest)?;
    let selection = read_selection(inputs.require("selection")?)?;
    let need_positions = selection.structures.iter().any(|s| s.village_code.is_none());
    let positions = if need_positions {
        Some(crate::objects::index("admin-refresh", &opened.target.path(), None)?)
    } else {
        None
    };
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(selection.structures.len());
    let mut coordinate_evidence = Vec::new();
    for selected in &selection.structures {
        let structure = resolve_structure(opened.session.snapshot(), &selected.id)?;
        if !seen.insert(structure.id.as_str().to_owned()) {
            return Err(Failure::invalid("selection_invalid", format!(
                "structure {} occurs more than once", structure.id,
            )));
        }
        let code = if let Some(code) = &selected.village_code {
            code.clone()
        } else {
            let object = positions.as_ref().unwrap().structures.iter()
                .find(|object| object.id == structure.id.as_str())
                .ok_or_else(|| Failure::invalid(
                    "model_crs_unsupported",
                    format!("structure {} has no provable projected position", structure.id),
                ))?;
            let [lon, lat] = object.position;
            let hit = index.enrich_unique(lon, lat)
                .map_err(|e| Failure::invalid("village_unknown", e.to_string()))?
                .ok_or_else(|| Failure::invalid(
                    "village_unknown",
                    format!("structure {} position is outside indexed Rwanda villages", structure.id),
                ))?;
            coordinate_evidence.push(json!({
                "structure_id": structure.id.as_str(),
                "declared_crs": positions.as_ref().unwrap().crs,
                "wgs84": [lon, lat],
                "village_code": hit.code_village.as_ref(),
            }));
            hit.code_village.to_string()
        };
        rows.push(RwandaAdminRefreshRow {
            structure_id: structure.id.clone(),
            location: location_from_detail(exact_village(&index, &code)?, digest)?,
        });
    }
    let accept_conflicts = inputs.switch("accept-existing-conflicts");
    let preview = preview_rwanda_admin_refresh(
        opened.session.snapshot(), &opened.head, digest, &rows, accept_conflicts,
    ).map_err(crate::apply::map_command_error)?;
    let preview_token = preview.preview_token.clone();
    if !writing {
        return Ok(json!({
            "source_revision": opened.head.as_str(),
            "model_id": opened.package.manifest.model.model_id.as_str(),
            "index_path": index_path,
            "index_sha256": digest,
            "village_count": index.village_count(),
            "coordinate_evidence": coordinate_evidence,
            "preview": preview,
            "persisted": false,
        }));
    }
    if inputs.value("token") != Some(preview_token.as_str()) {
        return Err(Failure::conflict(
            "preview_changed",
            "the token does not match this model revision, index, selection and conflict choice",
        ).detail(json!({ "expected_preview_token": preview_token })));
    }
    let command = GridCommand::RefreshRwandaAdmin {
        authority_sha256: digest.to_owned(),
        rows,
        preview_token,
        accept_existing_conflicts: accept_conflicts,
    };
    mutation::run(
        opened,
        vec![Planned { command_id: uuid::Uuid::new_v4().to_string(), command }],
        true,
        json!({
            "index_path": index_path,
            "index_sha256": digest,
            "village_count": index.village_count(),
            "coordinate_evidence": coordinate_evidence,
            "preview": preview,
        }),
        vec![],
        &["DON"],
    )
}

pub fn render(data: &Value) -> String {
    let revision = data["source_revision"].as_str().unwrap_or("unknown");
    let count = data["preview"]["rows"].as_array().map_or(0, Vec::len);
    let token = data["preview"]["preview_token"].as_str().unwrap_or("");
    format!("Rwanda admin refresh: {count} structures at {revision}\nPreview token: {token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_real_village_code_resolves_from_checked_in_authority() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network-reporter/assets/rwanda_villages.geojson.zst");
        if !path.exists() {
            return;
        }
        let index = load_index(path.to_str().unwrap(), ds_rwanda_admin_bounds::RWANDA_GEOJSON_SHA256)
            .unwrap();
        let location = location_from_detail(exact_village(&index, "11090307").unwrap(),
            ds_rwanda_admin_bounds::RWANDA_GEOJSON_SHA256).unwrap();
        assert_eq!(location.village, "Inyarurembo");
        assert_eq!(location.sector, "Nyarugenge");
        assert_eq!(location.district, "Nyarugenge");
        assert_eq!(location.village_code, "11090307");
    }
}
