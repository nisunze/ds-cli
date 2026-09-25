//! `ds assets list` — the project's assets index as bounded pages.
//!
//! The entry point when nothing else is known: `read`, `preview`, `classify`,
//! `promote`, `attach` and `versions` all need an asset id, and this and
//! `tree` are where one comes from. `--order recent`, the default, is the
//! project's timeline: whatever changed last, across uploads and every source
//! the index projects, first.
//!
//! The index is ds-brain's (assets-index §5.1). On a lane whose ds-brain
//! predates it, the native client falls back once to the catalogue and marks
//! the answer `index_status: "unavailable"`; this command then answers as it
//! did before the index — catalogued uploads, newest received first — and
//! says so.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{
    CURSOR_ARG, CatalogueCommand, IndexList, IndexOrder, LANE_ARG, LIMIT_ARG, PROJECT_ARG,
};

const FOLDER_ARG: Arg = Arg::value(
    "folder",
    "<path>",
    "Only this exact index folder, e.g. Transformers/TX-104 or contracts/2026; not its subfolders.",
);

const ORDER_ARG: Arg = Arg::value(
    "order",
    "<order>",
    "recent: newest change first, the project's timeline; name: by name.",
)
.choices(&["recent", "name"])
.default("recent");

const REFRESH_ARG: Arg = Arg::switch(
    "refresh",
    "Rebuild the project's index first, even when no source says it changed.",
);

const KIND_ARG: Arg =
    Arg::value("kind", "<kind>", "Only this kind of asset.").choices(crate::KINDS);

const STATUS_ARG: Arg = Arg::value(
    "status",
    "<status>",
    "Only this status; absent means everything but archive.",
)
.choices(crate::STATUSES);

const SENSITIVITY_ARG: Arg = Arg::value(
    "sensitivity",
    "<class>",
    "Only this class. A class you cannot read answers forbidden, or empty — never a hint.",
)
.choices(crate::SENSITIVITIES);

const SINCE_ARG: Arg = Arg::value(
    "since",
    "<date>",
    "Only assets dated on or after this date (YYYY-MM-DD or RFC 3339).",
);

/// The most rows one human projection prints before it says how many it did
/// not print. A page is bounded at [`crate::MAX_PAGE_SIZE`] by the owner, so
/// this only ever bites on a malformed answer — and then it is reported, not
/// swallowed. The JSON answer is whole.
const MAX_LINES: usize = crate::MAX_PAGE_SIZE as usize;

