//! Governed Rwanda geographic data distribution through the paired Desktop.
use crate::discover::Descriptor;
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.status",
    arguments: &[],
};
pub const CATALOG_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.catalog",
    arguments: &[],
};
pub const PUBLISH_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.publish",
    arguments: &["resources"],
};
pub const INSTALL_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.install",
    arguments: &["resources", "max_download_mib"],
};
pub const STORAGE_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.storage",
    arguments: &["path"],
};
pub const REMOVE_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.remove",
    arguments: &["resources"],
};

const RESOURCE_ARG: Arg = Arg::repeated(
    "resource",
    "<sha256-id>",
    "Exact governed dataset ID; repeat for install or remove.",
);
const REQUIRED_RESOURCE_ARG: Arg = Arg::value(
    "resource",
    "<sha256-id>",
    "Exact governed dataset ID returned by data rwanda status.",
)
.required();
const MAX_DOWNLOAD_ARG: Arg = Arg {
    name: "max-download-mib",
    kind: ArgKind::Value,
    value: "<MiB>",
    required: false,
    default: None,
    choices: &[],
    summary: "Skip bundles larger than this compressed transfer size (1-102400 MiB).",
};
const STORAGE_PATH_ARG: Arg = Arg::value(
    "path",
    "<directory|recommended>",
    "Set a different existing parent directory, or use recommended to restore app-data storage. Omit to inspect.",
);

const REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    // Constructed by the input guards below; declared so `--help` can predict
    // them and the refusal contract can see them.
    Refusal {
        code: "dataset_required",
        when: "the command needs at least one dataset and none was named",
        remedy: "repeat --dataset for each dataset, or run `ds desktop data rwanda status` to list them",
    },
    Refusal {
        code: "invalid_dataset",
        when: "a named dataset is not one this build offers",
        remedy: "run `ds desktop data rwanda status` and use a name it lists",
    },
    Refusal {
        code: "invalid_size_limit",
        when: "--max-size is outside 1-102400 MiB",
        remedy: "pass a whole number of MiB within that range, or omit it",
    },
    Refusal {
        code: "invalid_storage_path",
        when: "the storage root is not an absolute path this computer can use",
        remedy: "pass an absolute directory path",
    },
];

