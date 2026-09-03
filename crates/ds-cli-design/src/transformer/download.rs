//! `ds design transformer download` — materialize saved transformer rooms in
//! the paired application's local cache without entering an edit context.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const TRANSFORMER_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Exact active transformer room to download. Repeat for an explicit set; omit for all active transformers.",
};

const FORCE_ARG: Arg = Arg {
    name: "force",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Refresh clean cached rooms even when their saved server version is already current; dirty rooms remain untouched.",
};

pub static COMMAND: Command = Command {
    id: "design.transformer.download",
    path: &["design", "transformer", "download"],
    contract: 1,
    summary: "Bulk-download saved transformer rooms into the paired local cache.",
    purpose: "\
Materializes saved transformer rooms in the paired application's local cache \
for local reporting and other background work. Repeat --transformer for an \
explicit set, or omit it for every active non-special transformer. This is \
the canonical local transformer room, bulk download, and background report \
preparation command. It does not open the map, activate a transformer edit \
context, process geometry, stage edits, save, or create a version. Current \
clean rooms are reused; a dirty local room is never overwritten, including \
with --force.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, FORCE_ARG, DESCRIPTOR_ARG],
    output: "\
A bounded cache receipt from the paired application naming the project and \
requested scope, total/local counts, downloaded and already-local rooms, \
dirty rooms preserved, per-transformer failures and cancellation state. It \
reports staged=false, persisted=false and context_changed=false.",
    examples: &[
        Example {
            command: "ds design transformer download --output json",
            note: "Downloads every active non-special transformer without opening the map or changing the visible editor context.",
            runnable: false,
        },
        Example {
            command: "ds design transformer download --transformer agasharu --transformer gitega --output json",
            note: "Materializes exactly two saved rooms; already-current rooms are reused.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SIGNED_OUT,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        super::INVALID_SCOPE,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs, false)?;
    let mut arguments = Map::new();
    if !requested.is_empty() {
        arguments.insert("transformers".into(), json!(requested.names()));
    }
    if inputs.switch("force") {
        arguments.insert("force".into(), Value::Bool(true));
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TRANSFORMER_DOWNLOAD,
        Value::Object(arguments),
        crate::LOCAL_ROOM_DOWNLOAD_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let project = data["project"].as_str().unwrap_or("?");
    let total = data["total"].as_u64().unwrap_or(0);
    let count = |key: &str| {
        data[key]
            .as_array()
            .map_or_else(|| data[key].as_u64().unwrap_or(0), |rows| rows.len() as u64)
    };
    let downloaded = count("downloaded");
    let already_local = count("already_local");
    let dirty = count("dirty_preserved");
    let failed = count("failed");
    let cancelled = count("cancelled");
    let local = downloaded + already_local + dirty;

    let mut out = format!(
        "local transformer rooms in {project}: {local}/{total} · {downloaded} downloaded · {already_local} already local"
    );
    if dirty > 0 {
        out.push_str(&format!(" · {dirty} dirty preserved"));
    }
    if failed > 0 {
        out.push_str(&format!(" · {failed} failed"));
    }
    if cancelled > 0 {
        out.push_str(&format!(" · {cancelled} cancelled"));
    }
    out.push('\n');

    if let Some(failures) = data["failed"].as_array() {
        for row in failures {
            out.push_str(&format!(
                "  {} · {}\n",
                row["name"].as_str().unwrap_or("?"),
                row["message"].as_str().unwrap_or("download failed"),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;
    use ds_cli_contract::{Format, Output};

    fn inputs(arguments: &[&str]) -> Inputs {
        parse(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("valid download arguments")
    }

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output {
                format: Format::Json,
                pretty: false,
                color: false,
            },
        }
    }

    #[test]
    fn omitted_transformer_reaches_pairing_as_the_all_active_scope() {
        let failure = run(
            &inputs(&["--desktop-descriptor", "/missing/ds-session.json"]),
            &context(),
        )
        .expect_err("the synthetic descriptor is absent");
        assert!(
            [
                "desktop_unreachable",
                "desktop_not_paired",
                "desktop_unreadable"
            ]
            .contains(&failure.code()),
            "unexpected refusal: {}",
            failure.code(),
        );
    }

    #[test]
    fn malformed_explicit_name_refuses_before_pairing() {
        let failure = run(&inputs(&["--transformer", " agasharu"]), &context())
            .expect_err("an untrimmed exact name must refuse");
        assert_eq!(failure.code(), "invalid_transformer_scope");
    }

    #[test]
    fn operation_declares_only_the_application_contract() {
        assert_eq!(crate::TRANSFORMER_DOWNLOAD.operation, COMMAND.id);
        assert_eq!(
            crate::TRANSFORMER_DOWNLOAD.arguments,
            &["transformers", "force"]
        );
    }
}
