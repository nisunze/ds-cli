//! `ds pm task geometry` — a task's one geometry, from the DS objects it is
//! about.
//!
//! The owner's case is a consultant's comment naming structures 74, 76 and
//! 77 in a swamp: the task that answers it has to say WHERE, on the map and
//! in the plan. Nobody draws it. The person or agent that read the comment
//! supplies the structure numbers as a typed reference —
//! `dsgrid:local-<id>:structure:74,76,77` — and the kernel resolves them to
//! the model's positions and shapes an area; `--dry-run` shows the proposal;
//! `--yes` writes it as the task's `geometry` with one `ds_object` link per
//! structure, in one revision.
//!
//! ```text
//!   create --geometry-from … | set --from …   →  read  →  clear
//! ```
//!
//! This module holds the pipeline both writes share: parse the references
//! (kernel), load the models they name (`ds-cli-dsgrid` reads the packages
//! and reprojects — the kernel never opens one), propose (kernel), and turn
//! a kernel refusal into the same-named CLI refusal. The write itself is the
//! ordinary `writes::create_task` / `writes::update_task` through the one
//! headless door every `ds pm` command uses. The contract is
//! `ds-command-kernel/docs/contracts/task-geometry-from-objects.md`.

pub mod clear;
pub mod read;
pub mod set;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use ds_command_kernel::local_models::Scope;
use ds_command_kernel::task_geometry::{
    self, IndexInput, ObjectIndex, Proposal, ProposeRequest, Source,
};
use serde_json::{Map, Value, json};

pub const FROM_ARG: Arg = Arg {
    name: "from",
    kind: ArgKind::Repeated,
    value: "<reference>",
    required: true,
    default: None,
    choices: &[],
    summary: "Typed DS objects: dsgrid:local-<id>:structure:74,76,77 or dsgrid:<src>:alignment:<aln>[:74..77]. Repeatable.",
};

pub const GEOMETRY_FROM_ARG: Arg = Arg {
    name: "geometry-from",
    kind: ArgKind::Repeated,
    value: "<reference>",
    required: false,
    default: None,
    choices: &[],
    summary: "Carry the geometry of these DS objects: dsgrid:local-<id>:structure:74,76,77 or dsgrid:<src>:alignment:<aln>[:74..77]. Repeatable.",
};

pub const PACKAGE_ARG: Arg = Arg {
    name: "package",
    kind: ArgKind::Value,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "The .dsgrid a `dsgrid:package:…` reference reads.",
};

pub const BUFFER_ARG: Arg = Arg {
    name: "buffer-m",
    kind: ArgKind::Value,
    value: "<metres>",
    required: false,
    default: Some("25"),
    choices: &[],
    summary: "Margin around several structures (1-500 m); they become an area.",
};

pub const DRY_RUN_ARG: Arg = Arg {
    name: "dry-run",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Answer the proposal — geometry, links, rule — and write nothing. Needs no --yes.",
};

