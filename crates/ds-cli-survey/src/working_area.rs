//! `ds survey working-area forms|select|clear` — which survey forms the map's
//! working area loads for a project on this host.
//!
//! The working area used to load every active form's entries by default. It
//! now loads exactly the forms the operator selected here, and until they
//! choose it loads nothing: `forms` reads the catalogue and the choice,
//! `select` replaces the choice (an explicit list, `--all` or `--none`) and
//! `clear` forgets it. The decision is the kernel's
//! (`survey_working_area_forms`), the owner every host calls is
//! `ds_layer_ops::working_area_forms`, and `--target` picks the host exactly
//! as `ds map layer …` does: the desktop's own native client, or the running
//! Server for the project named.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_server::target::{self, Target};
use serde_json::Value;

pub const FORMS: &str = "survey.working-area.forms";
pub const SELECT: &str = "survey.working-area.select";
pub const CLEAR: &str = "survey.working-area.clear";

const FORM_ARG: Arg = Arg {
    name: "form",
    kind: ArgKind::Repeated,
    value: "<slug>",
    required: false,
    default: None,
    choices: &[],
    summary: "Form slug from `ds survey working-area forms`. Repeat for several.",
};

pub const UNKNOWN_FORM: Refusal = Refusal {
    code: "unknown_form",
    when: "a --form slug is not a survey form of the project (named in the message)",
    remedy: "copy slugs from `ds survey working-area forms --output json` (.data.forms[].slug)",
};
pub const NO_FORMS_NAMED: Refusal = Refusal {
    code: "no_forms_named",
    when: "select was given no --form, --all or --none",
    remedy: "pass --form <slug> (repeatable), --all, or --none",
};
pub const AMBIGUOUS_SELECTION: Refusal = Refusal {
    code: "ambiguous_selection",
    when: "more than one of --form, --all and --none was passed",
    remedy: "pass --form <slug>… alone, --all alone, or --none alone",
};
pub const KERNEL_REFUSED: Refusal = Refusal {
    code: "working_area_forms_refused",
    when: "the shared kernel refused the question this build asked",
    remedy: "update ds and report the working-area forms contract failure",
};
pub const LOCAL_STORE: Refusal = Refusal {
    code: "local_layer_refused",
    when: "the machine-local selection store cannot be read or persisted",
    remedy: "check the local data directory; DS_LAYER_HOME may name an absolute shared directory",
};
pub const LAYER_STATE_STALE: Refusal = Refusal {
    code: "project_context_changed",
    when: "the layer document belongs to another project than the one named or selected",
    remedy: "run the command again against the current selected project",
};

/// The native vocabulary every project-forms read already declares, plus the
/// host routing this family takes and the refusals the choice itself owns.
/// `OWN` is the full length: the base's 18 native codes (its 19th,
/// `invalid_number`, is not raised here; the base already carries
/// `project_required` and `context_corrupt`), the four host refusals, then `own`.
const fn refusals<const OWN: usize>(own: &[Refusal]) -> [Refusal; OWN] {
    const BASE: &[Refusal] = crate::project_forms::LIST_COMMAND.refusals;
    const HOSTED: [Refusal; 4] = [
        target::TARGET_INSTANCE_UNSUPPORTED,
        target::UNKNOWN_TARGET,
        target::SERVER_REFUSED,
        target::SERVER_OWNER_CHANGED,
    ];
    assert!(OWN == 18 + HOSTED.len() + own.len());
    let mut list = [UNKNOWN_FORM; OWN];
    let mut i = 0;
    while i < 18 {
        list[i] = BASE[i];
        i += 1;
    }
    let mut j = 0;
    while j < HOSTED.len() {
        list[18 + j] = HOSTED[j];
        j += 1;
    }
    let mut k = 0;
    while k < own.len() {
        list[18 + HOSTED.len() + k] = own[k];
        k += 1;
    }
    list
}

const READ_REFUSALS: [Refusal; 25] = refusals(&[LOCAL_STORE, LAYER_STATE_STALE, KERNEL_REFUSED]);
const SELECT_REFUSALS: [Refusal; 28] = refusals(&[
    LOCAL_STORE,
    LAYER_STATE_STALE,
    KERNEL_REFUSED,
    UNKNOWN_FORM,
    NO_FORMS_NAMED,
    AMBIGUOUS_SELECTION,
]);

