//! Governed Rwanda geographic data distribution through the paired Desktop.
use crate::discover::Descriptor;
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "data.rwanda.status",
    arguments: &["resources"],
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
    "<dataset-id>",
    "Dataset ID from status; repeat for install/remove. Calculation data also accepts rw-admin-villages and rwanda-dem-10m.",
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
        remedy: "repeat --resource for each dataset, or run `ds desktop data rwanda status` to list them",
    },
    Refusal {
        code: "invalid_dataset",
        when: "a named dataset is not one this build offers",
        remedy: "run `ds desktop data rwanda status` and use a name it lists",
    },
    Refusal {
        code: "invalid_size_limit",
        when: "--max-download-mib is outside 1-102400 MiB",
        remedy: "pass a whole number of MiB within that range, or omit it",
    },
    // Named by the application (`data.rwanda.install` refuses a cloud-resident
    // catalogue row before any transfer) and carried through as its own code:
    // a national table is never installed (docs/contracts/foundation-datasets.md R2).
    Refusal {
        code: "dataset_cloud_only",
        when: "a named dataset is cloud-resident (rwanda_upi_parcels, edcl_customers): it is never installed nationally",
        remedy: "read it bounded (`ds data parcels query`, `ds data customers query`) or seed one project's extents with `ds data project-cache seed`",
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
    purpose: "Reads the governed dataset catalog and local install sizes, and answers `why is this not here?` for every row: held, not held, or kept off this computer by the disk reserve the next install would apply. Reports this volume's free, reserved and spendable bytes with it. With --resource, reads durable publication status for each selected dataset without starting another job. Missing bundles never block printing.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, DESCRIPTOR_ARG],
    output: "Project, dataset counts, aggregate sizes, this volume's free, reserved and spendable bytes, and per dataset its publication state, install state and whether the disk reserve is what keeps it off this computer.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    search: &["cache", "held", "disk"],
    requires: Requires::Window,
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
    search: &[],
    requires: Requires::Window,
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
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static INSTALL_COMMAND: Command = Command {
    id: "desktop.data.rwanda.install",
    path: &["desktop", "data", "rwanda", "install"],
    contract: 1,
    summary: "Download and index the Rwanda datasets this disk can hold.",
    purpose: "Safe to run at any time, including on the way to a report, a map or any other action: it spends disk and nothing else, changes nothing in the cloud, and `ds desktop data rwanda remove` puts it back. Without --resource it takes EVERY published dataset this computer does not already hold, elevation and administrative boundaries included, smallest first, up to free disk less a reserve of a tenth of the volume or 20 GiB, whichever is smaller; a dataset that would cross the reserve is named in the receipt instead of refusing the run. --resource narrows the run to exact datasets. Every bundle is verified by compressed and expanded digest before its local SQLite RTree is accepted. --max-download-mib bounds transfer size; skipped or missing datasets never block maps or printing.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, MAX_DOWNLOAD_ARG, DESCRIPTOR_ARG],
    output: "What was installed, what was already held, what was skipped and why, the volume before and after, and the catalog receipt with compressed transfer, expanded payload, spatial index and total disk sizes.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    search: &[
        "cache",
        "reference",
        "ground",
        "geographic",
        "offline",
        "seed",
    ],
    requires: Requires::Window,
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
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static REMOVE_COMMAND: Command = Command {
    id: "desktop.data.rwanda.remove",
    path: &["desktop", "data", "rwanda", "remove"],
    contract: 1,
    summary: "Remove selected Rwanda datasets from this computer.",
    purpose: "The exact undo of install: removes every locally indexed version of each explicit opaque catalog dataset ID and gives the disk back. It changes only this computer; Firestore catalog rows and Cloud data are untouched, and the dataset can be downloaded again at any time.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, DESCRIPTOR_ARG],
    output: "The refreshed catalog receipt and exact remaining local disk use.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.data.md"),
    search: &["uninstall", "undo"],
    requires: Requires::Window,
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
    if values
        .iter()
        .any(|value| !valid_id(value) && value != "rw-admin-villages" && value != "rwanda-dem-10m")
    {
        return Err(Failure::invalid(
            "invalid_dataset",
            "--resource must name a dataset returned by status",
        ));
    }
    Ok(values.to_vec())
}
fn invoke_empty(inputs: &Inputs, op: &BridgeOp, timeout: Duration) -> Result<Value, Failure> {
    ops::invoke(&descriptor(inputs)?, op, json!({}), timeout).map_err(ops::classify_signed_out)
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let values = ids(inputs)?;
    let args = if values.is_empty() {
        json!({})
    } else {
        json!({"resources": values})
    };
    ops::invoke(
        &descriptor(inputs)?,
        &STATUS_OP,
        args,
        Duration::from_secs(180),
    )
    .map_err(ops::classify_signed_out)
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
    .map_err(classify_cloud_only)
}

/// The application's own `dataset_cloud_only` refusal, re-coded so a script
/// branches on the name rather than on a sentence inside `desktop_refused`.
fn classify_cloud_only(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_owned();
    if !detail.starts_with("dataset_cloud_only") {
        return failure;
    }
    Failure::conflict("dataset_cloud_only", detail)
        .remedy("read it bounded (`ds data parcels query`, `ds data customers query`) or seed one project's extents with `ds data project-cache seed`")
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
        assert_eq!(STATUS_OP.arguments, &["resources"]);
        assert_eq!(CATALOG_OP.arguments, &[] as &[&str]);
        assert_eq!(PUBLISH_OP.arguments, &["resources"]);
        assert_eq!(INSTALL_OP.arguments, &["resources", "max_download_mib"]);
        assert_eq!(STORAGE_OP.arguments, &["path"]);
        assert_eq!(REMOVE_OP.arguments, &["resources"]);
        assert_eq!(CATALOG_COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(PUBLISH_COMMAND.effect, Effect::GlobalWrite);
    }
}