pub const REFERENCE_INVALID: Refusal = Refusal {
    code: "reference_invalid",
    when: "a reference is not dsgrid:<local-<id>|package>:structure:… or :alignment:…, or a selector is empty or repeated",
    remedy: "write e.g. --from dsgrid:local-<id>:structure:74,76,77; ids come from `ds dsgrid model list`",
};
pub const MODEL_UNKNOWN: Refusal = Refusal {
    code: "model_unknown",
    when: "no working copy carries the referenced id for this lane and account, or `package` was referenced without --package",
    remedy: "pick an id from `ds dsgrid model list`, or pass --package <path>",
};
pub const OBJECT_UNRESOLVED: Refusal = Refusal {
    code: "object_unresolved",
    when: "a structure number or id, or an alignment, does not exist in the model (detail names each)",
    remedy: "check the numbers against `ds dsgrid run --model <path> --operation project_plan`",
};
pub const OBJECT_AMBIGUOUS: Refusal = Refusal {
    code: "object_ambiguous",
    when: "a structure number or alignment label names more than one object (detail lists the candidates)",
    remedy: "use the structure or alignment id, or name the alignment: dsgrid:…:alignment:<aln>:74..77",
};
pub const REFERENCES_INCOMPATIBLE: Refusal = Refusal {
    code: "references_incompatible",
    when: "structures and an alignment, or two alignments, were named together",
    remedy: "one proposal is either structures (point/area) or one alignment or range (line)",
};
pub const TOO_MANY_OBJECTS: Refusal = Refusal {
    code: "too_many_objects",
    when: "more than 100 objects were resolved",
    remedy: "name the structures the task is about; a whole line is an alignment reference",
};
pub const GEOMETRY_TOO_LARGE: Refusal = Refusal {
    code: "geometry_too_large",
    when: "the shaped geometry would carry more than 2,000 positions",
    remedy: "name a range (:74..77) instead of a whole alignment",
};
pub const BUFFER_OUT_OF_RANGE: Refusal = Refusal {
    code: "buffer_out_of_range",
    when: "--buffer-m is not a number from 1 through 500",
    remedy: "pass e.g. --buffer-m 25",
};
pub const LINKS_BOUND_EXCEEDED: Refusal = Refusal {
    code: "links_bound_exceeded",
    when: "the task would carry more than 128 links",
    remedy: "clear links the task no longer needs, or name fewer objects",
};

/// The refusals every proposal can raise — the kernel's, under the same
/// names, then the model reads behind them (`ds-cli-dsgrid`'s own).
pub const PROPOSAL_REFUSALS: [Refusal; 17] = [
    REFERENCE_INVALID,
    MODEL_UNKNOWN,
    OBJECT_UNRESOLVED,
    OBJECT_AMBIGUOUS,
    REFERENCES_INCOMPATIBLE,
    TOO_MANY_OBJECTS,
    GEOMETRY_TOO_LARGE,
    BUFFER_OUT_OF_RANGE,
    LINKS_BOUND_EXCEEDED,
    ds_cli_dsgrid::objects::REFUSALS[0],
    ds_cli_dsgrid::objects::REFUSALS[1],
    ds_cli_dsgrid::objects::REFUSALS[2],
    ds_cli_dsgrid::objects::REFUSALS[3],
    ds_cli_dsgrid::objects::REFUSALS[4],
    ds_cli_dsgrid::objects::REFUSALS[5],
    ds_cli_dsgrid::objects::REFUSALS[6],
    ds_cli_dsgrid::objects::REFUSALS[7],
];
const _: () = assert!(ds_cli_dsgrid::objects::REFUSALS.len() == 8);

/// `--buffer-m`, as a number inside the kernel's bounds. The range is the
/// kernel's (`MIN_BUFFER_M..=MAX_BUFFER_M`), checked here too so a caller
/// hears it before the round trip rather than after the graph was read.
pub fn buffer(inputs: &Inputs) -> Result<Option<f64>, Failure> {
    let Some(raw) = inputs.value("buffer-m") else {
        return Ok(None);
    };
    let refuse = || {
        Failure::invalid(
            BUFFER_OUT_OF_RANGE.code,
            format!(
                "`--buffer-m {raw}` is not a number of metres from {} through {}",
                task_geometry::MIN_BUFFER_M,
                task_geometry::MAX_BUFFER_M
            ),
        )
        .remedy(BUFFER_OUT_OF_RANGE.remedy)
        .detail(json!({ "given": raw, "min": task_geometry::MIN_BUFFER_M, "max": task_geometry::MAX_BUFFER_M }))
    };
    let value = raw.trim().parse::<f64>().map_err(|_| refuse())?;
    if !value.is_finite()
        || !(task_geometry::MIN_BUFFER_M..=task_geometry::MAX_BUFFER_M).contains(&value)
    {
        return Err(refuse());
    }
    Ok(Some(value))
}

