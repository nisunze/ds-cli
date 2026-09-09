//! `ds assets attach` — link an asset to a task or a DS object, or unlink it.
//!
//! The link is recorded on the asset and nowhere else. Nothing is written onto
//! the task, and nothing at all onto a DS object (§12.9): Project Work's own
//! contract requires links to point one way, so that deleting an asset can
//! never leave a half-written field on a transformer somebody else owns.
//!
//! There are exactly two ways to name the other end — a task, or an object's
//! type and id — and naming both, or half of one, is refused here rather than
//! resolved into a guess.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, DESCRIPTOR_ARG};

const TASK_ARG: Arg = Arg::value(
    "task",
    "<task-id>",
    "The Project Work task, by the id `ds work task list` reports.",
);

const OBJECT_TYPE_ARG: Arg = Arg::value(
    "object-type",
    "<type>",
    "The DS object's type, e.g. transformer; needs --entity-id.",
);

const ENTITY_ID_ARG: Arg = Arg::value(
    "entity-id",
    "<id>",
    "The DS object's id, e.g. TX-104; needs --object-type.",
);

const DETACH_ARG: Arg = Arg::switch("detach", "Remove the link instead of adding it.");

pub static COMMAND: Command = Command {
    id: "assets.attach",
    path: &["assets", "attach"],
    contract: 1,
    summary: "Link an asset to a task or a DS object, or remove the link.",
    purpose: "\
Records the link on the asset through Project Work's own attachment path: \
exactly one of --task, or --object-type with --entity-id. The link points from \
the asset to the work; nothing is ever written onto the task or the DS object. \
--detach removes the same link. Projected sys: rows are refused by name in this \
slice, and so is an offline device: the link is a ds-brain write.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        ASSET_ARG,
        TASK_ARG,
        OBJECT_TYPE_ARG,
        ENTITY_ID_ARG,
        DETACH_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "`asset` — the row with its `links` after the change.",
    examples: &[Example {
        command: "ds assets attach --asset a_7kq3nr2v0b1c --task t_4812 --yes",
        note: "The task's own `ds work task read` then lists the asset among its attachments.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::ASSETS_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_ASSET_ID,
        crate::INVALID_ATTACHMENT,
        crate::PROJECTED_ASSET_READ_ONLY,
        crate::CONFIRMATION_REQUIRED,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_VERSION_CONFLICT,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::ASSETS_OFFLINE_WRITE,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The link, validated locally, in the exact keys the operation declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let asset = crate::asset_id(inputs.require("asset")?, "asset")?;
    if crate::is_projected(&asset) {
        return Err(Failure::invalid(
            "projected_asset_read_only",
            format!("`{asset}` is a projected system row, which carries no links of its own"),
        )
        .remedy(crate::PROJECTED_ASSET_READ_ONLY.remedy)
        .detail(json!({ "asset": asset })));
    }

    let named = |flag: &str| {
        inputs
            .value(flag)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };

    let mut arguments = Map::new();
    arguments.insert("asset".into(), json!(asset));
    match (named("task"), named("object-type"), named("entity-id")) {
        (Some(task), None, None) => {
            arguments.insert("task".into(), json!(task));
        }
        (None, Some(object_type), Some(entity_id)) => {
            arguments.insert("object_type".into(), json!(object_type));
            arguments.insert("entity_id".into(), json!(entity_id));
        }
        (None, None, None) => {
            return Err(attachment("name the work this asset belongs to"));
        }
        (Some(_), _, _) => {
            return Err(attachment(
                "--task and --object-type name two different links",
            ));
        }
        (None, Some(_), None) => return Err(attachment("--object-type needs --entity-id")),
        (None, None, Some(_)) => return Err(attachment("--entity-id needs --object-type")),
    }

    // Absent is the ordinary case, so `detach` travels only when it was
    // asked for; there is no third state for the application to interpret.
    if inputs.switch("detach") {
        arguments.insert("detach".into(), json!(true));
    }

    Ok(Value::Object(arguments))
}

fn attachment(why: &str) -> Failure {
    Failure::invalid("invalid_attachment", why.to_string())
        .remedy(crate::INVALID_ATTACHMENT.remedy)
        .next("ds assets attach --help")
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_ATTACH,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    let row = &data["asset"];
    let links: Vec<&Value> = row["links"].as_array().into_iter().flatten().collect();
    let mut out = format!(
        "{} now has {}\n",
        row["asset_id"].as_str().unwrap_or("?"),
        crate::plural(links.len() as u64, "link"),
    );
    if row.is_object() {
        out.push_str(&crate::asset_line(row));
    }
    for link in links.iter().take(crate::MAX_LINKS) {
        let label = link
            .as_str()
            .map_or_else(|| link.to_string(), str::to_string);
        out.push_str(&format!("  → {}\n", crate::truncate(&label, 76)));
    }
    if links.len() > crate::MAX_LINKS {
        out.push_str(&format!("  … {} more\n", links.len() - crate::MAX_LINKS));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::spec::ArgKind;
    use ds_cli_desktop::ops::undeclared_key;

    fn parse(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn refused(tokens: &[&str]) -> Failure {
        arguments(&parse(tokens)).expect_err("must refuse")
    }

    #[test]
    fn exactly_one_end_of_the_link_is_named_or_nothing_is_sent() {
        let asset = ["--asset", "a_7kq3nr2v0b1c"];
        let cases: &[&[&str]] = &[
            &[],
            &["--object-type", "transformer"],
            &["--entity-id", "TX-104"],
            &["--task", "t_4812", "--object-type", "transformer"],
            &[
                "--task",
                "t_4812",
                "--object-type",
                "transformer",
                "--entity-id",
                "TX-104",
            ],
        ];
        for case in cases {
            let mut tokens: Vec<&str> = asset.to_vec();
            tokens.extend_from_slice(case);
            assert_eq!(
                refused(&tokens).code(),
                "invalid_attachment",
                "{case:?} was accepted as one link"
            );
        }
    }

    #[test]
    fn each_form_travels_under_the_keys_the_operation_declares() {
        let task =
            arguments(&parse(&["--asset", "a_7kq3nr2v0b1c", "--task", " t_4812 "])).expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_ATTACH, &task), None);
        assert_eq!(task, json!({ "asset": "a_7kq3nr2v0b1c", "task": "t_4812" }));

        let object = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--object-type",
            "transformer",
            "--entity-id",
            "TX-104",
            "--detach",
        ]))
        .expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_ATTACH, &object), None);
        assert_eq!(
            object,
            json!({
                "asset": "a_7kq3nr2v0b1c",
                "object_type": "transformer",
                "entity_id": "TX-104",
                "detach": true
            })
        );
        // Attaching is the ordinary case, and says nothing about detaching.
        assert!(task.get("detach").is_none());
    }

    #[test]
    fn a_projected_row_is_refused_by_name_before_the_bridge() {
        let failure = refused(&["--asset", "sys:pm_attachment:9", "--task", "t_4812"]);
        assert_eq!(failure.code(), "projected_asset_read_only");
        assert_eq!(
            refused(&["--asset", "t_4812", "--task", "t_4812"]).code(),
            "invalid_asset_id"
        );
    }

    #[test]
    fn the_human_projection_bounds_the_links_it_prints() {
        let many: Vec<Value> = (0..crate::MAX_LINKS + 3)
            .map(|index| json!(format!("pm_task:t_{index}")))
            .collect();
        let rendered = render(&json!({
            "asset": { "asset_id": "a_7kq3nr2v0b1c", "name": "EPC Lot 3.pdf", "folder": "contracts",
                       "kind": "doc", "status": "durable", "sensitivity": "internal", "bytes": 12,
                       "links": many }
        }));
        assert!(rendered.contains("a_7kq3nr2v0b1c now has 35 links"));
        assert!(rendered.contains("→ pm_task:t_0"));
        assert!(rendered.contains("… 3 more"));
        assert!(!rendered.contains("pm_task:t_34"));
    }

    #[test]
    fn this_write_cannot_be_reached_without_explicit_confirmation() {
        // The gate itself lives once, in `ds`'s dispatch, and reads exactly
        // this declaration — so the declaration is the part a test inside
        // this crate can hold. A confirmation-gated command may also take no
        // positional argument, because `--yes` must never be the thing that
        // shifts what an operand means.
        assert!(COMMAND.effect.needs_confirmation());
        assert!(COMMAND.confirmation_required_for(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--task",
            "t_4812"
        ])));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.kind != ArgKind::Positional)
        );
    }
}
