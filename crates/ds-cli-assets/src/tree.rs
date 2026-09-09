//! `ds assets tree` — the folder tree, or the inside of one container.
//!
//! Two answers behind one verb, because they are the same question asked at
//! two depths: where does this project keep its documents, and what is inside
//! this one. Without `--into` the answer is the folder tree the Assets tab
//! renders — declared folders and the system folders projected from what the
//! project already holds. With `--into` it is one pack's central directory,
//! read but never unpacked.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DEPTH_ARG, DESCRIPTOR_ARG, FOLDER_ARG};

const INTO_ARG: Arg = Arg::value(
    "into",
    "<asset-id>",
    "Walk inside this `pack` asset instead: its members, sizes and shapefile companions.",
);

const QUERY_ARG: Arg = Arg::value(
    "query",
    "<text>",
    "Only rows whose name, path, kind, format, status or owner contains this (case-insensitive).",
);

const LINK_ARG: Arg = Arg::repeated(
    "link",
    "<kind:id>",
    "Only assets linked to this: pm_task:<id> or ds_object:<type>:<id>. One filter per read.",
);

const KIND_ARG: Arg = Arg::value(
    "kind",
    "<folder-kind>",
    "Only system (auto-indexed) or only user (declared) folders; absent means both.",
)
.choices(crate::FOLDER_KINDS);

/// The container listing this command's `--into` answer carries, told apart
/// from the folder tree by the schema the kernel stamps on it.
const CONTAINER_SCHEMA: &str = "ds.assets.container/v1";

/// The most rows one human projection prints before it says how many it did
/// not print. The JSON answer is whole; this is the terminal's bound, and it
/// is always followed by the count it left out.
const MAX_LINES: usize = 400;

