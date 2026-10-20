//! `ds map local` — the prepared local layers this machine keeps for itself.
//!
//! A prepared local layer is a catalogue row on the operator's own disk: a
//! name, an origin, a geometry type, a style, a column schema, a feature count
//! and a copied payload beside it. The browser keeps its catalogue in
//! IndexedDB; `ds` and the installed desktop keep theirs in a file under the
//! shared native layer root, one per lane and DS account.
//!
//! Every decision belongs to the shared kernel
//! (`ds-command-kernel::local_layers`) and every file belongs to
//! `ds-layer-store::prepared`. These four commands carry inputs in and a
//! receipt out; they mint nothing, default nothing and delete nothing the
//! receipt did not name. Refusals keep the kernel's own codes, so a caller
//! plans for the same words here, in the desktop and in the browser.

pub mod list;
pub mod register;
pub mod remove;
pub mod rename;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Availability, Refusal};
use ds_layer_store::prepared::{Host, Scope, StoreError};
use serde_json::{Value, json};

pub use crate::layer::native::LANE_ARG;

pub const ACCOUNT_ARG: Arg = Arg::value(
    "account",
    "<uid>",
    "DS account this catalogue belongs to; `local` is the machine's own.",
)
.default("local");

pub const LAYER_ARG: Arg = Arg::value("layer", "<id>", "Id from `ds map local list`.");

pub const STORE_REFUSED: Refusal = Refusal {
    code: "local_layer_refused",
    when: "the catalogue file cannot be read or persisted, or is malformed",
    remedy: "repair the store under DS_LAYER_HOME (or the local data directory); it is never overwritten for you",
};
pub const INVALID_PAYLOAD: Refusal = Refusal {
    code: "invalid_payload",
    when: "--file is not a readable bounded GeoJSON FeatureCollection of the declared geometry",
    remedy: "pass one FeatureCollection under 64 MiB whose features all carry the --geometry type",
};
pub const MALFORMED_DESCRIPTOR: Refusal = Refusal {
    code: "malformed_descriptor",
    when: "the kernel refused a name, a bound or a row already in the catalogue",
    remedy: "use a trimmed name of 1 to 200 characters; repair a row the message names",
};
pub const DUPLICATE_LAYER: Refusal = Refusal {
    code: "duplicate_layer",
    when: "the catalogue holds one id twice",
    remedy: "remove the repeated row from the catalogue file; ids are never reassigned for you",
};
pub const UNKNOWN_LAYER: Refusal = Refusal {
    code: "unknown_layer",
    when: "--layer is not an id in this lane and account's catalogue",
    remedy: "copy the id from ds map local list --output json",
};
pub const SCOPE_MISMATCH: Refusal = Refusal {
    code: "scope_mismatch",
    when: "the catalogue found under this lane and account records a different one",
    remedy: "run the command under the lane and account the catalogue names, or move the file",
};
pub const UNSUPPORTED_SOURCE_KIND: Refusal = Refusal {
    code: "unsupported_source_kind",
    when: "a row in the catalogue is a Project Work root, which never persists",
    remedy: "delete that row; Project Work is an ephemeral projection of governed entities",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not supplied to a removal",
    remedy: "review ds map local list, then repeat the command with --yes",
};

/// What every op can refuse regardless of which one it is: a store that cannot
/// be read, and the three faults a catalogue file itself can carry.
pub const STORE_REFUSALS: &[Refusal] = &[
    STORE_REFUSED,
    MALFORMED_DESCRIPTOR,
    DUPLICATE_LAYER,
    SCOPE_MISMATCH,
    UNSUPPORTED_SOURCE_KIND,
];

/// Reading and writing a file on this machine needs no principal and no map.
pub fn availability() -> Availability {
    Availability::Available
}

/// Which catalogue this invocation is about. A lane and an account, because
/// two lanes and two signed-in people on one machine never share one.
pub fn scope(inputs: &Inputs) -> Result<Scope, Failure> {
    Ok(Scope {
        host: Host::Native,
        lane: Some(inputs.require("lane")?.trim().to_owned()),
        uid: Some(inputs.require("account")?.trim().to_owned()),
    })
}

/// Re-raise the kernel's refusal under its own name.
///
/// The vocabulary is closed and shared with the browser and the desktop, so a
/// caller that has planned for `unknown_layer` once has planned for it
/// everywhere. Nothing is renamed into a local word here.
pub fn refuse(error: StoreError) -> Failure {
    match error {
        StoreError::Refused { code, message } => match code.as_str() {
            "malformed_descriptor" => Failure::invalid("malformed_descriptor", message)
                .remedy(MALFORMED_DESCRIPTOR.remedy),
            "duplicate_layer" => {
                Failure::invalid("duplicate_layer", message).remedy(DUPLICATE_LAYER.remedy)
            }
            "unknown_layer" => {
                Failure::invalid("unknown_layer", message).remedy(UNKNOWN_LAYER.remedy)
            }
            "scope_mismatch" => {
                Failure::invalid("scope_mismatch", message).remedy(SCOPE_MISMATCH.remedy)
            }
            "unsupported_source_kind" => Failure::invalid("unsupported_source_kind", message)
                .remedy(UNSUPPORTED_SOURCE_KIND.remedy),
            _ => Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy),
        },
        StoreError::Payload(message) => {
            Failure::invalid("invalid_payload", message).remedy(INVALID_PAYLOAD.remedy)
        }
        StoreError::Store(message) => {
            Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy)
        }
    }
}

/// One catalogue row, in this CLI's own spelling.
pub fn row(row: &Value) -> Value {
    let mut out = json!({
        "layer": row["id"].clone(),
        "name": row["name"].clone(),
        "source_name": row["sourceName"].clone(),
        "source_layer_name": row["sourceLayerName"].clone(),
        "source_kind": row["sourceKind"].clone(),
        "geometry_type": row["geometryType"].clone(),
        "feature_count": row["featureCount"].clone(),
        "visible": row["visible"].clone(),
        "created_at": row["createdAt"].clone(),
    });
    if let Some(origin) = row.get("origin").filter(|origin| !origin.is_null()) {
        out["origin"] = json!({
            "kind": origin["kind"].clone(),
            "label": origin["label"].clone(),
        });
    }
    out
}

/// The scope and where it was persisted, said the same way by all four.
pub fn stamped(scope: &Scope, answer: &Value, mut data: Value) -> Value {
    data["lane"] = json!(scope.lane);
    data["account"] = json!(scope.uid);
    data["persisted"] = answer["persisted"].clone();
    data["revision"] = answer["revision"].clone();
    data["changed"] = answer["changed"].clone();
    data
}

pub fn render_row(row: &Value) -> String {
    format!(
        "{:<26} {:<11} {:>7} {:<8} {}\n",
        row["layer"].as_str().unwrap_or("?"),
        row["geometry_type"].as_str().unwrap_or("?"),
        row["feature_count"],
        if row["visible"].as_bool().unwrap_or(false) {
            "visible"
        } else {
            "hidden"
        },
        row["name"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests;
