//! Survey data materialized through the paired window's Working Area.
//!
//! Project-to-project survey migration is not here: it names both projects
//! and needs no window, so it is `ds survey migrate` on the native client.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

const DESKTOP_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the project is unavailable or the application refuses the Working Area load",
    remedy: "read detail.detail for the application's exact refusal",
};

pub mod download {
    use super::*;

    const ENTIRE_PROJECT_ARG: Arg = Arg::switch(
        "entire-project",
        "Explicitly materialize every survey form in the active project Working Area.",
    )
    .required();

    pub static COMMAND: Command = Command {
        id: "map.survey.download",
        path: &["map", "survey", "download"],
        contract: 2,
        summary: "Materialize survey data through the active Working Area.",
        purpose: "Materializes the CLI-selected project, or the desktop's active project when no CLI project is selected. The shared kernel routes this map-dependent operation through a verified UI project switch when needed. The desktop applies its full-project Working Area and sequential survey loader; only bounded cache counts return.",
        chapter: Chapter::Survey,
        effect: Effect::LocalUi,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[ENTIRE_PROJECT_ARG, DESCRIPTOR_ARG],
        output: "The active project, applied full-project Working Area, form count, and bounded before/after/materialized cache counts. No survey row is returned.",
        examples: &[Example {
            command: "ds map survey download --entire-project --output json",
            note: "Uses the same loader as checking Load entire project survey data and applying Working Area.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::PROJECT_NOT_OPEN,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            DESKTOP_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
        ],
        reference: Some("docs/reference/map.md"),
        search: &[],
        requires: Requires::Window,
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::SURVEY_WORKING_AREA_DOWNLOAD,
            json!({ "entireProject": inputs.switch("entire-project") }),
            crate::SURVEY_DOWNLOAD_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_design_failure)
    }

    fn receipt(result: Value) -> Value {
        json!({
            "project": result["project"],
            "working_area": result["workingArea"],
            "forms": result["forms"],
            "before": result["before"],
            "after": result["after"],
            "cached_total": result["cachedTotal"],
            "materialized": result["materialized"],
            "materialized_total": result["materializedTotal"],
            "rows_returned": 0,
        })
    }

    pub fn render(data: &Value) -> String {
        format!(
            "survey cache materialized  {}\n  forms {}  ·  cached {}  ·  newly materialized {}\n  Working Area: full project  ·  raw rows returned: 0\n",
            data["project"].as_str().unwrap_or("?"),
            data["forms"].as_u64().unwrap_or(0),
            data["cached_total"].as_u64().unwrap_or(0),
            data["materialized_total"].as_u64().unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_area_download_declares_only_explicit_intent() {
        assert_eq!(download::COMMAND.effect, Effect::LocalUi);
        assert_eq!(download::COMMAND.authority, Authority::Project);
        assert!(download::COMMAND.args[0].required);
        assert_eq!(
            crate::SURVEY_WORKING_AREA_DOWNLOAD.arguments,
            &["entireProject"]
        );
    }

    /// The window's survey operation is the Working Area load alone; the
    /// migration it once relayed is `ds survey migrate`.
    #[test]
    fn no_survey_migration_travels_through_the_window() {
        assert!(
            crate::BRIDGE_OPS
                .iter()
                .all(|op| !op.operation.starts_with("survey.migrate"))
        );
    }
}