pub static COMMAND: Command = Command {
    id: "assets.tree",
    path: &["assets", "tree"],
    contract: 1,
    summary: "Show the folder tree, or walk inside one container asset.",
    purpose: "\
Projects the open project's folder tree: the folders a person declared and the \
system folders auto-indexed from what the project already holds — transformer \
attachments and versions, MV model revisions, project-work attachments, report \
and print artifacts — each with its counts, expanded to --depth. With --into it \
walks one `pack` asset's central directory and lists its members with sizes, \
never unpacking to disk. --query and --link narrow to the same rows the Assets \
tab and the Project work page search, computed by the same kernel.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        FOLDER_ARG,
        DEPTH_ARG,
        INTO_ARG,
        QUERY_ARG,
        LINK_ARG,
        KIND_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`ds.assets.tree/v1`: `folders` nested to `depth`, each with `path`, `kind` \
(system or user), `counts`, `default_sensitivity` and its matched `assets`; \
`truncated` when a bound cut it. With --into, `ds.assets.container/v1`: the \
`members` of one pack with `path`, `bytes`, `kind`, `format` and shapefile \
`companions`, plus `walked` and `truncated` (5,000 members, 8 levels).",
    examples: &[Example {
        command: "ds assets tree --into a_7kq3nr2v0b1c --output json",
        note: "Read .data.members[].path to feed `preview --member` or `read --member`.",
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
        crate::INVALID_ASSET_ID,
        crate::INVALID_FOLDER_PATH,
        crate::INVALID_QUERY,
        crate::INVALID_LINK,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::ASSET_IS_NOT_A_FILE,
        crate::ASSET_TOO_LARGE,
        crate::ORIGIN_READ_FAILED,
        crate::ORIGIN_UNREACHABLE,
        crate::ORIGIN_READ_UNAVAILABLE,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The tree read, validated locally, in the exact keys the operation
/// declares. Only flags that were given travel.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(folder) = inputs.value("folder") {
        arguments.insert(
            "folder".into(),
            json!(crate::folder_path(folder, "folder")?),
        );
    }
    if let Some(depth) = inputs.value("depth") {
        arguments.insert(
            "depth".into(),
            json!(crate::integer(depth, "depth", 1, crate::MAX_TREE_DEPTH)?),
        );
    }
    if let Some(into) = inputs.value("into") {
        arguments.insert("into".into(), json!(crate::asset_id(into, "into")?));
    }
    if let Some(query) = inputs.value("query") {
        arguments.insert("query".into(), json!(crate::query(query, "query")?));
    }
    if let Some(link) = link_filter(inputs)? {
        arguments.insert("link".into(), link);
    }
    // The folder kinds are closed at the parser; an absent flag stays absent
    // on the wire, because "both kinds" is the tree's own answer to a
    // question that was not asked.
    if let Some(kind) = inputs.value("kind") {
        arguments.insert("kind".into(), json!(kind));
    }
    Ok(Value::Object(arguments))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_TREE,
        arguments,
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

/// The one `--link` filter, as the text the application's adapter parses:
/// `pm_task:<id>` or `ds_object:<type>:<id>`, exactly as it was typed. The
/// adapter turns it into the kernel's own `Link` shape on its side; sending
/// that shape from here would be a second parser for one grammar.
///
/// The flag is repeatable so a second one is this domain's typed refusal
/// rather than the parser's, and it is refused rather than folded: the tree
/// filters on one link, and quietly dropping the rest would answer a
/// different question from the one that was asked.
fn link_filter(inputs: &Inputs) -> Result<Option<Value>, Failure> {
    let given = inputs.repeated("link");
    let Some(first) = given.first() else {
        return Ok(None);
    };
    if given.len() > 1 {
        return Err(Failure::invalid(
            "invalid_link",
            format!(
                "`--link` was given {} times; a tree read filters on one link",
                given.len()
            ),
        )
        .remedy("pass one --link, or narrow with --folder and --query instead")
        .detail(json!({ "given": given.len(), "max": 1 })));
    }
    Ok(Some(json!(crate::link(first, "link")?)))
}

pub fn render(data: &Value) -> String {
    if data["schema"].as_str() == Some(CONTAINER_SCHEMA) || data["members"].is_array() {
        return render_container(data);
    }
    render_tree(data)
}

/// The folder tree, indented like an explorer, with the totals the response
/// reported rather than the ones these lines happen to show.
fn render_tree(data: &Value) -> String {
    let mut lines = Vec::new();
    for folder in data["folders"].as_array().into_iter().flatten() {
        push_folder(folder, 0, &mut lines);
    }
    let mut out = format!(
        "{} · {}\n",
        crate::plural(data["total_folders"].as_u64().unwrap_or(0), "folder"),
        crate::plural(data["total_assets"].as_u64().unwrap_or(0), "asset"),
    );
    let shown = lines.len().min(MAX_LINES);
    for line in &lines[..shown] {
        out.push_str(line);
    }
    if lines.len() > shown {
        out.push_str(&format!("  … {} more\n", lines.len() - shown));
    }
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str("  ! the tree stopped at a bound; narrow with --folder, --depth or --query\n");
    }
    out
}

fn push_folder(folder: &Value, level: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(level + 1);
    let name = folder["name"]
        .as_str()
        .or_else(|| folder["path"].as_str())
        .unwrap_or("?");
    let counts = &folder["counts"];
    lines.push(format!(
        "{indent}{}/  {} · {}{}\n",
        crate::truncate(name, 40),
        crate::plural(counts["assets"].as_u64().unwrap_or(0), "asset"),
        crate::plural(counts["folders"].as_u64().unwrap_or(0), "folder"),
        if folder["kind"].as_str() == Some("system") {
            "  [system]"
        } else {
            ""
        },
    ));
    for asset in folder["assets"].as_array().into_iter().flatten() {
        lines.push(asset_row(asset, level + 1));
    }
    let children = folder["children"].as_array();
    // The kernel expands at most `MAX_TREE_DEPTH` levels; the same bound is
    // applied here so a malformed answer cannot walk this renderer off the
    // stack, and what it stops at is reported rather than dropped.
    if level + 1 >= crate::MAX_TREE_DEPTH as usize {
        let deeper = children.map_or(0, Vec::len) as u64;
        if deeper > 0 {
            lines.push(format!(
                "{indent}  … {} not expanded at depth {}\n",
                crate::plural(deeper, "folder"),
                crate::MAX_TREE_DEPTH,
            ));
        }
        return;
    }
    for child in children.into_iter().flatten() {
        push_folder(child, level + 1, lines);
    }
}

fn asset_row(asset: &Value, level: usize) -> String {
    format!(
        "{}{:<28} {:<5} {:<7} {:>10}  {}\n",
        "  ".repeat(level + 1),
        crate::truncate(asset["asset_id"].as_str().unwrap_or("?"), 28),
        asset["kind"].as_str().unwrap_or("—"),
        asset["status"].as_str().unwrap_or("—"),
        asset["bytes"].as_u64().unwrap_or(0),
        crate::truncate(asset["name"].as_str().unwrap_or("?"), 44),
    )
}

/// One walked container: its members, and honestly what the walk did not
/// reach.
fn render_container(data: &Value) -> String {
    let members = data["members"].as_array();
    let listed = members.map_or(0, Vec::len);
    let total = data["member_count_total"].as_u64().unwrap_or(listed as u64);
    let mut out = format!(
        "{} · {} · {}{}\n",
        data["asset_id"].as_str().unwrap_or("?"),
        data["format"].as_str().unwrap_or("—"),
        crate::plural(total, "member"),
        if data["walked"].as_bool().unwrap_or(true) {
            ""
        } else {
            " · not walked"
        },
    );
    let mut lines = Vec::new();
    for member in members.into_iter().flatten() {
        lines.push(format!(
            "  {:<56} {:>10}  {}/{}{}\n",
            crate::truncate(member["path"].as_str().unwrap_or("?"), 56),
            member["bytes"].as_u64().unwrap_or(0),
            member["kind"].as_str().unwrap_or("—"),
            member["format"].as_str().unwrap_or("—"),
            if member["nested"].as_bool().unwrap_or(false) {
                "  [nested pack]"
            } else {
                ""
            },
        ));
        let companions: Vec<&str> = member["companions"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !companions.is_empty() {
            lines.push(format!(
                "      + {}\n",
                crate::truncate(&companions.join(", "), 72)
            ));
        }
    }
    let shown = lines.len().min(MAX_LINES);
    for line in &lines[..shown] {
        out.push_str(line);
    }
    if lines.len() > shown {
        out.push_str(&format!("  … {} more\n", lines.len() - shown));
    } else if total > listed as u64 {
        out.push_str(&format!("  … {} more\n", total - listed as u64));
    }
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  ! the walk stopped at its bound ({} members); preview one member instead\n",
            crate::MAX_CONTAINER_MEMBERS,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::args::parse;
    use ds_cli_contract::output::{Format, Output};

    use super::*;

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    /// A descriptor path that cannot exist, so nothing pairs and every local
    /// refusal below is proved to happen before the bridge is reached.
    fn unpaired() -> [String; 2] {
        [
            "--desktop-descriptor".to_string(),
            std::env::temp_dir()
                .join(format!(
                    "ds-cli-assets-tree-{}-absent.json",
                    std::process::id()
                ))
                .display()
                .to_string(),
        ]
    }

    fn refusal(flags: &[&str]) -> String {
        let mut tokens: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        run(&inputs, &context())
            .expect_err("an unpaired tree read cannot succeed")
            .code()
            .to_string()
    }

    #[test]
    fn every_local_input_is_refused_by_name_before_the_bridge() {
        assert_eq!(refusal(&["--depth", "9"]), "invalid_number");
        assert_eq!(refusal(&["--depth", "0"]), "invalid_number");
        assert_eq!(refusal(&["--folder", "/contracts"]), "invalid_folder_path");
        assert_eq!(refusal(&["--into", "a_7Kq3nR2v"]), "invalid_asset_id");
        assert_eq!(refusal(&["--query", ""]), "invalid_query");
        assert_eq!(refusal(&["--link", "task:t_1"]), "invalid_link");
        assert_eq!(
            refusal(&["--link", "pm_task:t_1", "--link", "pm_task:t_2"]),
            "invalid_link",
            "a second --link must be refused, never quietly dropped"
        );
    }

    #[test]
    fn a_well_formed_read_reaches_the_pairing_boundary() {
        // Everything this command validates locally is valid here, so the
        // only thing left to refuse is the desktop that is not running.
        let code = refusal(&[
            "--folder",
            "contracts/2026",
            "--depth",
            "8",
            "--into",
            "a_7kq3nr2v0b1c",
            "--query",
            "poles",
            "--link",
            "ds_object:transformer:TX-104",
            "--kind",
            "system",
        ]);
        assert!(
            !code.starts_with("invalid_"),
            "a well-formed read was refused locally as `{code}`"
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // `invoke` refuses an undeclared key, but only once a desktop has
        // paired — which no CI machine has. This is the one place the
        // handler is held against its own declaration, with every flag set.
        let mut tokens = [
            "--folder",
            "contracts/2026",
            "--depth",
            "8",
            "--into",
            "a_7kq3nr2v0b1c",
            "--query",
            " poles ",
            "--link",
            "ds_object:transformer:TX-104",
            "--kind",
            "system",
        ]
        .map(str::to_string)
        .to_vec();
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        let payload = arguments(&inputs).expect("valid");
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::ASSETS_TREE, &payload),
            None
        );
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["depth", "folder", "into", "kind", "link", "query"]);
        assert_eq!(payload["depth"], json!(8));
        assert_eq!(payload["query"], json!("poles"));

        // The parser fills `--depth` from its default, and nothing else.
        let inputs = parse(&COMMAND, &unpaired()).expect("declared tokens parse");
        assert_eq!(
            arguments(&inputs).expect("valid"),
            json!({ "depth": 3 }),
            "a flag nobody gave must not travel"
        );
    }

    #[test]
    fn a_link_is_sent_as_the_text_the_adapter_parses() {
        // The desktop adapter's `link()` validator takes the typed text and
        // builds the kernel's `Link` itself. What leaves here is therefore the
        // validated string under the one declared key, not a second reading of
        // it — a shape the parity suite cannot see, so this holds it.
        let mut tokens = vec![
            "--link".to_string(),
            " ds_object:transformer:TX-104 ".to_string(),
        ];
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        assert_eq!(
            link_filter(&inputs).expect("valid link"),
            Some(json!("ds_object:transformer:TX-104"))
        );

        let mut tokens = vec!["--link".to_string(), "pm_task:t_4812".to_string()];
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        assert_eq!(
            link_filter(&inputs).expect("valid link"),
            Some(json!("pm_task:t_4812"))
        );

        let inputs = parse(&COMMAND, &unpaired()).expect("declared tokens parse");
        assert_eq!(link_filter(&inputs).expect("no link"), None);
    }

    #[test]
    fn the_tree_prints_its_totals_and_says_what_it_cut() {
        let folders: Vec<Value> = (0..300)
            .map(|index| {
                json!({
                    "kind": "user",
                    "path": format!("contracts/{index}"),
                    "name": format!("{index}"),
                    "counts": { "assets": 1, "folders": 0 },
                    "children": [],
                    "assets": [{
                        "asset_id": format!("a_{index:012}"),
                        "name": format!("lot-{index}.pdf"),
                        "kind": "doc",
                        "status": "durable",
                        "bytes": 811_233,
                    }],
                })
            })
            .collect();
        let out = render(&json!({
            "schema": "ds.assets.tree/v1",
            "folders": folders,
            "total_assets": 300,
            "total_folders": 300,
            "truncated": true,
        }));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "300 folders · 300 assets");
        // header + MAX_LINES rows + the "… more" line + the bound line
        assert_eq!(lines.len(), MAX_LINES + 3);
        assert!(
            lines.iter().any(|line| line.trim() == "… 200 more"),
            "the count it did not print must be visible: {out}"
        );
        assert!(out.contains("! the tree stopped at a bound"));
        assert!(out.contains("a_000000000000"), "an asset row must render");
    }

    #[test]
    fn a_system_folder_is_named_and_deeper_levels_are_reported_not_dropped() {
        let mut folder = json!({
            "kind": "user",
            "path": "deep",
            "name": "deep",
            "counts": { "assets": 0, "folders": 1 },
            "children": [],
            "assets": [],
        });
        for level in 0..12 {
            folder = json!({
                // The outermost wrapper is the system root; the rest are
                // ordinary user folders nested under it.
                "kind": if level == 11 { "system" } else { "user" },
                "path": format!("level-{level}"),
                "name": format!("level-{level}"),
                "counts": { "assets": 0, "folders": 1 },
                "children": [folder],
                "assets": [],
            });
        }
        let out = render(&json!({
            "schema": "ds.assets.tree/v1",
            "folders": [folder],
            "total_assets": 0,
            "total_folders": 13,
            "truncated": false,
        }));
        assert!(
            out.contains("[system]"),
            "a system folder must say so: {out}"
        );
        assert!(
            out.contains("not expanded at depth 8"),
            "a nesting past the bound must be reported: {out}"
        );
    }

    #[test]
    fn a_container_walk_lists_members_companions_and_what_it_did_not_reach() {
        let out = render(&json!({
            "schema": "ds.assets.container/v1",
            "asset_id": "a_7kq3nr2v0b1c",
            "format": "zip",
            "walked": true,
            "truncated": true,
            "member_count_total": 5_002,
            "members": [
                { "path": "Lot3/drawings/site.pdf", "bytes": 811_233, "kind": "doc",
                  "format": "pdf", "nested": false },
                { "path": "Lot3/gis/poles.shp", "bytes": 44_112, "kind": "geo",
                  "format": "shp", "nested": false,
                  "companions": ["Lot3/gis/poles.dbf", "Lot3/gis/poles.shx"] },
                { "path": "Lot3/inner.zip", "bytes": 9_001, "kind": "pack",
                  "format": "zip", "nested": true }
            ],
        }));
        assert!(out.starts_with("a_7kq3nr2v0b1c · zip · 5002 members\n"));
        assert!(out.contains("Lot3/gis/poles.shp"));
        assert!(out.contains("+ Lot3/gis/poles.dbf, Lot3/gis/poles.shx"));
        assert!(out.contains("[nested pack]"));
        assert!(out.contains("… 4999 more"), "{out}");
        assert!(out.contains("! the walk stopped at its bound (5000 members)"));
    }
}
