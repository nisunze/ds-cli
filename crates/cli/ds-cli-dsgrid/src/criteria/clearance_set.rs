//! `ds dsgrid criteria clearance set` — fill a criterion set's clearance
//! criteria from a voltage class of the standard, as one revision.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_model::CriterionSetId;
use ds_network_contracts::grid_operation::{StandardInputV1, TypedMutationVerbV1};
use serde_json::Value;

use crate::feature_codes;
use crate::mutation;

const OWN: &[Refusal] = &[
    feature_codes::STANDARD_REFUSALS[0],
    feature_codes::STANDARD_REFUSALS[1],
    feature_codes::STANDARD_REFUSALS[2],
    feature_codes::STANDARD_REFUSALS[3],
    Refusal {
        code: "clearance_case_missing",
        when: "--vertical-case or --horizontal-case names a weather label the model's weather set does not carry",
        remedy: "use a label from the receipt's `available` list (`ds dsgrid criteria show` prints the weather cases)",
    },
    Refusal {
        code: "criterion_set_missing",
        when: "the model has no criterion set",
        remedy: "convert the workspace with its CRI member, or author a criterion set first",
    },
    Refusal {
        code: "criterion_set_ambiguous",
        when: "the model has several criterion sets and --criterion-set was not given",
        remedy: "pass --criterion-set <id> from `ds dsgrid criteria show`",
    },
    Refusal {
        code: "criterion_set_not_found",
        when: "--criterion-set is not a criterion set of the model",
        remedy: "pass an id from `ds dsgrid criteria show`",
    },
    Refusal {
        code: "invalid_voltage",
        when: "--voltage-kv is not a positive number",
        remedy: "pass the nominal clearance voltage in kV (30 for Rwanda MV)",
    },
];

const REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < mutation::REFUSALS.len() {
        all[OWN.len() + shared] = mutation::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.criteria.clearance.set",
    path: &["dsgrid", "criteria", "clearance", "set"],
    contract: 1,
    summary: "Set clearance voltage and survey-point clearance cases from a class.",
    purpose: "\
Fills the criterion set's clearance criteria the way PLS-CADD's Criteria › \
Voltage and Criteria › Survey Point Clearances hold them: the clearance \
voltage of the class (30 kV for MV, overridable with --voltage-kv), the \
vertical survey-point clearance case at the named weather label (REG: the \
maximum conductor temperature, final), the horizontal (blow-out) case at the \
named label (high wind), and the wire clearance line at the class terrain \
floor. A label the model's weather set does not carry is refused by name \
with the labels it does carry. A vertical case colder than the 75 °C REG \
rates conductors at is a finding in the receipt, not a refusal. One revision \
of a working copy, or one new package; the sync (contract 02) writes CRI 94 \
from it.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        feature_codes::STANDARD_ARG,
        feature_codes::VOLTAGE_CLASS_ARG.required(),
        Arg::value(
            "vertical-case",
            "<weather label>",
            "The weather case of the vertical survey-point clearance (the maximum conductor temperature case).",
        )
        .required(),
        Arg::value(
            "horizontal-case",
            "<weather label>",
            "The weather case of the horizontal (blow-out) survey-point clearance (the high-wind case).",
        )
        .required(),
        Arg::value(
            "voltage-kv",
            "<kV>",
            "Override the class's clearance voltage.",
        ),
        Arg::value(
            "criterion-set",
            "<id>",
            "The criterion set to fill; the model's only set when omitted.",
        ),
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
    ],
    output: "\
The family receipt plus the set, the class and voltage, the vertical and \
horizontal cases (analysis case, weather, temperature, wind, condition), the \
wire clearance line, the rows authored/updated and the findings.",
    examples: &[
        Example {
            command: "ds dsgrid criteria clearance set --model local-e9b0ccbf92d7447b --voltage-class MV --vertical-case \"Maximum Conductor Temperature\" --horizontal-case \"High wind\" --yes",
            note: "30 kV, vertical at the maximum conductor temperature, horizontal at high wind.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "survey point",
        "ground clearance",
        "weather case",
        "30 kv",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let standard = feature_codes::standard_input(inputs)?;
    let voltage_class = inputs.require("voltage-class")?.trim().to_string();
    let voltage_kv = match inputs.value("voltage-kv") {
        Some(raw) => Some(
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| *v > 0.0)
                .ok_or_else(|| {
                    Failure::invalid("invalid_voltage", "--voltage-kv must be a positive number")
                        .remedy("pass the nominal clearance voltage in kV")
                })?,
        ),
        None => None,
    };
    let criterion_set_id = match inputs.value("criterion-set") {
        Some(raw) => Some(
            CriterionSetId::new(raw.trim())
                .map_err(|error| {
                    Failure::invalid("criterion_set_not_found", error.to_string())
                        .remedy("pass an id from `ds dsgrid criteria show`")
                })?
                .as_str()
                .to_owned(),
        ),
        None => None,
    };
    let target = mutation::Target::resolve(inputs, writing)?;
    let (standard, staged) = match standard {
        None => (None, Vec::new()),
        Some((label, bytes)) => (
            Some(StandardInputV1 {
                input: "standard.json".into(),
                label,
            }),
            vec![("standard.json".to_owned(), bytes)],
        ),
    };
    mutation::run(
        target,
        TypedMutationVerbV1::CriteriaClearanceSet {
            standard,
            voltage_class,
            vertical_case: inputs.require("vertical-case")?.to_string(),
            horizontal_case: inputs.require("horizontal-case")?.to_string(),
            voltage_kv,
            criterion_set_id,
        },
        inputs,
        writing,
        &staged,
    )
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "set      {} ({})  class {}  voltage {} kV  wire clearance line {} m\nvertical {} ({} °C, {} Pa)  horizontal {} ({} °C, {} Pa)\n",
        data["criterion_set_id"].as_str().unwrap_or("?"),
        data["criterion_set_label"].as_str().unwrap_or("?"),
        data["voltage_class"].as_str().unwrap_or("?"),
        data["clearance_voltage_kv"],
        data["wire_clearance_line_m"],
        data["vertical"]["weather_label"].as_str().unwrap_or("?"),
        data["vertical"]["temperature_c"],
        data["vertical"]["wind_pressure_pa"],
        data["horizontal"]["weather_label"].as_str().unwrap_or("?"),
        data["horizontal"]["temperature_c"],
        data["horizontal"]["wind_pressure_pa"],
    ));
    for finding in data["findings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "finding  {}: {}\n",
            finding["code"].as_str().unwrap_or("?"),
            finding["detail"].as_str().unwrap_or("")
        ));
    }
    out
}
