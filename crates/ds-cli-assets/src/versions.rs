//! `ds assets versions` — one asset's version history, newest first.
//!
//! A transformer design, a design attachment label and a grid model each
//! keep versions; the index folds them into one row whose `versions` chip
//! says how many (`ds assets list`). This command unfolds one: the version
//! rows ds-brain's index answers for `get` (assets-index §5.1), exactly as
//! served. The Assets page shows this same list.
//!
//! The history exists only in the index. A lane whose ds-brain predates it
//! has none to give, so this refuses by name rather than answering an empty
//! list that would read as "never revised".

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{ASSET_ARG, CatalogueCommand, LANE_ARG, PROJECT_ARG};

/// The most version rows one human projection prints before it says how
/// many it left out. The JSON answer is whole.
const MAX_LINES: usize = 200;

pub static COMMAND: Command = Command {
    id: "assets.versions",
    path: &["assets", "versions"],
    contract: 1,
    summary: "List one asset's versions, newest first.",
    purpose: "\
The version history of one asset in the named project: each saved version \
of a transformer design, a design attachment label or a grid model, newest \
first, with its label, when and by whom it was made, its milestone and \
reason, and which one is current. Read from the index ds-brain builds and \
serves; the Assets page shows the same rows. Take the asset_id from `ds \
assets list`, whose versions chip says how many there are; each version's \
own asset_id feeds preview or read. An asset without versions answers an \
empty list. Changes nothing. Headless.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ASSET_ARG, LANE_ARG, PROJECT_ARG],
    output: "\
`asset` (the index row, with `versions` {count, current, latest_at}); \
`versions` rows of `asset_id`, `label`, `created_at`, `created_by`, `reason`, \
`milestone` and `current`, newest first; ds-brain's `index` meta when served; \
`index_status` `served`.",
    examples: &[Example {
        command: "ds assets versions --project <exact-id> --asset sys:transformers:TX-104 --output json",
        note: "Read .data.versions[].asset_id to preview or read one version.",
        runnable: false,
    }],
    refusals: &crate::refusals::<25>(&[
        crate::INVALID_ASSET_ID,
        crate::ASSETS_INDEX_UNAVAILABLE,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[
        "history",
        "revisions",
        "version history",
        "document history",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let asset = crate::asset_id(inputs.require("asset")?, "asset")?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let answer = crate::catalogue(
        lane,
        project,
        &CatalogueCommand::IndexGet {
            asset_id: asset.clone(),
        },
    )?;
    if !crate::index_served(&answer) {
        return Err(Failure::unavailable(
            crate::ASSETS_INDEX_UNAVAILABLE.code,
            format!("this lane's ds-brain does not serve the assets index, which holds the versions of `{asset}`"),
        )
        .remedy(crate::ASSETS_INDEX_UNAVAILABLE.remedy)
        .detail(json!({ "asset": asset, "index_status": answer["index_status"] }))
        .next(format!("ds assets list --project {project} --output json")));
    }
    let mut data = json!({
        "asset": answer["asset"],
        "versions": answer["versions"],
        "index_status": crate::INDEX_SERVED,
    });
    if !answer["index"].is_null() {
        data["index"] = answer["index"].clone();
    }
    Ok(data)
}

pub fn render(data: &Value) -> String {
    let asset = &data["asset"];
    let rows = data["versions"].as_array();
    let count = rows.map_or(0, Vec::len);
    let mut out = format!(
        "{} · {} · {}\n",
        crate::truncate(asset["name"].as_str().unwrap_or("?"), 60),
        asset["asset_id"].as_str().unwrap_or("?"),
        crate::plural(count as u64, "version"),
    );
    let shown = count.min(MAX_LINES);
    for row in rows.into_iter().flatten().take(shown) {
        let mut note: Vec<&str> = Vec::new();
        if let Some(milestone) = row["milestone"].as_str().filter(|text| !text.is_empty()) {
            note.push(milestone);
        }
        if let Some(reason) = row["reason"].as_str().filter(|text| !text.is_empty()) {
            note.push(reason);
        }
        out.push_str(&format!(
            "  {:<10} {:<8} {:<17} {:<28} {}\n",
            crate::truncate(row["label"].as_str().unwrap_or("?"), 10),
            if row["current"] == Value::Bool(true) {
                "current"
            } else {
                ""
            },
            when(row["created_at"].as_str()),
            crate::truncate(row["created_by"].as_str().unwrap_or("—"), 28),
            crate::truncate(&note.join(" · "), 60),
        ));
        out.push_str(&format!(
            "  {:<10} {}\n",
            "",
            row["asset_id"].as_str().unwrap_or("?")
        ));
    }
    if count > shown {
        out.push_str(&format!("  … {} more\n", count - shown));
    }
    out
}

/// `2026-09-25 09:00Z` from an RFC 3339 instant: the minute a version was
/// made, the same whenever it is printed.
fn when(stamp: Option<&str>) -> String {
    match stamp {
        Some(stamp) if crate::epoch_seconds(stamp).is_some() && stamp.len() >= 16 => {
            format!("{} {}Z", &stamp[..10], &stamp[11..16])
        }
        _ => "—".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::args::parse;

    use super::*;

    #[test]
    fn the_asset_is_required_and_checked_before_any_round_trip() {
        let parsed = |tokens: &[&str]| {
            let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
            parse(&COMMAND, &tokens)
        };
        assert!(parsed(&["--project", "p"]).is_err(), "--asset is required");
        let inputs = parsed(&["--asset", "sys:", "--project", "p"]).expect("parses");
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::output::Output::resolve(
                ds_cli_contract::output::Format::Json,
                false,
                true,
            ),
        };
        assert_eq!(
            run(&inputs, &context).expect_err("refused").code(),
            "invalid_asset_id"
        );
    }

    #[test]
    fn the_history_prints_newest_first_as_served() {
        let out = render(&json!({
            "asset": {"asset_id": "sys:transformers:TX-104", "name": "TX-104 LV design",
                      "versions": {"count": 2, "current": "v3", "latest_at": "2026-09-25T09:00:00Z"}},
            "versions": [
                {"asset_id": "sys:transformer_versions:TX-104:v3", "label": "v3",
                 "created_at": "2026-09-25T09:00:00Z", "created_by": "lead@ds.rw",
                 "milestone": "IFC", "reason": "client comments", "current": true},
                {"asset_id": "sys:transformer_versions:TX-104:v2", "label": "v2",
                 "created_at": "2026-09-20T08:30:00.250Z", "created_by": "lead@ds.rw",
                 "current": false},
            ],
            "index_status": "served",
        }));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0],
            "TX-104 LV design · sys:transformers:TX-104 · 2 versions"
        );
        assert!(lines[1].trim_start().starts_with("v3"), "{out}");
        assert!(lines[1].contains("current"), "{out}");
        assert!(lines[1].contains("2026-09-25 09:00Z"), "{out}");
        assert!(lines[1].contains("IFC · client comments"), "{out}");
        assert!(
            lines[2].ends_with("sys:transformer_versions:TX-104:v3"),
            "{out}"
        );
        assert!(lines[3].trim_start().starts_with("v2"), "{out}");
        assert!(!lines[3].contains("current"), "{out}");
        assert!(lines[3].contains("2026-09-20 08:30Z"), "{out}");

        let none = render(&json!({
            "asset": {"asset_id": "a_000000000001", "name": "EPC.pdf"},
            "versions": [],
            "index_status": "served",
        }));
        assert_eq!(none, "EPC.pdf · a_000000000001 · 0 versions\n");
    }
}
