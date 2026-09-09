//! `ds assets list` — the project's catalogue as bounded rows.
//!
//! The entry point when nothing else is known: `read`, `preview`, `classify`,
//! `promote` and `attach` all need an asset id, and this and `tree` are where
//! one comes from.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{CURSOR_ARG, DESCRIPTOR_ARG, FOLDER_ARG, LIMIT_ARG};

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
    "Only assets received on or after this date (YYYY-MM-DD or RFC 3339).",
);

/// The most rows one human projection prints before it says how many it did
/// not print. A page is bounded at [`crate::MAX_PAGE_SIZE`] by the owner, so
/// this only ever bites on a malformed answer — and then it is reported, not
/// swallowed. The JSON answer is whole.
const MAX_LINES: usize = crate::MAX_PAGE_SIZE as usize;

pub static COMMAND: Command = Command {
    id: "assets.list",
    path: &["assets", "list"],
    contract: 1,
    summary: "List the project's assets, one bounded page at a time.",
    purpose: "\
Names the assets the signed-in user may see in the paired application's open \
project, newest first, with each one's folder, kind, format, size, status and \
sensitivity. This is where an assets session starts: every other `ds assets` \
command needs an asset_id from here or from `ds assets tree`. A restricted or \
confidential asset the caller cannot read has no row, no name and no count — \
absence is the answer, never a placeholder. Reads the same catalogue the Assets \
tab renders and changes nothing; offline it serves the cached catalogue, \
labelled with its age.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        FOLDER_ARG,
        KIND_ARG,
        STATUS_ARG,
        SENSITIVITY_ARG,
        SINCE_ARG,
        LIMIT_ARG,
        CURSOR_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`assets` rows of `asset_id`, `folder`, `name`, `kind`, `format`, `bytes`, \
`digest`, `status`, `sensitivity`, `dated`, `owner`, `source`, `container` and \
`links`; then `next_cursor` and `more` for the next page, `scanned` for how many \
rows the read considered, and `truncated` when a scan bound stopped it early.",
    examples: &[Example {
        command: "ds assets list --folder contracts --status durable --output json",
        note: "Read .data.assets[].asset_id to feed read, preview, classify, promote or attach.",
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
        crate::INVALID_NUMBER,
        crate::INVALID_DATE,
        crate::INVALID_FOLDER_PATH,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::UNKNOWN_FOLDER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The page request, validated locally, in the exact keys the operation
/// declares. Only flags that were given travel.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(folder) = inputs.value("folder") {
        arguments.insert(
            "folder".into(),
            json!(crate::folder_path(folder, "folder")?),
        );
    }
    // The vocabularies are closed at the parser, so a value that reaches here
    // is one of the contract's words; an absent flag stays absent on the wire
    // because the application's own default (everything but archive) is the
    // answer to "no status was asked for".
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
    Ok(Value::Object(arguments))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_LIST,
        arguments,
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    let rows = data["assets"].as_array();
    let on_page = rows.map_or(0, Vec::len);
    let mut out = format!("{} on this page", crate::plural(on_page as u64, "asset"));
    if let Some(scanned) = data["scanned"].as_u64() {
        out.push_str(&format!(" · {scanned} scanned"));
    }
    if let Some(age) = data["cache_age"].as_str() {
        out.push_str(&format!(" · cached {age} ago"));
    }
    out.push('\n');
    let shown = on_page.min(MAX_LINES);
    for row in rows.into_iter().flatten().take(shown) {
        out.push_str(&crate::asset_line(row));
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
    out
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::args::parse;
    use ds_cli_desktop::ops::undeclared_key;

    use super::*;

    fn inputs(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        parse(&COMMAND, &tokens).expect("declared tokens parse")
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // `invoke` refuses an undeclared key, but only once a desktop has
        // paired — which no CI machine has. This is the one place the
        // handler is held against its own declaration, with every flag set.
        let payload = arguments(&inputs(&[
            "--folder",
            " contracts/2026 ",
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
        ]))
        .expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_LIST, &payload), None);
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
                "sensitivity",
                "since",
                "status"
            ]
        );
        assert_eq!(payload["folder"], json!("contracts/2026"));
        assert_eq!(payload["limit"], json!(25));
        assert_eq!(payload["cursor"], json!("c_9"));

        // The parser fills `--limit` from its default, and nothing else: a
        // flag nobody gave is absent, not null, so the application's own
        // default answers the question that was not asked.
        let bare = arguments(&inputs(&[])).expect("valid");
        assert_eq!(bare, json!({ "limit": crate::DEFAULT_PAGE_SIZE }));
        let blank_cursor = arguments(&inputs(&["--cursor", "  "])).expect("valid");
        assert!(blank_cursor.get("cursor").is_none());
    }

    #[test]
    fn every_local_input_is_refused_by_name_before_the_bridge() {
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
    }

    #[test]
    fn the_page_prints_its_rows_and_says_what_it_cut() {
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
        }));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "207 assets on this page · 400 scanned");
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

        let short = render(&json!({ "assets": [], "more": false }));
        assert_eq!(short, "0 assets on this page\n");
    }
}