pub static COMMAND: Command = Command {
    id: "assets.list",
    path: &["assets", "list"],
    contract: 2,
    summary: "List the project's assets, newest change first, one page at a time.",
    purpose: "\
The project's timeline: every asset the signed-in user may see in the named \
project — uploads, transformer designs, MV models, survey media, reports, \
Solar, prints — newest change first, each with its folder, kind and version \
count. --order name sorts by name; --folder narrows to one exact index folder. \
Read from the index ds-brain builds and serves; --refresh rebuilds it first. \
Every other `ds assets` command takes an asset_id from here or `ds assets \
tree`. A class the caller cannot read has no row and no count. A lane whose \
ds-brain predates the index answers the catalogue of uploads instead, marked \
`index_status: unavailable`. Changes nothing. Headless.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ORDER_ARG,
        FOLDER_ARG,
        KIND_ARG,
        STATUS_ARG,
        SENSITIVITY_ARG,
        SINCE_ARG,
        LIMIT_ARG,
        CURSOR_ARG,
        REFRESH_ARG,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "\
`assets` rows (`asset_id`, `name`, `kind`, `folder_path`, `modified_at`, \
`versions` {count, current, latest_at} …), `more` and `next_cursor`, `order`, \
ds-brain's `index` {generation, built_at, checked_at, rebuilt, stale_sources, \
truncated_sources} and `index_status` `served`. When `unavailable`: catalogue \
rows with `more`, `next_cursor`, `scanned` and `truncated`.",
    examples: &[Example {
        command: "ds assets list --project <exact-id> --folder Transformers/TX-104 --output json",
        note: "Read .data.assets[].asset_id to feed versions, read, preview or attach.",
        runnable: false,
    }],
    refusals: &crate::refusals::<28>(&[
        crate::INVALID_NUMBER,
        crate::INVALID_FOLDER_PATH,
        crate::INVALID_DATE,
        crate::UNKNOWN_FOLDER,
        crate::ASSETS_INDEX_MOVED,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &["timeline", "recent", "latest changes"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The page request, validated locally, in the exact keys the operation
/// declares. Only flags that were given travel.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    // The order is closed at the parser and defaulted there: `recent`.
    if let Some(order) = inputs.value("order") {
        arguments.insert("order".into(), json!(order));
    }
    if let Some(folder) = inputs.value("folder") {
        arguments.insert(
            "folder".into(),
            json!(crate::folder_path(folder, "folder")?),
        );
    }
    // The vocabularies are closed at the parser, so a value that reaches here
    // is one of the contract's words; an absent flag stays absent on the wire
    // because the owner's own default (everything but archive) is the answer
    // to "no status was asked for".
    for flag in ["kind", "status", "sensitivity"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    if let Some(since) = inputs.value("since") {
        arguments.insert("since".into(), json!(crate::since(since, "since")?));
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?),
        );
    }
    if let Some(cursor) = inputs
        .value("cursor")
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
    {
        arguments.insert("cursor".into(), json!(cursor));
    }
    if inputs.switch("refresh") {
        arguments.insert("refresh".into(), json!(true));
    }
    Ok(Value::Object(arguments))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let text = |key: &str| arguments[key].as_str().map(str::to_owned);
    let limit = arguments["limit"]
        .as_u64()
        .map_or(crate::DEFAULT_PAGE_SIZE as u16, |limit| limit as u16);
    let order = text("order")
        .as_deref()
        .and_then(IndexOrder::parse)
        .unwrap_or_default();
    let page = crate::catalogue(
        lane,
        project,
        &CatalogueCommand::IndexList(IndexList {
            order,
            folder: text("folder"),
            kind: text("kind"),
            status: text("status"),
            sensitivity: text("sensitivity"),
            since: text("since"),
            limit,
            cursor: text("cursor"),
            refresh: arguments["refresh"] == Value::Bool(true),
        }),
    )?;
    if crate::index_served(&page) {
        let mut answer = json!({
            "assets": page["assets"],
            "more": page["more"] == Value::Bool(true),
            "order": order.as_str(),
            "index": page["index"],
            "index_status": crate::INDEX_SERVED,
        });
        if let Some(cursor) = page["next_cursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty())
        {
            answer["next_cursor"] = json!(cursor);
        }
        return Ok(answer);
    }

    // This lane's ds-brain predates the index. The native client already
    // answered the catalogue page when the request had a catalogue form; a
    // folder path has none, so it is resolved the way it always was: a
    // declared folder, by the one folder authority.
    let page = if page["assets"].is_array() {
        page
    } else {
        let folder_id = match text("folder") {
            Some(path) => Some(
                crate::folder_at(lane, project, &path)?["folder_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ),
            None => None,
        };
        crate::catalogue(
            lane,
            project,
            &CatalogueCommand::List {
                folder_id,
                kind: text("kind"),
                status: text("status"),
                sensitivity: text("sensitivity"),
                since: text("since"),
                limit,
                cursor: text("cursor"),
            },
        )?
    };
    let mut answer = json!({
        "assets": page["assets"],
        "more": page["has_more"] == Value::Bool(true),
        "scanned": page["scanned"],
        "truncated": page["truncated"] == Value::Bool(true),
        "index_status": crate::INDEX_UNAVAILABLE,
    });
    if let Some(cursor) = page["next_cursor"]
        .as_str()
        .filter(|cursor| !cursor.is_empty())
    {
        answer["next_cursor"] = json!(cursor);
    }
    Ok(answer)
}

pub fn render(data: &Value) -> String {
    let rows = data["assets"].as_array();
    let on_page = rows.map_or(0, Vec::len);
    let served = crate::index_served(data);
    let mut out = format!("{} on this page", crate::plural(on_page as u64, "asset"));
    if served {
        out.push_str(match data["order"].as_str() {
            Some("name") => " · by name",
            _ => " · newest change first",
        });
        let index = &data["index"];
        if let Some(generation) = index["generation"].as_i64() {
            out.push_str(&format!(" · index generation {generation}"));
        }
        if let Some(built) = index["built_at"]
            .as_str()
            .zip(index["checked_at"].as_str())
            .and_then(|(built, checked)| crate::ago(built, checked))
        {
            out.push_str(&format!(", built {built}"));
        }
    } else if let Some(scanned) = data["scanned"].as_u64() {
        out.push_str(&format!(" · {scanned} scanned"));
    }
    out.push('\n');
    if !served {
        out.push_str(crate::INDEX_UNAVAILABLE_NOTICE);
        out.push('\n');
    }
    let shown = on_page.min(MAX_LINES);
    // Times are relative to the moment the index was checked, which is this
    // read to within the index's own 30-second probe window: the same answer
    // renders the same way however late it is printed.
    let now = data["index"]["checked_at"].as_str();
    for row in rows.into_iter().flatten().take(shown) {
        if served {
            out.push_str(&timeline_line(row, now));
        } else {
            out.push_str(&crate::asset_line(row));
        }
    }
    if on_page > shown {
        out.push_str(&format!("  … {} more on this page\n", on_page - shown));
    }
    if data["more"].as_bool().unwrap_or(false) {
        match data["next_cursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty())
        {
            Some(cursor) => out.push_str(&format!("  … more; continue with --cursor {cursor}\n")),
            None => out.push_str("  … more\n"),
        }
    }
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str(
            "  ! the scan stopped at its bound; narrow with --folder, --kind or --since\n",
        );
    }
    let names = |list: &Value| -> Vec<String> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                item.as_str().map(str::to_owned).or_else(|| {
                    match (item["source"].as_str(), item["actual"].as_u64()) {
                        (Some(source), Some(actual)) => Some(format!(
                            "{source} ({} of {actual})",
                            item["bound"].as_u64().unwrap_or(0)
                        )),
                        _ => None,
                    }
                })
            })
            .collect()
    };
    let stale = names(&data["index"]["stale_sources"]);
    if !stale.is_empty() {
        out.push_str(&format!(
            "  ! not rebuilt since they changed: {}; --refresh rebuilds\n",
            crate::truncate(&stale.join(", "), 120)
        ));
    }
    let bounded = names(&data["index"]["truncated_sources"]);
    if !bounded.is_empty() {
        out.push_str(&format!(
            "  ! read to a bound: {}\n",
            crate::truncate(&bounded.join(", "), 120)
        ));
    }
    out
}

/// One index row as a timeline line: when it changed, its kind and name, its
/// version chip, and the id every other command takes.
fn timeline_line(row: &Value, now: Option<&str>) -> String {
    let when = row["modified_at"]
        .as_str()
        .and_then(|then| now.and_then(|now| crate::ago(then, now)))
        .unwrap_or_else(|| "—".to_owned());
    format!(
        "  {:>9}  {:<5} {:<44} {:<20} {}\n",
        when,
        row["kind"].as_str().unwrap_or("—"),
        crate::truncate(row["name"].as_str().unwrap_or("?"), 44),
        crate::versions_chip(&row["versions"]).unwrap_or_default(),
        row["asset_id"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::args::parse;

    use super::*;

    fn inputs(tokens: &[&str]) -> Inputs {
        let mut tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        tokens.extend(["--project".to_string(), "test_project".to_string()]);
        parse(&COMMAND, &tokens).expect("declared tokens parse")
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // The handler is held against its own declaration, with every flag
        // set.
        let payload = arguments(&inputs(&[
            "--order",
            "name",
            "--folder",
            " Transformers/TX-104 ",
            "--kind",
            "doc",
            "--status",
            "durable",
            "--sensitivity",
            "internal",
            "--since",
            "2026-09-01",
            "--limit",
            "25",
            "--cursor",
            " c_9 ",
            "--refresh",
        ]))
        .expect("valid");
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "cursor",
                "folder",
                "kind",
                "limit",
                "order",
                "refresh",
                "sensitivity",
                "since",
                "status"
            ]
        );
        assert_eq!(payload["folder"], json!("Transformers/TX-104"));
        assert_eq!(payload["order"], json!("name"));
        assert_eq!(payload["limit"], json!(25));
        assert_eq!(payload["cursor"], json!("c_9"));

        // The parser fills `--order` and `--limit` from their defaults, and
        // nothing else: the timeline is what an unqualified list reads.
        let bare = arguments(&inputs(&[])).expect("valid");
        assert_eq!(
            bare,
            json!({ "order": "recent", "limit": crate::DEFAULT_PAGE_SIZE })
        );
        let blank_cursor = arguments(&inputs(&["--cursor", "  "])).expect("valid");
        assert!(blank_cursor.get("cursor").is_none());
    }

    #[test]
    fn every_local_input_is_refused_by_name_before_the_round_trip() {
        let code = |tokens: &[&str]| {
            arguments(&inputs(tokens))
                .expect_err("must refuse")
                .code()
                .to_string()
        };
        assert_eq!(code(&["--limit", "201"]), "invalid_number");
        assert_eq!(code(&["--limit", "0"]), "invalid_number");
        assert_eq!(code(&["--since", "01-09-2026"]), "invalid_date");
        assert_eq!(code(&["--folder", "/contracts"]), "invalid_folder_path");
        let tokens: Vec<String> = ["--order", "oldest", "--project", "p"]
            .map(str::to_owned)
            .to_vec();
        assert!(parse(&COMMAND, &tokens).is_err(), "the order is closed");
    }

    #[test]
    fn the_timeline_prints_when_what_and_how_many_versions() {
        let out = render(&json!({
            "assets": [
                {"asset_id": "sys:transformers:TX-104", "name": "TX-104 LV design", "kind": "doc",
                 "folder_path": "Transformers/TX-104", "modified_at": "2026-09-25T07:05:00Z",
                 "versions": {"count": 12, "current": "v3", "latest_at": "2026-09-25T07:05:00Z"}},
                {"asset_id": "a_000000000001", "name": "EPC contract.pdf", "kind": "doc",
                 "folder_path": "contracts/2026", "modified_at": "2026-09-22T10:05:00Z"},
            ],
            "more": true,
            "next_cursor": "g7:2",
            "order": "recent",
            "index": {"schema": "ds.assets.index/v1", "generation": 7,
                      "built_at": "2026-09-25T09:05:00Z", "checked_at": "2026-09-25T10:05:00Z",
                      "stale_sources": ["solar"],
                      "truncated_sources": [{"source": "survey_media", "bound": 20000, "actual": 20431}]},
            "index_status": "served",
        }));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0],
            "2 assets on this page · newest change first · index generation 7, built 1h ago"
        );
        assert!(lines[1].trim_start().starts_with("3h ago"), "{out}");
        assert!(lines[1].contains("TX-104 LV design"), "{out}");
        assert!(lines[1].contains("v3 · 12 versions"), "{out}");
        assert!(lines[1].ends_with("sys:transformers:TX-104"), "{out}");
        assert!(lines[2].trim_start().starts_with("3d ago"), "{out}");
        assert!(out.contains("… more; continue with --cursor g7:2"));
        assert!(out.contains("! not rebuilt since they changed: solar; --refresh rebuilds"));
        assert!(out.contains("! read to a bound: survey_media (20000 of 20431)"));
        assert!(
            !out.contains("catalogue"),
            "a served page is the index: {out}"
        );
    }

    #[test]
    fn the_catalogue_fallback_says_it_is_not_the_index() {
        let rows: Vec<Value> = (0..MAX_LINES + 7)
            .map(|index| {
                json!({
                    "asset_id": format!("a_{index:012}"),
                    "folder": "contracts",
                    "name": format!("lot-{index}.pdf"),
                    "kind": "doc",
                    "status": "durable",
                    "sensitivity": "internal",
                    "bytes": 811_233,
                })
            })
            .collect();
        let out = render(&json!({
            "assets": rows,
            "next_cursor": "c_207",
            "more": true,
            "scanned": 400,
            "truncated": true,
            "index_status": "unavailable",
        }));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "207 assets on this page · 400 scanned");
        assert_eq!(lines[1], crate::INDEX_UNAVAILABLE_NOTICE);
        assert_eq!(
            out.matches("lot-").count(),
            MAX_LINES,
            "the projection must stop at its own bound"
        );
        assert!(out.contains("… 7 more on this page"), "{out}");
        assert!(out.contains("… more; continue with --cursor c_207"));
        assert!(out.ends_with(
            "! the scan stopped at its bound; narrow with --folder, --kind or --since\n"
        ));

        let short = render(&json!({ "assets": [], "more": false, "index_status": "unavailable" }));
        assert_eq!(
            short,
            format!(
                "0 assets on this page\n{}\n",
                crate::INDEX_UNAVAILABLE_NOTICE
            )
        );
    }
}