pub static FORMS_COMMAND: Command = Command {
    id: FORMS,
    path: &["survey", "working-area", "forms"],
    contract: 1,
    summary: "Which survey forms the map's working area loads for a project.",
    purpose: "Answers, before any survey data is fetched, which forms the working area will load on this host for one project: every active form of the project (from the layer catalogue the map already reads — no new backend read) with whether it is selected, the exact list that loads, and whether the operator ever chose here. Until a choice is made the working area loads NOTHING and the answer carries the remedy; the map never loads every form by default. Use it to filter a large project down to the forms you are working on, or to see why the map shows no survey entries.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::LANE,
        target::TARGET_ARG,
        target::PROJECT_ARG,
        target::STATE_DIR_ARG,
    ],
    output: "Lane, project, chosen (did the operator ever choose here), form_count, selected_count, loads (the slugs the working area loads, in catalogue order), stale (chosen slugs the project no longer has), forms[] with slug, label, selected, layer_ids and count (null unless already known), remedy when nothing loads, selection_source: native_local.",
    examples: &[
        Example {
            command: "ds survey working-area forms --output json",
            note: "Read the selected project's forms and which of them the working area loads on this machine.",
            runnable: false,
        },
        Example {
            command: "ds survey working-area forms --project <project-id> --target server --output json",
            note: "The same answer from the running Server for the project named.",
            runnable: false,
        },
    ],
    refusals: &READ_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "form filter",
        "load forms",
        "survey layers",
        "load all",
        "entries loaded",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static SELECT_COMMAND: Command = Command {
    id: SELECT,
    path: &["survey", "working-area", "select"],
    contract: 1,
    summary: "Choose which survey forms the working area loads for a project.",
    purpose: "Replaces the working-area form selection for one project on this host: exactly the forms named with --form, every active form with --all, or no form with --none. The choice persists per lane, account and project, the map and `survey working-area forms` read it, and nothing is fetched here — the next working-area load fetches only the selected forms. Idempotent: choosing what is already chosen changes nothing. A slug that is not a form of the project is refused by name and nothing is written.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        FORM_ARG,
        Arg::switch("all", "Select every active survey form of the project."),
        Arg::switch(
            "none",
            "Select no form: the working area loads nothing until chosen again.",
        ),
        crate::LANE,
        target::TARGET_ARG,
        target::PROJECT_ARG,
        target::STATE_DIR_ARG,
    ],
    output: "The same projection as `survey working-area forms` after the change, plus changed, next_selection (what was persisted), persisted: native_local and the store revision.",
    examples: &[
        Example {
            command: "ds survey working-area select --form <form-slug> --form <form-slug> --output json",
            note: "The working area now loads these two forms and no other; copy the slugs from `ds survey working-area forms`.",
            runnable: false,
        },
        Example {
            command: "ds survey working-area select --all --project <project-id> --target server",
            note: "Every form, on the Server, for the project named.",
            runnable: false,
        },
    ],
    refusals: &SELECT_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "form filter",
        "load forms",
        "filter forms",
        "only these",
        "load all",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static CLEAR_COMMAND: Command = Command {
    id: CLEAR,
    path: &["survey", "working-area", "clear"],
    contract: 1,
    summary: "Forget the working-area form choice for a project on this host.",
    purpose: "Removes the persisted form selection for one project on this host, back to never chosen: the working area loads nothing until `survey working-area select` is run again. Differs from `select --none`, which records an explicit empty choice. Idempotent.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::LANE,
        target::TARGET_ARG,
        target::PROJECT_ARG,
        target::STATE_DIR_ARG,
    ],
    output: "The same projection as `survey working-area forms` after the change (chosen: false), plus changed, persisted: native_local and the store revision.",
    examples: &[Example {
        command: "ds survey working-area clear --output json",
        note: "Back to never chosen for the selected project on this machine.",
        runnable: false,
    }],
    refusals: &READ_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["form filter", "reset forms", "forget selection", "unselect"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn forms(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if target::resolve(inputs)? == Target::Server {
        return ds_cli_server::working_area_forms_read(inputs, context);
    }
    let mut documents = target::desktop_documents(inputs)?;
    ds_layer_ops::working_area_forms::read(&mut documents, &ds_layer_ops::Preferences::native()?)
}

pub fn select(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if target::resolve(inputs)? == Target::Server {
        return ds_cli_server::working_area_forms_select(inputs, context);
    }
    let request = ds_layer_ops::working_area_forms::SelectRequest {
        forms: inputs.repeated("form").to_vec(),
        all: inputs.switch("all"),
        none: inputs.switch("none"),
    };
    let mut documents = target::desktop_documents(inputs)?;
    ds_layer_ops::working_area_forms::select(
        &mut documents,
        &ds_layer_ops::Preferences::native()?,
        &request,
    )
}

pub fn clear(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if target::resolve(inputs)? == Target::Server {
        return ds_cli_server::working_area_forms_clear(inputs, context);
    }
    let mut documents = target::desktop_documents(inputs)?;
    ds_layer_ops::working_area_forms::clear(&mut documents, &ds_layer_ops::Preferences::native()?)
}

pub fn render(data: &Value) -> String {
    ds_layer_ops::working_area_forms::render(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_commands_declare_the_same_host_arguments_as_the_layer_drawer() {
        for command in [&FORMS_COMMAND, &SELECT_COMMAND, &CLEAR_COMMAND] {
            for name in ["lane", "target", "project", "state-dir"] {
                assert!(
                    command.arg(name).is_some(),
                    "{} declares --{name}",
                    command.id
                );
            }
            assert!(matches!(command.requires, Requires::Server));
        }
        assert_eq!(
            SELECT_COMMAND.refusals.len(),
            READ_REFUSALS.len() + 3,
            "select adds exactly its own three refusals"
        );
        assert!(
            READ_REFUSALS
                .iter()
                .all(|refusal| refusal.code != "invalid_number"),
            "no number is parsed here"
        );
    }
}
