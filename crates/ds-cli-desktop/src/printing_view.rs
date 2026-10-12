//! Consume the paired printing view through the native held-view contract.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG, TARGET_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::printing::view::Request;
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

pub const OP: BridgeOp = BridgeOp {
    operation: "printing.view",
    arguments: &["project", "location", "page", "catalogue"],
};
const INVALID: Refusal = Refusal {
    code: "printing_request_invalid",
    when: "The explicit project, typed location, page or combination is invalid",
    remedy: "Use the current Desktop project and either an exact catalogue location or a positive page number",
};
const NOT_OPEN: Refusal = Refusal {
    code: "printing_view_not_open",
    when: "The current project printing view is not open and ready",
    remedy: "Run map ui open --target project-printing --ref <project>, then retry",
};
const TEMPLATE: Refusal = Refusal {
    code: "printing_view_template_unavailable",
    when: "The exact pinned location is absent from the held project catalogue",
    remedy: "Use an exact location from desktop printing view --project <id> --catalogue; explicitly refresh the library if needed",
};
const PAGE: Refusal = Refusal {
    code: "printing_view_page_unavailable",
    when: "The requested page is not in the current held document",
    remedy: "Read desktop printing view and choose a page within its actual page_count",
};
const DIRTY: Refusal = Refusal {
    code: "printing_discard_unconfirmed",
    when: "Selecting another template would replace an unsaved print draft",
    remedy: "Save or explicitly discard the draft in its current editor before selecting another template",
};
const SUPERSEDED: Refusal = Refusal {
    code: "printing_view_superseded",
    when: "The account, project or print draft changed during selection",
    remedy: "Retry against the current project view after preserving its draft",
};

pub static COMMAND: Command = Command {
    id: "desktop.printing.view",
    path: &["desktop", "printing", "view"],
    contract: 1,
    summary: "Read, select a template or navigate a held printing page.",
    purpose: "Requires the current signed-in project's open Printing templates view. With no selection arguments reads its actual template and held page count; --catalogue includes its already-held native groups, preview subjects and exact location pins. --location consumes one such native Location from a bounded JSON file; --page changes the held PDF page camera. Select first, then navigate pages. The native report.layout.edit view_plan admits the exact catalogue pins and page bounds and refuses replacement of dirty work. No arbitrary UI automation, acquisition, refresh, export, save, renderer restart or project switch; existing preview jobs continue independently.",
    chapter: Chapter::Reports,
    effect: Effect::LocalUi,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Current Desktop project whose printing view is consumed.",
        )
        .required(),
        Arg::value(
            "location",
            "<json-file>",
            "Exact location object from the native template catalogue, at most 4096 bytes; mutually exclusive with --page.",
        ),
        Arg::value(
            "page",
            "<n>",
            "Positive held document page number; does not request additional MV atlas sheets.",
        ),
        TARGET_ARG,
        Arg {
            name: "catalogue",
            kind: ArgKind::Switch,
            value: "",
            required: false,
            default: None,
            choices: &[],
            summary: "Include the already-held native template groups, subject and exact location pins; no acquisition or refresh.",
        },
        DESCRIPTOR_ARG,
    ],
    output: "Exact project, shown template location, opened printout name, actual held page_count and current_page. --catalogue adds already-held native groups/subject/location pins. No design features or credentials.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        INVALID,
        NOT_OPEN,
        TEMPLATE,
        PAGE,
        DIRTY,
        SUPERSEDED,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &["printing navigation", "template selection", "print pages"],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

fn invalid(message: impl ToString) -> Failure {
    Failure::invalid(INVALID.code, message.to_string()).remedy(INVALID.remedy)
}

fn arguments(inputs: &Inputs, location: Option<Value>) -> Result<Value, Failure> {
    let page = inputs
        .value("page")
        .map(|value| value.parse::<usize>().map_err(invalid))
        .transpose()?;
    let request = Request {
        project: inputs.require("project")?.into(),
        location: location
            .map(serde_json::from_value)
            .transpose()
            .map_err(invalid)?,
        page,
    };
    ds_command_kernel::printing::view::validate(&request).map_err(invalid)?;
    let mut args = json!(request);
    if inputs.switch("catalogue") {
        args["catalogue"] = json!(true);
    }
    Ok(args)
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let location = inputs
        .value("location")
        .map(|path| {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(invalid)?
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(invalid)?;
            if bytes.len() > 4096 {
                return Err(invalid("template location exceeds 4096 bytes"));
            }
            serde_json::from_slice(&bytes).map_err(invalid)
        })
        .transpose()?;
    let args = arguments(inputs, location)?;
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &OP,
        args,
        Duration::from_secs(240),
    )
    .map_err(ops::classify_signed_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(args: &[&str]) -> Result<Inputs, Failure> {
        ds_cli_contract::args::parse(
            &COMMAND,
            &args
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn closed_view_transport_uses_the_native_request_and_preserves_exact_pins() {
        let inputs = inputs(&["--project", "p1", "--location", "location.json"]).unwrap();
        let location = json!({"kind":"governed_standard","project_id":"p1","id":"cover","template_id":"cover","revision_id":"r1","content_sha256":"a".repeat(64)});
        assert_eq!(
            arguments(&inputs, Some(location.clone())).unwrap(),
            json!({"project":"p1","location":location})
        );
        assert_eq!(OP.arguments, ["project", "location", "page", "catalogue"]);
        assert_eq!(COMMAND.effect, Effect::LocalUi);
    }
    #[test]
    fn invalid_selection_and_provider_options_refuse_before_pairing() {
        for flag in ["--refresh", "--acquire", "--force", "--selector"] {
            assert!(inputs(&["--project", "p1", flag]).is_err());
        }
        for page in ["0", "-1", "many"] {
            assert!(
                arguments(&inputs(&["--project", "p1", "--page", page]).unwrap(), None).is_err()
            );
        }
        let selected = inputs(&["--project", "p1", "--page", "2"]).unwrap();
        assert!(arguments(&selected, Some(json!({"kind":"layout","id":"a0"}))).is_err());
        assert!(
            arguments(
                &selected,
                Some(json!({"kind":"layout","id":"a0","selector":"canvas"}))
            )
            .is_err()
        );
    }

    #[test]
    fn catalogue_is_an_explicit_held_response_switch_without_an_acquisition_option() {
        let ask = inputs(&["--project", "p1", "--catalogue"]).unwrap();
        assert_eq!(
            arguments(&ask, None).unwrap(),
            json!({"project":"p1","catalogue":true})
        );
        assert!(inputs(&["--project", "p1", "--catalogue", "--refresh"]).is_err());
    }
}
