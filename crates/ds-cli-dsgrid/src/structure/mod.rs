//! `ds dsgrid structure …` — typed edits of one placed structure, over the
//! family plumbing in [`crate::mutation`].
//!
//! The first typed mutations of program contract 01 §2. Each verb is typed
//! inputs, one engine command (or a batch of them as one revision), and the
//! family receipt; the engine decides everything else. `describe` is the
//! consultant's point 1 (a description on every structure); `retype` is
//! point 2 (single poles on a big angle become H-poles), and it is the first
//! command to carry a REG structure-rule finding on its receipt.

pub mod describe;
pub mod retype;

use ds_cli_contract::outcome::Failure;
use ds_grid_model::{GridModelSnapshot, StructureId, StructureRow, StructureTypeId};
use serde_json::json;

/// One structure named by its id (`str-…`) or, when unique, by the
/// engineering number a structure list prints (`230`). An engineer types the
/// number they read on the sheet; the id is what the receipt carries.
pub fn resolve_structure<'a>(
    snapshot: &'a GridModelSnapshot,
    raw: &str,
) -> Result<&'a StructureRow, Failure> {
    let wanted = raw.trim();
    if let Some(row) = snapshot
        .structures
        .iter()
        .find(|row| row.id.as_str() == wanted)
    {
        return Ok(row);
    }
    let by_number: Vec<&StructureRow> = snapshot
        .structures
        .iter()
        .filter(|row| row.engineering_number.as_deref() == Some(wanted))
        .collect();
    match by_number.len() {
        1 => Ok(by_number[0]),
        0 => Err(Failure::invalid(
            "structure_unknown",
            format!("no structure `{wanted}` in this revision"),
        )
        .remedy("use a structure id or engineering number from `ds dsgrid report structures`")
        .detail(json!({ "structures": snapshot.structures.len() }))),
        _ => Err(Failure::invalid(
            "structure_unknown",
            format!(
                "`{wanted}` is the engineering number of {} structures",
                by_number.len()
            ),
        )
        .remedy("name the structure by its id (str-…) instead")
        .detail(json!({
            "candidates": by_number.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        }))),
    }
}

/// One structure type named by its id (`st-…`) or its exact library name
/// (`j-w-60d-S325.014`). A name that resolves to two types (a library can
/// hold two definitions under one name when their geometry diverged) is a
/// refusal that lists them, never a guess.
pub fn resolve_structure_type(
    snapshot: &GridModelSnapshot,
    raw: &str,
) -> Result<StructureTypeId, Failure> {
    let wanted = raw.trim();
    if let Some(row) = snapshot
        .structure_types
        .iter()
        .find(|row| row.id.as_str() == wanted)
    {
        return Ok(row.id.clone());
    }
    let by_name: Vec<&ds_grid_model::StructureTypeRow> = snapshot
        .structure_types
        .iter()
        .filter(|row| row.engineering_name == wanted)
        .collect();
    match by_name.len() {
        1 => Ok(by_name[0].id.clone()),
        0 => {
            let known: Vec<&str> = snapshot
                .structure_types
                .iter()
                .map(|row| row.engineering_name.as_str())
                .collect();
            let (shown, withheld) = crate::package::take(known, 40);
            Err(Failure::invalid(
                "structure_type_unknown",
                format!("this model has no structure type `{wanted}`"),
            )
            .remedy("use a type id or exact library name from the model's structure-type list")
            .detail(json!({ "structure_types": shown, "withheld": withheld })))
        }
        _ => Err(Failure::invalid(
            "structure_type_ambiguous",
            format!(
                "`{wanted}` names {} structure types in this model",
                by_name.len()
            ),
        )
        .remedy("name the type by its id (st-…) instead")
        .detail(json!({
            "candidates": by_name.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        }))),
    }
}

/// The library name of a type id, for receipts that print what an engineer
/// reads rather than a digest.
pub fn type_name(snapshot: &GridModelSnapshot, id: &StructureTypeId) -> String {
    snapshot
        .structure_types
        .iter()
        .find(|row| row.id == *id)
        .map(|row| row.engineering_name.clone())
        .unwrap_or_else(|| id.as_str().to_string())
}

pub fn structure_json(row: &StructureRow, type_name: &str) -> serde_json::Value {
    json!({
        "structure": row.id.as_str(),
        "number": row.engineering_number,
        "structure_type": type_name,
        "structure_type_id": row.structure_type_id.as_str(),
        "alignment": row.alignment_id.as_ref().map(|id| id.as_str()),
        "station_m": row.station_m,
        "description": row.description,
    })
}

pub(crate) fn id_of(row: &StructureRow) -> StructureId {
    row.id.clone()
}
