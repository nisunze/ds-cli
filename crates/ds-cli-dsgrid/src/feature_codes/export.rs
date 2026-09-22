//! `ds dsgrid feature-codes export` — write a model's feature-code table as a
//! PLS-CADD 16.81 FEA 15 member.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::feature_code_fea::{FeaExportError, FeaExportOptions, export_fea};
use serde_json::{Value, json};

use crate::mutation;

const OWN: &[Refusal] = &[
    Refusal {
        code: "voltage_class_unknown",
        when: "--voltage-class is not LV, MV, HV_110 or HV_220",
        remedy: "pass one of the four classes the standard defines",
    },
    Refusal {
        code: "voltage_class_mismatch",
        when: "the model's codes carry another voltage class than --voltage-class",
        remedy: "run `ds dsgrid feature-codes import --voltage-class <class>` first, or export the class the table carries",
    },
    Refusal {
        code: "feature_codes_without_clearances",
        when: "no feature code in the model carries a number and clearances",
        remedy: "import the standard (`ds dsgrid feature-codes import`) or convert the workspace with its FEA member first",
    },
    Refusal {
        code: "feature_code_number_duplicate",
        when: "two live feature codes share one PLS-CADD number",
        remedy: "retire one of them; a FEA lists each number once",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new path; the export never overwrites",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the parent directory of --out does not exist",
        remedy: "create the intended directory, then retry",
    },
    Refusal {
        code: "output_unwritable",
        when: "the FEA file cannot be written",
        remedy: "check free space and permissions; a partial file is removed",
    },
];

const REFUSALS: &[Refusal; OWN.len() + super::READ_REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + super::READ_REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + super::READ_REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < super::READ_REFUSALS.len() {
        all[OWN.len() + shared] = super::READ_REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.feature-codes.export",
    path: &["dsgrid", "feature-codes", "export"],
    contract: 1,
    summary: "Write a model's feature-code table as a PLS-CADD 16.81 FEA 15 file.",
    purpose: "\
Writes every live feature code of the model that carries a number and \
clearances as one PLS-CADD `FEA FILE` version 15 (SI): code number, name, \
required horizontal and vertical clearance, the native columns a code was \
imported with or the field-file symbols for its kind, the default terrain \
and TIN code (GROUND 200). --voltage-class names the class the table must \
carry; the model is not changed. The workspace sync (contract 02) writes the \
same member into a linked workspace; this verb writes it to a file you name.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        super::VOLTAGE_CLASS_ARG,
        Arg::value(
            "out",
            "<file.fea>",
            "The FEA file to write; never overwrites.",
        )
        .required(),
        Arg::value(
            "filename",
            "<workspace path>",
            "The FILENAME= header attribute PLS-CADD records (the member's path in its workspace); defaults to --out.",
        ),
    ],
    output: "\
The member written (path, bytes, sha256), `FEA 15 / SI`, the code count, the \
class, the default codes, the assumed codes and the definitions skipped \
(retired, or without clearances).",
    examples: &[Example {
        command: "ds dsgrid feature-codes export --model local-e9b0ccbf92d7447b --voltage-class MV --out ./nyamagabe-mv.fea",
        note: "The MV table as the FEA 15 member PLS-CADD 16.81 opens.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["feature code", "pls-cadd", "code data"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs.require("out")?;
    crate::apply::validate_output_path(out)?;
    let voltage_class = inputs
        .value("voltage-class")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(class) = &voltage_class
        && !ds_grid_engine::feature_code_standard::VOLTAGE_CLASSES.contains(&class.as_str())
    {
        return Err(Failure::invalid(
            "voltage_class_unknown",
            format!("`{class}` is not a voltage class of the standard"),
        )
        .remedy("pass LV, MV, HV_110 or HV_220"));
    }
    let (target, opened) = super::open_read_only(inputs)?;
    let filename = inputs
        .value("filename")
        .map(str::to_string)
        .unwrap_or_else(|| out.to_string());
    let (bytes, report) = export_fea(
        opened.session.snapshot(),
        &FeaExportOptions {
            voltage_class: voltage_class.clone(),
            filename: Some(filename),
            user: None,
        },
    )
    .map_err(|error| match error {
        FeaExportError::VoltageClassMismatch { requested, found } => Failure::invalid(
            "voltage_class_mismatch",
            format!("the model's codes carry {found:?}, not {requested}"),
        )
        .remedy("import the class first, or export the class the table carries")
        .detail(json!({ "requested": requested, "found": found })),
        FeaExportError::NoClearanceFacts => Failure::invalid(
            "feature_codes_without_clearances",
            "no feature code carries a number and clearances",
        )
        .remedy("run `ds dsgrid feature-codes import` first"),
        FeaExportError::DuplicateNumber { number, a, b } => Failure::invalid(
            "feature_code_number_duplicate",
            format!("codes {a} and {b} share the number {number}"),
        )
        .remedy("retire one of them"),
        FeaExportError::Writer(detail) => {
            Failure::failed("output_unwritable", "the FEA writer refused")
                .remedy("report this with the model")
                .detail(json!({ "detail": detail }))
        }
    })?;
    crate::apply::write_new(out, &bytes)?;
    Ok(json!({
        "target": target,
        "member": {
            "type": "FEA",
            "version": report.version,
            "units": report.units,
            "path": out,
            "byte_len": bytes.len(),
            "sha256": format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)),
        },
        "code_count": report.code_count,
        "voltage_class": report.voltage_class,
        "terrain_point_feature_code": report.terrain_point_feature_code,
        "interp_tin_point_feature_code": report.interp_tin_point_feature_code,
        "assumed_codes": report.assumed_codes,
        "skipped_retired": report.skipped_retired,
        "skipped_without_clearance": report.skipped_without_clearance,
        "staged": false,
        "persisted": true,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "written  {} (FEA {} / {}, {} bytes)\ncodes    {} class {} default terrain/TIN {}/{}\nassumed  {}  skipped retired {}  without clearances {}\n",
        data["member"]["path"].as_str().unwrap_or("?"),
        data["member"]["version"],
        data["member"]["units"].as_str().unwrap_or("SI"),
        data["member"]["byte_len"],
        data["code_count"],
        data["voltage_class"].as_str().unwrap_or("—"),
        data["terrain_point_feature_code"],
        data["interp_tin_point_feature_code"],
        data["assumed_codes"].as_array().map_or(0, Vec::len),
        data["skipped_retired"].as_array().map_or(0, Vec::len),
        data["skipped_without_clearance"]
            .as_array()
            .map_or(0, Vec::len),
    )
}
