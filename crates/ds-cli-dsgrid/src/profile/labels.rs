//! Headless, model-authored interactive Profile label composition.
//! This policy travels in the .dsgrid manifest and survives model edits and
//! checkpoints. Fixed-paper plan/profile sheets own separate layout policies.
use std::collections::BTreeSet;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::{PackOptions, dsgrid};
use ds_grid_model::{
    StructureLabelAffix, StructureLabelField, StructureLabelOrientation, StructureLabelPolicy,
};
use serde_json::{Value, json};
use sha2::Digest;

use crate::{apply, package};

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "labels_invalid",
        when: "a field is unknown or repeated, or orientation is unsupported",
        remedy: "use unique number,station,type,height,alignment,comment1,comment2,comment3 fields",
    },
    Refusal {
        code: "output_exists",
        when: "the output package already exists",
        remedy: "choose a new .dsgrid filename",
    },
    Refusal {
        code: "package_emit_failed",
        when: "the updated package cannot be emitted",
        remedy: "inspect the source package and report the engine failure",
    },
];

pub static SHOW: Command = Command {
    id: "dsgrid.profile.labels.show",
    path: &["dsgrid", "profile", "labels", "show"],
    contract: 1,
    summary: "Read model-authored interactive Profile labels with the Desktop closed.",
    purpose: "Reads the verified .dsgrid manifest's interactive Profile label policy. An absent authored policy resolves to the typed DS Grid default; no browser preference is consulted. Fixed-paper sheet labels are separate.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("model", "<path>", "Source .dsgrid package.").required()],
    output: "Model identity, authored/default status, ordered fields and orientation.",
    examples: &[Example {
        command: "ds dsgrid profile labels show --model ./design.dsgrid --output json",
        note: "Read label composition without opening a map.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["profile", "labels", "model", "headless"],
    requires: Requires::Server,
    availability: available,
};

pub static SET: Command = Command {
    id: "dsgrid.profile.labels.set",
    path: &["dsgrid", "profile", "labels", "set"],
    contract: 1,
    summary: "Author interactive Profile labels into a new .dsgrid package.",
    purpose: "Changes only the model's typed Profile label policy. Source bytes remain intact; a new verified package carries the composition across machines and checkpoints. This command needs no open Desktop. Printed plan/profile sheet composition is separate.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "Source .dsgrid package.").required(),
        Arg::value(
            "fields",
            "<comma-separated-fields>",
            "Ordered unique structure label fields; an empty string hides labels.",
        )
        .required(),
        Arg::value(
            "orientation",
            "<orientation>",
            "auto,right,left,above,below,vertical.",
        )
        .choices(&["auto", "right", "left", "above", "below", "vertical"]),
        Arg::value(
            "separator",
            "<text>",
            "Between nonempty fields; empty concatenates; \n means a new line.",
        ),
        Arg::value(
            "affixes",
            "<json-path>",
            "JSON array of selected field/prefix/suffix objects.",
        ),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid package; never overwrites source.",
        )
        .required(),
    ],
    output: "New package path, byte length, SHA-256, ordered label fields and orientation.",
    examples: &[Example {
        command: "ds dsgrid profile labels set --model ./design.dsgrid --fields number,type,comment1,comment2,comment3 --out ./design-labelled.dsgrid --output json",
        note: "Compose labels without chainage and carry them inside the new model.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["profile", "labels", "model", "headless"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn show(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = inputs.require("model")?;
    let bytes = package::read_bytes(path)?;
    let manifest = package::read_manifest(path, &bytes)?;
    let policy = manifest
        .model
        .presentation
        .effective_profile_structure_labels();
    Ok(json!({
        "model": path,
        "model_id": manifest.model.model_id.as_str(),
        "authored": manifest.model.presentation.profile_structure_labels.is_some(),
        "policy": policy,
    }))
}

pub fn set(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = inputs.require("model")?;
    let out = inputs.require("out")?;
    let fields = parse_fields(inputs.require("fields")?)?;
    let bytes = package::read_bytes(path)?;
    let package = package::decode(path, &bytes)?;
    let orientation = match inputs.value("orientation") {
        Some(raw) => serde_json::from_value::<StructureLabelOrientation>(json!(raw))
            .map_err(|_| invalid("orientation is unsupported"))?,
        None => {
            package
                .manifest
                .model
                .presentation
                .effective_profile_structure_labels()
                .orientation
        }
    };
    let previous = package
        .manifest
        .model
        .presentation
        .effective_profile_structure_labels();
    let separator = inputs
        .value("separator")
        .map(|raw| raw.replace("\\n", "\n"))
        .unwrap_or(previous.separator);
    let affixes = match inputs.value("affixes") {
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|error| invalid(&format!("cannot read affixes file: {error}")))?;
            serde_json::from_slice::<Vec<StructureLabelAffix>>(&bytes)
                .map_err(|error| invalid(&format!("invalid affixes JSON: {error}")))?
        }
        None => previous
            .affixes
            .into_iter()
            .filter(|part| fields.contains(&part.field))
            .collect(),
    };
    let policy = StructureLabelPolicy {
        fields,
        orientation,
        separator,
        affixes,
    };
    let mut presentation = package.manifest.model.presentation.clone();
    presentation.profile_structure_labels = Some(policy.clone());
    apply::validate_output_path(out)?;
    let options = PackOptions {
        model_id: package.manifest.model.model_id.clone(),
        model_revision: package.manifest.model.model_revision + 1,
        presentation,
        coordinate_system: package.manifest.model.coordinate_system.clone(),
        library_pins: package.manifest.model.library_pins.clone(),
        library_needs: package.manifest.model.library_needs.clone(),
        assets: package.assets,
        exchange_bindings: package.exchange_bindings,
    };
    let (plan, _) = dsgrid::emit(&package.snapshot, &options)
        .map_err(|error| Failure::failed("package_emit_failed", error.to_string()))?;
    let artifact = plan
        .artifacts
        .first()
        .ok_or_else(|| Failure::failed("package_emit_failed", "no package artifact"))?;
    apply::write_new(out, &artifact.bytes)?;
    let digest = sha2::Sha256::digest(&artifact.bytes);
    Ok(json!({
        "source": path,
        "out": out,
        "model_id": options.model_id.as_str(),
        "model_revision": options.model_revision,
        "bytes": artifact.bytes.len(),
        "sha256": format!("{digest:x}"),
        "policy": policy,
    }))
}

fn parse_fields(raw: &str) -> Result<Vec<StructureLabelField>, Failure> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = BTreeSet::new();
    raw.split(',')
        .map(|part| {
            let value = part.trim();
            if !seen.insert(value.to_owned()) {
                return Err(invalid("label fields must be unique"));
            }
            serde_json::from_value::<StructureLabelField>(json!(value))
                .map_err(|_| invalid("label field is unsupported"))
        })
        .collect()
}

fn invalid(message: &str) -> Failure {
    Failure::invalid("labels_invalid", message)
        .remedy("use unique number,station,type,height,alignment,comment1,comment2,comment3 fields")
}

pub fn render(data: &Value) -> String {
    format!(
        "Profile labels: {}",
        data["policy"]["fields"]
            .as_array()
            .map(|fields| fields
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","))
            .unwrap_or_default()
    )
}