/// The kernel's refusal under its own name — each code is one the commands
/// declare, so a caller who planned from `--help` has planned for it.
pub fn refuse(refusal: task_geometry::Refusal) -> Failure {
    let message = refusal.message;
    let failure = match refusal.code {
        "reference_invalid" => {
            Failure::invalid("reference_invalid", message).remedy(REFERENCE_INVALID.remedy)
        }
        "model_unknown" => Failure::invalid("model_unknown", message)
            .remedy(MODEL_UNKNOWN.remedy)
            .next("ds dsgrid model list --output json"),
        "object_unresolved" => {
            Failure::invalid("object_unresolved", message).remedy(OBJECT_UNRESOLVED.remedy)
        }
        "object_ambiguous" => {
            Failure::invalid("object_ambiguous", message).remedy(OBJECT_AMBIGUOUS.remedy)
        }
        "references_incompatible" => Failure::invalid("references_incompatible", message)
            .remedy(REFERENCES_INCOMPATIBLE.remedy),
        "too_many_objects" => {
            Failure::invalid("too_many_objects", message).remedy(TOO_MANY_OBJECTS.remedy)
        }
        "geometry_too_large" => {
            Failure::invalid("geometry_too_large", message).remedy(GEOMETRY_TOO_LARGE.remedy)
        }
        "buffer_out_of_range" => {
            Failure::invalid("buffer_out_of_range", message).remedy(BUFFER_OUT_OF_RANGE.remedy)
        }
        "links_bound_exceeded" => {
            Failure::invalid("links_bound_exceeded", message).remedy(LINKS_BOUND_EXCEEDED.remedy)
        }
        _ => Failure::internal("task_geometry_unmapped", message),
    };
    failure.detail(refusal.detail)
}

/// The local checks a proposal can fail before any round trip: the grammar
/// and the buffer. A caller who mistyped a reference hears which part was
/// wrong, not that no session was found — on a CI machine with no
/// credential as much as on a server with one.
pub fn check(inputs: &Inputs, references: &[String]) -> Result<(), Failure> {
    task_geometry::grammar::parse_all(references).map_err(refuse)?;
    buffer(inputs)?;
    Ok(())
}

/// Resolve and shape: the proposal a dry run answers and a write commits.
///
/// `existing_links` are the task's current links, so a link already there
/// is not proposed twice and the bound is checked before the round trip.
pub fn propose(
    inputs: &Inputs,
    references: &[String],
    read: &crate::Graph,
    existing_links: &[Value],
) -> Result<Proposal, Failure> {
    let parsed = task_geometry::grammar::parse_all(references).map_err(refuse)?;
    let mut sources: Vec<Source> = parsed.iter().map(|r| r.source.clone()).collect();
    sources.sort();
    sources.dedup();

    let scope = Scope {
        lane: read.lane.to_owned(),
        uid: read.uid.clone(),
    };
    let mut index: Vec<ObjectIndex> = Vec::with_capacity(sources.len());
    for source in &sources {
        match source {
            Source::Local { id } => {
                // A working copy the catalogue does not hold is left out of
                // the index; the kernel names that `model_unknown` with the
                // reference it came from.
                if let Some((path, display_name)) =
                    ds_cli_dsgrid::objects::local_package(&scope, id)?
                {
                    index.push(ds_cli_dsgrid::objects::index(
                        &source.key(),
                        &path.to_string_lossy(),
                        Some(display_name),
                    )?);
                }
            }
            Source::Package => {
                if let Some(path) = inputs.value("package") {
                    index.push(ds_cli_dsgrid::objects::index(&source.key(), path, None)?);
                }
            }
        }
    }

    let request = ProposeRequest {
        schema: task_geometry::SCHEMA.into(),
        action: "propose".into(),
        project: read.project_id.clone(),
        references: references.to_vec(),
        buffer_m: buffer(inputs)?,
        existing_links: existing_links.to_vec(),
        index: IndexInput { sources: index },
    };
    task_geometry::propose(&request).map_err(refuse)
}

/// The proposal as every receipt carries it.
pub fn proposal_json(proposal: &Proposal) -> Value {
    serde_json::to_value(proposal).unwrap_or(Value::Null)
}

