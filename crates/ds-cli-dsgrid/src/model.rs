//! `ds dsgrid model` — the canonical way a project acquires a DS Grid model.
//!
//! Three ways in, one family. A project model can begin from nothing, from a
//! `.dsgrid` package the operator already verified, or from a PLS-CADD
//! workspace. Those are three sources for one act — registering a model head
//! and its first immutable revision against a project — so they are three
//! verbs of one family rather than three commands scattered across the
//! domains that happen to own each source.
//!
//! There is a fourth way in and it deliberately stays where it is:
//! `ds library global fork-example` copies one governed catalogue example
//! into a project model. That command belongs to library governance because
//! what it authorizes is *reading the global catalogue*; the project model is
//! its result, not its subject. Adding a `fork` verb here would be a second
//! description of a command, which is the first thing to reject in review, so
//! this family names it instead.
//!
//! ## Why every verb refuses today
//!
//! ds-brain owns the whole contract already: `POST /grid/models` takes
//! `start_upload`, `create_version`, `fork_example`, `delete_model` and the
//! read actions, and gates each on a project capability. What does not exist
//! is a route from `ds` to it. The paired application publishes exactly one
//! grid-model operation on its closed CLI bridge — `catalog.fork-example` —
//! and `ds-client-core` publishes none, so the three verbs here have no owner
//! to call.
//!
//! The choice at that point is between guessing and refusing. Guessing would
//! mean a generic HTTP client, an ambient service account, or reading the
//! project directly — each of which this repository exists to prevent. So the
//! family declares its complete contract, refuses closed with one code that
//! names the missing operation, and routes the caller to the local
//! preparation that does work today.
//! [`docs/contracts/project-grid-model-contract.md`](../../../docs/contracts/project-grid-model-contract.md)
//! enumerates the exact ds-web operations and ds-brain actions required.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

/// The model kinds ds-brain accepts (`projectGridModelKinds`). Enforced as a
/// closed choice set so a caller learns them from help rather than from a
/// round trip that cannot happen yet.
pub const MODEL_KINDS: &[&str] = &["general", "lv_network", "mv_line"];

/// The one refusal every verb in this family answers with, and the only code
/// any of its handlers can construct. Named for the missing capability rather
/// than for a generic outage, so a caller branching on `error.code` can tell
/// "this is not built yet" from "your desktop is not running".
const UNSUPPORTED: Refusal = Refusal {
    code: "project_model_registration_unsupported",
    when: "no reviewed owner exists yet for registering a project DS Grid model",
    remedy: "prepare the package locally, then register it in DS GridDesign",
};

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "Display name for the project model head.",
};

const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<kind>",
    required: true,
    default: None,
    choices: MODEL_KINDS,
    summary: "Which model family this is, as ds-brain classifies it.",
};

const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Longer description stored on the model head.",
};

const REASON_ARG: Arg = Arg {
    name: "reason",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "Why this revision exists. Stored on the revision, never inferred.",
};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "The verified .dsgrid package to register.",
};

const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "The PLS-CADD workspace directory or .bak to convert and register.",
};

const MODEL_ID_ARG: Arg = Arg {
    name: "model-id",
    kind: ArgKind::Value,
    value: "<id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Register as a new revision of this existing model instead of a new one.",
};

const EXPECTED_HEAD_ARG: Arg = Arg {
    name: "expected-head",
    kind: ArgKind::Value,
    value: "<revision-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "The head revision this one must follow. Required with --model-id.",
};

const PROJECT_ARG: Arg = Arg {
    name: "project",
    kind: ArgKind::Value,
    value: "<id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Refuse unless the paired session has exactly this project selected.",
};

const CREATE_REFUSALS: &[Refusal] = &[UNSUPPORTED];

const IMPORT_REFUSALS: &[Refusal] = &[
    UNSUPPORTED,
    Refusal {
        code: "model_not_found",
        when: "--model does not name a readable .dsgrid package",
        remedy: "check the path; `ds dsgrid validate --model <path>` proves it first",
    },
    Refusal {
        code: "expected_head_required",
        when: "--model-id was given without --expected-head",
        remedy: "pass the head revision the new one must follow, or omit --model-id",
    },
];

const CONVERT_REFUSALS: &[Refusal] = &[
    UNSUPPORTED,
    Refusal {
        code: "source_not_found",
        when: "--source does not name a readable PLS-CADD workspace or .bak",
        remedy: "check the path; `ds dsgrid-exchange inspect --source <path>` classifies it first",
    },
];