pub static STATUS_COMMAND: Command = Command {
    id: "desktop.data.rwanda.status",
    path: &["desktop", "data", "rwanda", "status"],
    contract: 1,
    summary: "List Rwanda datasets and prove their exact local install state.",
    purpose: "Reads the active project's governed Firestore catalog and reports source rows/bytes, compressed transfer bytes, expanded bytes and local spatial-index disk use. Missing bundles remain explicit and never block printing.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "Project, dataset counts, aggregate sizes and exact per-dataset publication and install state.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

pub static CATALOG_COMMAND: Command = Command {
    id: "desktop.data.rwanda.catalog",
    path: &["desktop", "data", "rwanda", "catalog"],
    contract: 1,
    summary: "Reconcile Rwanda geographic sources (needs --yes).",
    purpose: "Discovers geographic BigQuery sources only from governed global-tile publication workspaces, reads stable source metadata, and writes the authoritative Firestore data catalog with any verified compressed and expanded desktop bundle sizes.",
    chapter: Chapter::Data,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "The complete governed catalog and exact source, transfer and expanded sizes after reconciliation.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

pub static PUBLISH_COMMAND: Command = Command {
    id: "desktop.data.rwanda.publish",
    path: &["desktop", "data", "rwanda", "publish"],
    contract: 1,
    summary: "Publish one dataset's compressed Desktop bundle (needs --yes).",
    purpose: "Starts the existing governed global-tile worker for one opaque catalog dataset, waits for its immutable gzip GeoJSON Sequence bundle, then reconciles exact compressed and expanded sizes into Firestore. Arbitrary BigQuery identifiers are never accepted.",
    chapter: Chapter::Data,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUIRED_RESOURCE_ARG, DESCRIPTOR_ARG],
    output: "Dataset identity, feature count, compressed transfer bytes and expanded desktop bytes.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

pub static INSTALL_COMMAND: Command = Command {
    id: "desktop.data.rwanda.install",
    path: &["desktop", "data", "rwanda", "install"],
    contract: 1,
    summary: "Download and spatially index selected Rwanda datasets.",
    purpose: "Without --resource, downloads published datasets whose source and expanded size are at most 500 MiB. Larger datasets require explicit repeated --resource selections. Every bundle is verified by compressed and expanded digest before its local SQLite RTree is accepted. --max-download-mib bounds transfer size; skipped or missing datasets never block maps or printing.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, MAX_DOWNLOAD_ARG, DESCRIPTOR_ARG],
    output: "The catalog receipt with compressed transfer, expanded payload, spatial index and total disk sizes.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

pub static STORAGE_COMMAND: Command = Command {
    id: "desktop.data.rwanda.storage",
    path: &["desktop", "data", "rwanda", "storage"],
    contract: 1,
    summary: "Inspect or change this computer's geographic-data storage root.",
    purpose: "Reports the actual data root and filesystem capacity. With --path, creates only the app-owned child directory under an existing selected parent; use recommended to restore the platform app-data root. Changing roots does not copy or delete the old cache.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[STORAGE_PATH_ARG, DESCRIPTOR_ARG],
    output: "Resolved owned root, available and total filesystem bytes, and whether the recommended root is active.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

pub static REMOVE_COMMAND: Command = Command {
    id: "desktop.data.rwanda.remove",
    path: &["desktop", "data", "rwanda", "remove"],
    contract: 1,
    summary: "Remove selected Rwanda datasets from this computer.",
    purpose: "Removes every locally indexed version of each explicit opaque catalog dataset ID. It changes only this computer; Firestore catalog rows and Cloud data are untouched.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, DESCRIPTOR_ARG],
    output: "The refreshed catalog receipt and exact remaining local disk use.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    availability: ops::paired_availability,
};

fn descriptor(inputs: &Inputs) -> Result<Descriptor, Failure> {
    ops::paired(inputs.value("desktop-descriptor"))
}
fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn ids(inputs: &Inputs) -> Result<Vec<String>, Failure> {
    let values = inputs.repeated("resource");
    if values.iter().any(|value| !valid_id(value)) {
        return Err(Failure::invalid(
            "invalid_dataset",
            "--resource must be a 64-character lowercase SHA-256 dataset ID",
        ));
    }
    Ok(values.to_vec())
}
fn invoke_empty(inputs: &Inputs, op: &BridgeOp, timeout: Duration) -> Result<Value, Failure> {
    ops::invoke(&descriptor(inputs)?, op, json!({}), timeout).map_err(ops::classify_signed_out)
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke_empty(inputs, &STATUS_OP, Duration::from_secs(180))
}
pub fn catalog(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke_empty(inputs, &CATALOG_OP, Duration::from_secs(300))
}
pub fn publish(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let value = inputs.value("resource").unwrap_or_default();
    if !valid_id(value) {
        return Err(Failure::invalid(
            "dataset_required",
            "--resource must name exactly one dataset",
        ));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &PUBLISH_OP,
        json!({"resources": [value]}),
        Duration::from_secs(1850),
    )
    .map_err(ops::classify_signed_out)
}
pub fn install(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let values = ids(inputs)?;
    let mut arguments = Map::new();
    if !values.is_empty() {
        arguments.insert("resources".into(), json!(values));
    }
    if let Some(raw) = inputs.value("max-download-mib") {
        let value = raw.parse::<u64>().map_err(|_| {
            Failure::invalid(
                "invalid_size_limit",
                "--max-download-mib must be an integer from 1 to 102400",
            )
        })?;
        if !(1..=102400).contains(&value) {
            return Err(Failure::invalid(
                "invalid_size_limit",
                "--max-download-mib must be an integer from 1 to 102400",
            ));
        }
        arguments.insert("max_download_mib".into(), json!(value));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &INSTALL_OP,
        Value::Object(arguments),
        Duration::from_secs(7200),
    )
    .map_err(ops::classify_signed_out)
}
pub fn storage(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(path) = inputs.value("path") {
        if path.trim().is_empty() {
            return Err(Failure::invalid(
                "invalid_storage_path",
                "--path cannot be empty",
            ));
        }
        arguments.insert("path".into(), json!(path));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &STORAGE_OP,
        Value::Object(arguments),
        Duration::from_secs(60),
    )
    .map_err(ops::classify_signed_out)
}
pub fn remove(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let values = ids(inputs)?;
    if values.is_empty() {
        return Err(Failure::invalid(
            "dataset_required",
            "repeat --resource for each dataset to remove",
        ));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &REMOVE_OP,
        json!({"resources": values}),
        Duration::from_secs(300),
    )
    .map_err(ops::classify_signed_out)
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operations_are_closed_and_size_limit_is_explicit() {
        assert_eq!(STATUS_OP.arguments, &[] as &[&str]);
        assert_eq!(CATALOG_OP.arguments, &[] as &[&str]);
        assert_eq!(PUBLISH_OP.arguments, &["resources"]);
        assert_eq!(INSTALL_OP.arguments, &["resources", "max_download_mib"]);
        assert_eq!(STORAGE_OP.arguments, &["path"]);
        assert_eq!(REMOVE_OP.arguments, &["resources"]);
        assert_eq!(CATALOG_COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(PUBLISH_COMMAND.effect, Effect::GlobalWrite);
    }
}