/// A link's nine wire fields and nothing else: the command decoder refuses
/// an unknown field, and a link the graph answers may one day carry a
/// projection the engine did not author.
pub fn wire_link(link: &Value) -> Value {
    let mut out = Map::new();
    for key in [
        "kind",
        "project_id",
        "entity_id",
        "form_slug",
        "object_type",
        "object_revision",
        "label",
        "attached_by",
        "attached_at",
    ] {
        if let Some(value) = link.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), value.clone());
        }
    }
    Value::Object(out)
}

/// The task's `ds_object` links this family attached — the trace of a
/// geometry — as opposed to the links a person attached by hand.
pub fn is_geometry_link(link: &Value) -> bool {
    link["kind"] == "ds_object"
        && matches!(
            link["object_type"].as_str(),
            Some(task_geometry::OBJECT_TYPE_STRUCTURE) | Some(task_geometry::OBJECT_TYPE_ALIGNMENT)
        )
}

/// The task's links as the server published them, or none.
pub fn existing_links(task: &Value) -> Vec<Value> {
    task["links"].as_array().cloned().unwrap_or_default()
}

/// One proposal, rendered for a person: the rule, the objects, the links.
pub fn render_proposal(proposal: &Value) -> String {
    let rule = &proposal["rule"];
    let mut out = format!(
        "  geometry   {} · rule {}{} · {} positions · {} objects\n",
        proposal["geometry"]["type"].as_str().unwrap_or("?"),
        rule["kind"].as_str().unwrap_or("?"),
        rule["buffer_m"]
            .as_f64()
            .map(|b| format!(" ({b} m)"))
            .unwrap_or_default(),
        rule["positions"],
        rule["objects"],
    );
    for object in proposal["objects"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<18} {:<20} {} · {} rev {}{}\n",
            object["kind"].as_str().unwrap_or("?"),
            crate::truncate(object["label"].as_str().unwrap_or("?"), 20),
            object["id"].as_str().unwrap_or("?"),
            object["source"].as_str().unwrap_or("?"),
            object["model_revision"],
            object["position"]
                .as_array()
                .map(|p| format!(" · {} {}", p[0], p[1]))
                .unwrap_or_default(),
        ));
    }
    let links = proposal["links"].as_array().map_or(0, Vec::len);
    out.push_str(&format!(
        "  links      {links} new ds_object link{}\n",
        if links == 1 { "" } else { "s" }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kernel_refusal_has_a_same_named_cli_refusal_with_a_remedy() {
        // The kernel's vocabulary is closed; each code must be one a caller
        // can plan for from `--help`, and the mapping must not fall into the
        // internal arm for any of them.
        for code in task_geometry::REFUSALS {
            let failure = refuse(task_geometry::Refusal::new(
                code,
                "why",
                json!({ "part": "x" }),
            ));
            assert_eq!(failure.code(), *code);
            assert!(failure.remedy_text().is_some(), "{code} has no remedy");
            assert_eq!(failure.detail_value().unwrap()["part"], "x");
            assert!(
                PROPOSAL_REFUSALS
                    .iter()
                    .any(|declared| declared.code == *code),
                "{code} is not declared"
            );
        }
        assert_eq!(
            refuse(task_geometry::Refusal::new(
                "model_crs_unsupported",
                "x",
                json!({})
            ))
            .code(),
            "task_geometry_unmapped"
        );
    }

    #[test]
    fn a_geometry_link_is_told_from_a_hand_attached_one_and_re_sent_with_its_wire_fields() {
        assert!(is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "dsgrid_structure" })
        ));
        assert!(is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "dsgrid_alignment" })
        ));
        assert!(!is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "transformer" })
        ));
        assert!(!is_geometry_link(
            &json!({ "kind": "survey_entry", "object_type": "dsgrid_structure" })
        ));
        // The decoder refuses an unknown field, so a projection the graph
        // added would sink the whole command.
        assert_eq!(
            wire_link(
                &json!({ "kind": "pm_task", "project_id": "p", "entity_id": "t-0", "extra": "projection", "label": null })
            ),
            json!({ "kind": "pm_task", "project_id": "p", "entity_id": "t-0" })
        );
    }
}
