//! `ds assets attach` — link an asset to a task, a record or a DS object, or
//! unlink it.
//!
//! The link is recorded on the asset and nowhere else. Nothing is written onto
//! the task or the record, and nothing at all onto a DS object (§12.9):
//! Project Management's own contract requires links to point one way, so that
//! deleting an asset can never leave a half-written field on a transformer
//! somebody else owns.
//!
//! There are exactly three ways to name the other end — a task, a
//! correspondence record, or an object's type and id — and naming two, or
//! half of one, is refused here rather than resolved into a guess.
//!
//! Headless since 2026-09-20: `POST /api/v1/assets` `attach`/`detach` is one
//! governed write under the restored credential, for the selected project;
//! the window relayed it and nothing more.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::shared_assets::Command as Catalogue;
use ds_command_kernel::assets::Link;
use serde_json::{Value, json};

use crate::{ASSET_ARG, LANE_ARG};

const TASK_ARG: Arg = Arg::value(
    "task",
    "<task-id>",
    "The Project Management task, by the id `ds pm task list` reports.",
);

const RECORD_ARG: Arg = Arg::value(
    "record",
    "<record-id>",
    "The correspondence record, by the id `ds pm record list` reports; the asset becomes one of its attachments.",
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
    contract: 2,
    summary: "Link an asset to a task, a record or a DS object, or remove the link.",
    purpose: "\
Records the link on the asset through the catalogue's own attach action: \
exactly one of --task, --record, or --object-type with --entity-id. The link \
points from the asset to the work; nothing is ever written onto the task, \
the record or the DS object. An asset linked to a record is one of the \
record's attachments (`ds pm record read`) and is indexed under \
Assets › Correspondence with the record's thread. --detach removes the same \
link. Projected sys: rows are refused by name. Headless: writes to the \
selected project of the signed-in native credential, no window.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ASSET_ARG,
        TASK_ARG,
        RECORD_ARG,
        OBJECT_TYPE_ARG,
        ENTITY_ID_ARG,
        DETACH_ARG,
        LANE_ARG,
    ],
    output: "`asset` — the row with its `links` after the change — and the `project`.",
    examples: &[Example {
        command: "ds assets attach --asset a_7kq3nr2v0b1c --record R-0031 --yes",
        note: "The record's own `ds pm record read` then lists the asset among its attachments.",
        runnable: false,
    }],
    refusals: &crate::catalogue_refusals::<25>(&[
        crate::INVALID_ATTACHMENT,
        crate::PROJECTED_ASSET_READ_ONLY,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &["correspondence", "attachment", "link", "record", "letter"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The link, validated locally: exactly one end named.
fn command(inputs: &Inputs) -> Result<Catalogue, Failure> {
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
            .map(str::to_owned)
    };
    let link = match (
        named("task"),
        named("record"),
        named("object-type"),
        named("entity-id"),
    ) {
        (Some(id), None, None, None) => Link::PmTask { id },
        (None, Some(id), None, None) => Link::PmRecord { id },
        (None, None, Some(object_type), Some(entity_id)) => Link::DsObject {
            object_type,
            entity_id,
        },
        (None, None, None, None) => {
            return Err(attachment("name the work this asset belongs to"));
        }
        (None, None, Some(_), None) => return Err(attachment("--object-type needs --entity-id")),
        (None, None, None, Some(_)) => return Err(attachment("--entity-id needs --object-type")),
        _ => {
            return Err(attachment(
                "--task, --record and --object-type name different links; give one",
            ));
        }
    };
    Ok(Catalogue::Attach {
        asset_id: asset,
        link,
        detach: inputs.switch("detach"),
    })
}

fn attachment(why: &str) -> Failure {
    Failure::invalid("invalid_attachment", why.to_string())
        .remedy(crate::INVALID_ATTACHMENT.remedy)
        .next("ds assets attach --help")
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let command = command(inputs)?;
    crate::catalogue(inputs.value("lane").unwrap_or("stable"), &command)
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

    fn parse(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn refused(tokens: &[&str]) -> Failure {
        command(&parse(tokens)).expect_err("must refuse")
    }

    #[test]
    fn exactly_one_end_of_the_link_is_named_or_nothing_is_sent() {
        let asset = ["--asset", "a_7kq3nr2v0b1c"];
        let cases: &[&[&str]] = &[
            &[],
            &["--object-type", "transformer"],
            &["--entity-id", "TX-104"],
            &["--task", "t_4812", "--object-type", "transformer"],
            &["--task", "t_4812", "--record", "R-1"],
            &[
                "--record",
                "R-1",
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
    fn each_form_is_the_catalogue_link_it_names() {
        let task =
            command(&parse(&["--asset", "a_7kq3nr2v0b1c", "--task", " t_4812 "])).expect("valid");
        assert_eq!(
            task,
            Catalogue::Attach {
                asset_id: "a_7kq3nr2v0b1c".into(),
                link: Link::PmTask {
                    id: "t_4812".into()
                },
                detach: false
            }
        );
        let record = command(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--record",
            "R-0031",
            "--detach",
        ]))
        .expect("valid");
        assert_eq!(
            record,
            Catalogue::Attach {
                asset_id: "a_7kq3nr2v0b1c".into(),
                link: Link::PmRecord {
                    id: "R-0031".into()
                },
                detach: true
            }
        );
        let object = command(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--object-type",
            "transformer",
            "--entity-id",
            "TX-104",
        ]))
        .expect("valid");
        assert!(matches!(
            object,
            Catalogue::Attach {
                link: Link::DsObject { .. },
                detach: false,
                ..
            }
        ));
    }

    #[test]
    fn a_projected_row_is_refused_by_name_before_the_door() {
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
}