/// One availability answer for the whole family. Every verb needs the same
/// missing owner, so they refuse with the same code and the same remedy —
/// and there is one place to delete when the owner lands.
fn registration_availability() -> Availability {
    Availability::unavailable(
        UNSUPPORTED.code,
        "no owner reachable from ds registers a project DS Grid model",
        "prepare locally, register in DS GridDesign; see this command's reference",
    )
}

pub static CREATE_COMMAND: Command = Command {
    id: "dsgrid.model.create",
    path: &["dsgrid", "model", "create"],
    contract: 1,
    summary: "Register a new project DS Grid model from scratch.",
    purpose: "\
Creates a project DS Grid model head and its first empty revision, for a \
network that will be drawn rather than imported. `import` starts from a \
verified .dsgrid and `convert` from a PLS-CADD workspace. No reviewed owner \
is reachable from `ds` yet, so it refuses closed.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[NAME_ARG, KIND_ARG, REASON_ARG, DESCRIPTION_ARG, PROJECT_ARG],
    output: "\
The registered model id, its first revision id, the kind and the project. \
Nothing today: the command refuses closed.",
    examples: &[
        Example {
            command: "ds dsgrid model create --name \"Karongi MV\" --kind mv_line --reason \"New line design\" --yes",
            note: "Refuses with project_model_registration_unsupported until the owner lands.",
            runnable: false,
        },
        Example {
            command: "ds library global fork-example --payload '{...}' --yes",
            note: "The one project-model creation path that works today: from a governed catalogue example.",
            runnable: false,
        },
    ],
    refusals: CREATE_REFUSALS,
    reference: Some("docs/contracts/project-grid-model-contract.md"),
    availability: registration_availability,
};

pub static IMPORT_COMMAND: Command = Command {
    id: "dsgrid.model.import",
    path: &["dsgrid", "model", "import"],
    contract: 1,
    summary: "Register a verified local .dsgrid as a project model.",
    purpose: "\
Uploads one .dsgrid package the operator already has and registers it as a \
project model, or as a new revision of an existing one when --model-id and \
--expected-head are given. The package is expected to be verified first: \
`ds dsgrid validate` is the check, and it runs today with no project and no \
principal. Registration itself has no reviewed owner reachable from `ds` yet.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        MODEL_ARG,
        NAME_ARG,
        KIND_ARG,
        REASON_ARG,
        MODEL_ID_ARG,
        EXPECTED_HEAD_ARG,
        DESCRIPTION_ARG,
        PROJECT_ARG,
    ],
    output: "\
The model id, the new revision id, the package digest ds-brain stored, and \
the project. Nothing today: the command refuses closed.",
    examples: &[
        Example {
            command: "ds dsgrid validate --model ./karongi.dsgrid --output json",
            note: "Prove the package first; this part works today and needs no project.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid model import --model ./karongi.dsgrid --name \"Karongi MV\" --kind mv_line --reason \"As-built import\" --yes",
            note: "Refuses with project_model_registration_unsupported until the owner lands.",
            runnable: false,
        },
    ],
    refusals: IMPORT_REFUSALS,
    reference: Some("docs/contracts/project-grid-model-contract.md"),
    availability: registration_availability,
};

pub static CONVERT_COMMAND: Command = Command {
    id: "dsgrid.model.convert",
    path: &["dsgrid", "model", "convert"],
    contract: 1,
    summary: "Convert a PLS-CADD workspace and register it as a project model.",
    purpose: "\
Converts one PLS-CADD workspace or .bak to the canonical format and registers \
the result as a project model. The conversion half is owned by \
`ds dsgrid-exchange` and runs today, locally, with no project and no \
principal: inspect classifies the source, plan states what a conversion would \
do, convert writes the package. Registration has no reviewed owner reachable \
from `ds` yet, so this refuses closed rather than inventing one.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        NAME_ARG,
        KIND_ARG,
        REASON_ARG,
        DESCRIPTION_ARG,
        PROJECT_ARG,
    ],
    output: "\
The model id, the first revision id, the conversion's own summary, and the \
project. Nothing today: the command refuses closed.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange inspect --source ./Karongi --output json",
            note: "Classify the workspace first; this part works today and needs no project.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid model convert --source ./Karongi.bak --name \"Karongi MV\" --kind mv_line --reason \"PLS-CADD as-built\" --yes",
            note: "Refuses with project_model_registration_unsupported until the owner lands.",
            runnable: false,
        },
    ],
    refusals: CONVERT_REFUSALS,
    reference: Some("docs/contracts/project-grid-model-contract.md"),
    availability: registration_availability,
};

