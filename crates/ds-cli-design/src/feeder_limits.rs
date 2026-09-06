//! Bounded project feeder settings through the native selected-project owner.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Value, json};
use std::io::Write;

const MINIMUM: Arg = Arg::value(
    "minimum",
    "<mm2>",
    "Minimum feeder conductor area in mm²; positive and at most maximum.",
)
.required();
const MAXIMUM: Arg = Arg::value(
    "maximum",
    "<mm2>",
    "Maximum feeder conductor area in mm²; not a transformer ampere rating.",
)
.required();
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Optionally retain fresh configuration at a new local JSON path.",
);

pub(crate) const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_catalog_page",
        when: "catalog paging inputs are invalid",
        remedy: "use a nonnegative offset and a limit between 1 and 100",
    },
    ds_cli_report::project::NATIVE_PROFILE,
    ds_cli_report::project::NATIVE_PROFILE_DIGEST,
    ds_cli_report::project::NATIVE_PROFILE_UNSAFE,
    ds_cli_report::project::HEADLESS_SIGNED_OUT,
    ds_cli_report::project::HEADLESS_NO_PROJECT,
    ds_cli_report::project::PROJECT_CONTEXT_STALE,
    ds_cli_report::project::NATIVE_STATE_UNSAFE,
    ds_cli_report::project::NATIVE_STATE_UNAVAILABLE,
    ds_cli_report::project::NATIVE_STATE_PROTECTION,
    ds_cli_report::project::NATIVE_STATE_ROOT,
    ds_cli_report::project::NATIVE_STATE_CONFLICT,
    ds_cli_report::project::NATIVE_CLEANUP,
    ds_cli_report::project::AUTH_CONTEXT_MISMATCH,
    ds_cli_report::project::AUTH_REVOKED,
    ds_cli_report::project::AUTH_IDENTITY_MISMATCH,
    Refusal {
        code: "auth_input_invalid",
        when: "the configuration owner rejected a limit, category or conflicting alias",
        remedy: "inspect the current seed and correct the requested values",
    },
    Refusal {
        code: "auth_rejected",
        when: "the gateway refused access to the selected project's configuration",
        remedy: "verify the account's project access and configuration permissions",
    },
    Refusal {
        code: "auth_transient",
        when: "the configuration service is temporarily unavailable",
        remedy: "read current configuration before retrying a save whose outcome is uncertain",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "configuration or saved-value verification violated its contract",
        remedy: "read current configuration and report the malformed response; do not blindly repeat a save",
    },
    Refusal {
        code: "invalid_feeder_limit",
        when: "limits are nonpositive, nonfinite, or reversed",
        remedy: "supply positive mm2 values with minimum at most maximum",
    },
    Refusal {
        code: "configuration_output_exists",
        when: "the optional output path already exists",
        remedy: "choose a new output path",
    },
    Refusal {
        code: "configuration_output_write_failed",
        when: "the fresh configuration could not be retained locally",
        remedy: "inspect saved configuration and choose a writable new output path",
    },
    Refusal {
        code: "confirmation_required",
        when: "a settings write lacks --yes",
        remedy: "review the requested configuration change and pass --yes to save it",
    },
];

const fn command(
    id: &'static str,
    path: &'static [&'static str],
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        chapter: Chapter::Design,
        authority: Authority::HeadlessProject,
        effect,
        execution: Execution::Sync,
        summary: "Read or set the project's feeder cable limits.",
        purpose: "Reads the native selected project's fresh network configuration without Desktop. Set changes only minimum_feeder_cable_size and max_feeder_cable_size through the existing governed configuration owner and verifies both with a fresh read. Does not process or resize design geometry. Optional --out retains the fresh configuration for native report inputs.",
        args,
        output: "Project, the two feeder limits in mm², existing LV cable limits, transformer-to-LV-to-feeder catalog rows, and saved state.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        availability: ds_cli_auth::native_availability,
    }
}
pub static READ: Command = command(
    "design.feeder-limits.read",
    &["design", "feeder-limits", "read"],
    Effect::LocalFileWrite,
    &[LANE, OUT],
);
pub static SET: Command = command(
    "design.feeder-limits.set",
    &["design", "feeder-limits", "set"],
    Effect::GlobalWrite,
    &[MINIMUM, MAXIMUM, LANE, OUT],
);

fn invoke(inputs: &Inputs, write: bool) -> Result<Value, Failure> {
    let bounds = if write {
        let parse = |key| {
            inputs
                .require(key)?
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| {
                    Failure::invalid(
                        "invalid_feeder_limit",
                        "feeder limits must be positive finite mm² values",
                    )
                })
        };
        let min = parse("minimum")?;
        let max = parse("maximum")?;
        if min > max {
            return Err(Failure::invalid(
                "invalid_feeder_limit",
                "minimum must not exceed maximum",
            ));
        }
        Some((min, max))
    } else {
        None
    };
    if inputs
        .value("out")
        .is_some_and(|path| std::path::Path::new(path).exists())
    {
        return Err(Failure::invalid(
            "configuration_output_exists",
            "configuration output must not already exist",
        ));
    }
    let receipt = ds_cli_auth::feeder_configuration(inputs.require("lane")?, bounds)?;
    let mut summary = receipt.summary;
    if let Some(path) = inputs.value("out") {
        let bytes =
            serde_json::to_vec_pretty(&receipt.document).expect("JSON configuration serializes");
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
            .map_err(|_| Failure::invalid("configuration_output_write_failed", "cannot create configuration output; inspect saved state before repeating a write"))?;
        file.write_all(&bytes).and_then(|_| file.sync_all())
            .map_err(|_| Failure::invalid("configuration_output_write_failed", "cannot retain configuration output; inspect saved state before repeating a write"))?;
        summary["configuration_path"] = json!(path);
    }
    Ok(summary)
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, false)
}
pub fn set(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, true)
}
pub fn render(data: &Value) -> String {
    format!(
        "{} · feeder limits {}–{} mm² · saved={}\n",
        data["project"], data["minimum"], data["maximum"], data["saved"]
    )
}
