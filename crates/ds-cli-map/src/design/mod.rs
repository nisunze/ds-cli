//! `ds map design` — the project-backed design layers of one transformer.
//!
//! These are not local layers. A transformer's design layers are project
//! data: the LV lines, poles, spans and service cables an operator edits in
//! the design tools, loaded into a per-transformer *room* the application
//! keeps on this machine.
//!
//! The family is deliberately five reads and stages plus one push:
//!
//! ```text
//!   read → select → set | create | process → save
//! ```
//!
//! Feature editing and processing write only into the local room and mark it
//! dirty. `save` is the separate, explicit push to the project. A deliberate
//! `version begin` is another confirmed effect, owned by ds-brain and never
//! implied by save or report.
//!
//! That split is the application's, and the reason is worth restating: the
//! most consequential property write in this product is marking an as-built
//! network approved, because `drafting_status=approved` is what stops the
//! kernel from redesigning an installed transformer. One sentence from a
//! model must not be able to reach the project. So it cannot: staging is
//! reversible, and committing is a second command a human confirms.
//!
//! Every result therefore reports `staged` and `persisted` separately, and
//! `persisted` is false for staging commands; save and version begin report
//! their own distinct durable results.

pub mod batch_process;
pub mod batch_report;
pub mod batch_save;
pub mod create;
pub mod delete;
pub mod discard;
pub mod geometry;
pub mod layer_to_local;
pub mod list;
pub mod process;
pub mod process_setup;
pub mod read;
pub mod report;
pub mod save;
pub mod select;
pub mod set;
pub mod upload;
pub mod upload_stage;
pub mod upload_to_local;
pub mod version_create;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use serde_json::{Map, Value, json};

/// The transformer flag, declared identically everywhere in the family.
pub const TRANSFORMER_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Value,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "The transformer whose design layers to work on.",
};

pub const LAYER_ARG: Arg = Arg {
    name: "layer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Design layer to search, e.g. lv_lines. Repeat; omit for all.",
};

pub const WHERE_ARG: Arg = Arg {
    name: "where",
    kind: ArgKind::Repeated,
    value: "<key=value>",
    required: false,
    default: None,
    choices: &[],
    summary: "Property must equal this. Repeat to AND. Empty value means unset.",
};

pub const BBOX_ARG: Arg = Arg {
    name: "bbox",
    kind: ArgKind::Value,
    value: "<w,s,e,n>",
    required: false,
    default: None,
    choices: &[],
    summary: "Only features whose extent meets this box, in degrees.",
};

pub const ID_ARG: Arg = Arg {
    name: "id",
    kind: ArgKind::Repeated,
    value: "<feature-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Narrow to exactly these feature ids. Repeat.",
};

/// The application's bound on one selector's explicit id list.
pub const MAX_SELECTOR_IDS: usize = 5_000;

pub const DESIGN_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such transformer in the project, or the application declined",
    remedy: "check --transformer against the project; read detail.detail for its message",
};

pub const TOO_MANY_IDS: Refusal = Refusal {
    code: "too_many_ids",
    when: "more --id values were given than one selector accepts",
    remedy: "select by --where or --bbox instead; the refusal names the bound",
};

/// Build the selector keys shared by `select`, `set` and the differential
/// half of `process`.
///
/// Absent parts are omitted rather than sent empty. That is not tidiness: an
/// empty `where` and an absent one mean the same thing to the application,
/// but an empty `ids` array would mean "these zero features", and sending one
/// by accident is how a selector silently stops matching anything.
pub fn selector(inputs: &Inputs, prefix: &str) -> Result<Map<String, Value>, Failure> {
    let name = |base: &str| {
        if prefix.is_empty() {
            base.to_string()
        } else {
            format!("{prefix}-{base}")
        }
    };

    let mut selector = Map::new();

    let layers = inputs.repeated(&name("layer"));
    if !layers.is_empty() {
        selector.insert("layers".into(), json!(layers));
    }

    let predicates = crate::pairs(inputs.repeated(&name("where")), &name("where"))?;
    if !predicates.is_empty() {
        selector.insert("where".into(), Value::Object(predicates));
    }

    if let Some(raw) = inputs.value(&name("bbox")) {
        selector.insert("bbox".into(), json!(crate::bbox(raw)?));
    }

    let ids = inputs.repeated(&name("id"));
    if !ids.is_empty() {
        if ids.len() > MAX_SELECTOR_IDS {
            return Err(Failure::invalid(
                "too_many_ids",
                format!("{} ids is more than one selector accepts", ids.len()),
            )
            .remedy(TOO_MANY_IDS.remedy)
            .detail(json!({ "given": ids.len(), "max": MAX_SELECTOR_IDS })));
        }
        selector.insert("ids".into(), json!(ids));
    }

    Ok(selector)
}

/// Merge selector keys into an argument object, so every command sends one
/// spelling of a selector.
pub fn with_selector(mut arguments: Map<String, Value>, selector: Map<String, Value>) -> Value {
    for (key, value) in selector {
        arguments.insert(key, value);
    }
    Value::Object(arguments)
}

/// One line naming what a selector actually narrowed to, for human output.
pub fn describe(selector: &Map<String, Value>) -> String {
    if selector.is_empty() {
        return "every feature in the room".into();
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(Value::Array(layers)) = selector.get("layers") {
        parts.push(format!("{} layer(s)", layers.len()));
    }
    if let Some(Value::Object(predicates)) = selector.get("where") {
        for (key, value) in predicates {
            match value {
                Value::Null => parts.push(format!("{key} unset")),
                other => parts.push(format!("{key}={}", other.as_str().unwrap_or_default())),
            }
        }
    }
    if selector.contains_key("bbox") {
        parts.push("inside a box".into());
    }
    if let Some(Value::Array(ids)) = selector.get("ids") {
        parts.push(format!("{} id(s)", ids.len()));
    }
    parts.join(", ")
}

/// Geometry addressing is exactly-one by contract; the application's own
/// refusal message is folded into the named `ambiguous_feature` condition so
/// a caller gets a remedy instead of a raw sentence.
pub fn classify_geometry_failure(
    failure: ds_cli_contract::outcome::Failure,
) -> ds_cli_contract::outcome::Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !detail.contains("must address exactly one") {
        return failure;
    }
    ds_cli_contract::outcome::Failure::invalid(
        "ambiguous_feature",
        "the id matched zero features, or more than one",
    )
    .remedy("read ids from `ds map design select`; a geometry write addresses exactly one")
}

/// The closing line that keeps staging and saving distinguishable.
pub fn staging_note(data: &Value) -> &'static str {
    if data["persisted"].as_bool().unwrap_or(false) {
        "saved to the project\n"
    } else if data["staged"].as_bool().unwrap_or(false) {
        "staged locally; nothing reached the project\n  → ds map design save --transformer <name> --yes\n"
    } else {
        "nothing was changed\n"
    }
}