/// Every verb refuses the same way. Dispatch's availability gate already
/// does it; these exist so the family cannot become quietly reachable by a
/// change to the gate and answer with something invented.
fn refuse(next: &'static str) -> Failure {
    let Availability::Unavailable { reason, remedy, .. } = registration_availability() else {
        unreachable!("project model registration has no owner to reach");
    };
    Failure::unavailable("project_model_registration_unsupported", reason)
        .remedy(remedy)
        .next(next)
        .detail(json!({
            "missing_desktop_operations": [
                "grid.model.start_upload",
                "grid.model.create_version",
            ],
            "brain_actions": ["start_upload", "create_version"],
            "brain_endpoint": "POST /grid/models",
            "works_today": ["library.global.fork-example"],
            "contract": "docs/contracts/project-grid-model-contract.md",
        }))
}

pub fn run_create(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Err(refuse("ds library global fork-example --help"))
}

pub fn run_import(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Err(refuse("ds dsgrid validate --help"))
}

pub fn run_convert(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Err(refuse("ds dsgrid-exchange inspect --help"))
}

/// Shared by all three, and never reached while the family is closed.
pub fn render(data: &Value) -> String {
    format!(
        "model {}  revision {}  in {}\n",
        data["model_id"].as_str().unwrap_or("?"),
        data["revision_id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAMILY: &[&Command] = &[&CREATE_COMMAND, &IMPORT_COMMAND, &CONVERT_COMMAND];

    #[test]
    fn every_verb_refuses_closed_with_one_code_at_the_gate_and_the_handler() {
        // Fail-closed means one answer, whichever path reaches it. A gate
        // that says "unavailable" and a handler that invents a second
        // spelling is the ambiguity `error.code` exists to remove.
        let Availability::Unavailable { code, .. } = registration_availability() else {
            panic!("the family must stay closed until it has a reviewed owner");
        };
        assert_eq!(code, UNSUPPORTED.code);

        for handler in [run_create, run_import, run_convert] {
            let refused = handler(&Inputs::default(), &context()).expect_err("must refuse");
            assert_eq!(refused.code(), code);
            let detail = refused.detail_value().expect("detail names the gap");
            assert_eq!(detail["brain_endpoint"], "POST /grid/models");
            assert!(
                detail["missing_desktop_operations"]
                    .as_array()
                    .is_some_and(|operations| !operations.is_empty()),
                "a refusal that cannot be acted on must at least name what is missing"
            );
        }
    }

    #[test]
    fn the_family_declares_the_refusal_it_actually_emits() {
        for command in FAMILY {
            assert_eq!((command.availability)().token(), "unavailable");
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == UNSUPPORTED.code),
                "`{}` refuses with a code it does not document",
                command.id
            );
            assert_eq!(
                command.effect,
                Effect::GlobalWrite,
                "`{}` registers shared project state and must stay confirmation-gated",
                command.id
            );
            assert_eq!(command.authority, Authority::Project);
        }
    }

    #[test]
    fn the_kind_choice_set_is_exactly_what_ds_brain_accepts() {
        // Hand-copied from `projectGridModelKinds`. A fourth kind invented
        // here would be a value the owner rejects after a round trip; a
        // missing one would make a real model unreachable.
        assert_eq!(MODEL_KINDS, &["general", "lv_network", "mv_line"]);
        for command in FAMILY {
            assert_eq!(
                command
                    .arg("kind")
                    .expect("every verb names the kind")
                    .choices,
                MODEL_KINDS,
                "`{}` accepts a different kind set from its siblings",
                command.id
            );
        }
    }

    #[test]
    fn each_verb_names_exactly_one_source_so_the_family_cannot_be_ambiguous() {
        assert!(CREATE_COMMAND.arg("model").is_none() && CREATE_COMMAND.arg("source").is_none());
        assert!(IMPORT_COMMAND.arg("model").is_some() && IMPORT_COMMAND.arg("source").is_none());
        assert!(CONVERT_COMMAND.arg("source").is_some() && CONVERT_COMMAND.arg("model").is_none());
        // Concurrency is only meaningful against an existing head, and it is
        // required with one — a revision that silently replaced an unknown
        // head is the write this family must never make.
        assert!(IMPORT_COMMAND.arg("expected-head").is_some());
        assert!(CREATE_COMMAND.arg("expected-head").is_none());
    }

    fn context() -> Context {
        Context {
            confirmed: true,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Human, false, false),
        }
    }
}
