//! `ds assets preview` — one asset, or one pack member, as a bounded document.
//!
//! The answer is a `PreviewDoc`: typed blocks the kernel decoded, which this
//! renderer lays out as text and nothing more. Nothing here decodes, converts
//! or rasterises anything — a format with no kernel decoder answers the note
//! it carries, and a PDF says that its renderer is pdf.js in the host.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, DESCRIPTOR_ARG, MEMBER_ARG, PAGES_ARG, ROWS_ARG, SHEET_ARG};

/// Rows of one grid a human projection prints. The document carries what the
/// §4 bounds allowed; this is the terminal's own bound on top of it, and it
/// is always followed by the count it left out.
const MAX_GRID_ROWS: usize = 40;
/// Lines of a text body, and items of a list, printed the same way.
const MAX_TEXT_LINES: usize = 40;
/// The widest one grid column is padded to before it is cut.
const COLUMN_WIDTH: usize = 24;

pub static COMMAND: Command = Command {
    id: "assets.preview",
    path: &["assets", "preview"],
    contract: 1,
    summary: "Preview one asset, or one pack member, as a bounded document.",
    purpose: "\
Returns a preview document — text blocks, a bounded grid, mail headers, a \
metadata card or feature geometry — decoded by the kernel inside the paired \
application from cached or freshly fetched bytes. Never rendered pixels: the \
caller gets text and the client does the drawing. A PDF answers a \
metadata-only document that names pdf.js as its renderer. Anything over a \
bound is refused with the bound and the actual number, never silently cut, \
and no preview ever fetches remote content.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        ASSET_ARG,
        MEMBER_ARG,
        SHEET_ARG,
        PAGES_ARG,
        ROWS_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`ds.assets.preview_doc/v1`: `asset_id`, `member`, `kind`, `format`, a `note`, \
and — as the format allows — `blocks`, `grid`, `headers`, `features` or `meta`, \
each with its own `truncated` count.",
    examples: &[Example {
        command: "ds assets preview --asset a_7kq3nr2v0b1c --member Lot3/gis/poles.shp --output json",
        note: "A geo member answers features; `ds assets promote` turns them into a local layer.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::ASSETS_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_ASSET_ID,
        crate::INVALID_NUMBER,
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
        crate::INVALID_MEMBER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The preview request, validated locally, in the exact keys the operation
/// declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "asset".into(),
        json!(crate::asset_id(inputs.require("asset")?, "asset")?),
    );
    // A whole-asset preview sends no member at all rather than an empty one,
    // which the walk would read as a member named "".
    if let Some(member) = inputs
        .value("member")
        .map(str::trim)
        .filter(|member| !member.is_empty())
    {
        arguments.insert("member".into(), json!(member));
    }
    // A worksheet name is only sent when given; the kernel opens the first
    // sheet otherwise (contract §7.1: `sheet` and `member` are distinct).
    if let Some(sheet) = inputs
        .value("sheet")
        .map(str::trim)
        .filter(|sheet| !sheet.is_empty())
    {
        arguments.insert("sheet".into(), json!(sheet));
    }
    if let Some(pages) = inputs.value("pages") {
        arguments.insert(
            "pages".into(),
            json!(crate::integer(pages, "pages", 1, crate::MAX_PREVIEW_PAGES)?),
        );
    }
    if let Some(rows) = inputs.value("rows") {
        arguments.insert(
            "rows".into(),
            json!(crate::integer(rows, "rows", 1, crate::MAX_PREVIEW_ROWS)?),
        );
    }
    Ok(Value::Object(arguments))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_PREVIEW,
        arguments,
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {}/{}",
        data["asset_id"].as_str().unwrap_or("preview"),
        data["kind"].as_str().unwrap_or("—"),
        data["format"].as_str().unwrap_or("—"),
    );
    if let Some(member) = data["member"].as_str().filter(|member| !member.is_empty()) {
        out.push_str(&format!(" · {}", crate::truncate(member, 60)));
    }
    out.push('\n');

    for block in data["blocks"].as_array().into_iter().flatten() {
        out.push_str(&block_lines(block));
    }

    let mut noted = false;
    for note in data["notes"].as_array().into_iter().flatten() {
        if let Some(note) = note.as_str().filter(|note| !note.is_empty()) {
            out.push_str(&format!("  note: {note}\n"));
            noted = true;
        }
    }
    // The reason a document was cut is the last thing a reader sees, and it
    // is never inferred: `truncated` is the document's own word for it.
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str(if noted {
            "  ! truncated at a bound; the notes above say where\n"
        } else {
            "  ! truncated at a bound\n"
        });
    }
    out
}

fn block_lines(block: &Value) -> String {
    match block["type"].as_str().unwrap_or("") {
        "heading" => {
            let level = block["level"].as_u64().unwrap_or(1).clamp(1, 6) as usize;
            format!(
                "\n  {} {}\n",
                "#".repeat(level),
                crate::truncate(text_of(&block["text"]), 100)
            )
        }
        "paragraph" => body(text_of(&block["text"]), "  "),
        "list" => list_lines(block),
        "grid" => grid_lines(block),
        "image" => format!(
            "  image {} · {} · {}×{}{}\n",
            block["index"].as_u64().unwrap_or(0),
            text_of(&block["mime"]),
            block["width"].as_u64().unwrap_or(0),
            block["height"].as_u64().unwrap_or(0),
            if block["bytes_b64"].is_string() {
                " · bytes carried, not printed"
            } else {
                ""
            },
        ),
        "mail" => mail_lines(block),
        "geo" => geo_lines(block),
        "meta" => meta_lines(block),
        // A block type this build does not know is named rather than dropped:
        // a reader must be able to tell "nothing here" from "not shown here".
        other => format!("  [{other} block, not rendered by this build]\n"),
    }
}

fn list_lines(block: &Value) -> String {
    let items: Vec<&str> = block["items"]
        .as_array()
        .map(|items| items.iter().map(text_of).collect())
        .unwrap_or_default();
    let ordered = block["ordered"].as_bool().unwrap_or(false);
    let shown = items.len().min(MAX_TEXT_LINES);
    let mut out = String::new();
    for (index, item) in items.iter().take(shown).enumerate() {
        let bullet = if ordered {
            format!("{}.", index + 1)
        } else {
            "-".to_string()
        };
        out.push_str(&format!("  {bullet} {}\n", crate::truncate(item, 100)));
    }
    if items.len() > shown {
        out.push_str(&format!(
            "  … {} more\n",
            crate::plural((items.len() - shown) as u64, "item")
        ));
    }
    out
}

fn grid_lines(block: &Value) -> String {
    let columns: Vec<&str> = block["columns"]
        .as_array()
        .map(|columns| columns.iter().map(text_of).collect())
        .unwrap_or_default();
    let rows: Vec<Vec<&str>> = block["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .map(|cells| cells.iter().map(text_of).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let shown = rows.len().min(MAX_GRID_ROWS);

    let mut widths: Vec<usize> = columns
        .iter()
        .map(|column| column.chars().count().min(COLUMN_WIDTH))
        .collect();
    for row in rows.iter().take(shown) {
        for (index, cell) in row.iter().enumerate() {
            let width = cell.chars().count().min(COLUMN_WIDTH);
            match widths.get_mut(index) {
                Some(current) => *current = (*current).max(width),
                None => widths.push(width),
            }
        }
    }
    let line = |cells: &[&str]| -> String {
        let mut text = String::from("  ");
        for (index, width) in widths.iter().enumerate() {
            let cell = cells.get(index).copied().unwrap_or("");
            text.push_str(&format!(
                "{:<width$}  ",
                crate::truncate(cell, *width),
                width = *width
            ));
        }
        format!("{}\n", text.trim_end())
    };

    let mut out = String::new();
    if let Some(title) = block["title"].as_str().filter(|title| !title.is_empty()) {
        out.push_str(&format!("\n  {}\n", crate::truncate(title, 72)));
    }
    if !columns.is_empty() {
        out.push_str(&line(&columns));
        let rule: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
        let rule: Vec<&str> = rule.iter().map(String::as_str).collect();
        out.push_str(&line(&rule));
    }
    for row in rows.iter().take(shown) {
        out.push_str(&line(row));
    }
    // What the terminal did not print and what the document itself did not
    // carry are told together: `rows_total` is the sheet's own count.
    let rows_total = block["rows_total"].as_u64().unwrap_or(rows.len() as u64);
    if rows_total > shown as u64 {
        out.push_str(&format!(
            "  … {} more of {rows_total}\n",
            crate::plural(rows_total - shown as u64, "row")
        ));
    }
    let columns_total = block["columns_total"]
        .as_u64()
        .unwrap_or(columns.len() as u64);
    if columns_total > columns.len() as u64 {
        out.push_str(&format!(
            "  … {} more\n",
            crate::plural(columns_total - columns.len() as u64, "column")
        ));
    }
    out
}

fn mail_lines(block: &Value) -> String {
    let mut out = String::new();
    for header in block["headers"].as_array().into_iter().flatten() {
        let pair = header.as_array();
        let key = pair.and_then(|pair| pair.first()).map_or("", text_of);
        let value = pair.and_then(|pair| pair.get(1)).map_or("", text_of);
        out.push_str(&format!(
            "  {:<12} {}\n",
            crate::truncate(key, 12),
            crate::truncate(value, 76)
        ));
    }
    let text = text_of(&block["text"]);
    if !text.is_empty() {
        out.push('\n');
        out.push_str(&body(text, "  "));
    }
    for attachment in block["attachments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  attachment: {} · {} · {}\n",
            text_of(&attachment["name"]),
            text_of(&attachment["mime"]),
            crate::plural(attachment["bytes"].as_u64().unwrap_or(0), "byte"),
        ));
    }
    out
}

fn geo_lines(block: &Value) -> String {
    let mut out = format!("  geo · decoder {}\n", text_of(&block["decoder"]));
    if let Some(refused) = block["refused"]
        .as_str()
        .filter(|reason| !reason.is_empty())
    {
        out.push_str(&format!("  refused: {refused}\n"));
        return out;
    }
    if let Some(count) = block["feature_count"].as_u64() {
        out.push_str(&format!("  {}\n", crate::plural(count, "feature")));
    }
    if let Some(bbox) = block["bbox"].as_array().filter(|bbox| bbox.len() == 4) {
        let corners: Vec<String> = bbox
            .iter()
            .map(|value| match value.as_f64() {
                Some(number) => format!("{number:.6}"),
                None => "—".to_string(),
            })
            .collect();
        out.push_str(&format!("  bbox {}\n", corners.join(" ")));
    }
    let companions: Vec<&str> = block["companions"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !companions.is_empty() {
        out.push_str(&format!(
            "  companions: {}\n",
            crate::truncate(&companions.join(", "), 72)
        ));
    }
    out
}

fn meta_lines(block: &Value) -> String {
    let mut out = String::new();
    for entry in block["entries"].as_array().into_iter().flatten() {
        let pair = entry.as_array();
        let key = pair.and_then(|pair| pair.first()).map_or("", text_of);
        let value = pair.and_then(|pair| pair.get(1)).map_or("", text_of);
        out.push_str(&format!(
            "  {:<16} {}\n",
            crate::truncate(key, 16),
            crate::truncate(value, 72)
        ));
    }
    out
}

/// A text body, one line per line, bounded and honest about the rest.
fn body(text: &str, indent: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let shown = lines.len().min(MAX_TEXT_LINES);
    let mut out = String::new();
    for line in &lines[..shown] {
        out.push_str(&format!("{indent}{}\n", crate::truncate(line, 100)));
    }
    if lines.len() > shown {
        out.push_str(&format!(
            "{indent}… {} more\n",
            crate::plural((lines.len() - shown) as u64, "line")
        ));
    }
    out
}

fn text_of(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::args::parse;
    use ds_cli_contract::output::{Format, Output};
    use ds_cli_desktop::ops::undeclared_key;

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
                    "ds-cli-assets-preview-{}-absent.json",
                    std::process::id()
                ))
                .display()
                .to_string(),
        ]
    }

    fn inputs(flags: &[&str]) -> Inputs {
        let mut tokens: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
        tokens.extend(unpaired());
        parse(&COMMAND, &tokens).expect("declared tokens parse")
    }

    fn refusal(flags: &[&str]) -> String {
        run(&inputs(flags), &context())
            .expect_err("an unpaired preview cannot succeed")
            .code()
            .to_string()
    }

    #[test]
    fn the_asset_and_both_bounds_are_refused_by_name_before_the_bridge() {
        assert_eq!(
            refusal(&["--asset", "a_7Kq3nR2v"]),
            "invalid_asset_id",
            "a truncated paste must cost a local refusal, not a round trip"
        );
        for over in [
            vec!["--asset", "a_7kq3nr2v0b1c", "--pages", "6"],
            vec!["--asset", "a_7kq3nr2v0b1c", "--pages", "0"],
            vec!["--asset", "a_7kq3nr2v0b1c", "--rows", "201"],
            vec!["--asset", "a_7kq3nr2v0b1c", "--rows", "many"],
        ] {
            assert_eq!(refusal(&over), "invalid_number", "{over:?} was accepted");
        }
    }

    #[test]
    fn a_number_refusal_carries_the_bound_and_the_number_given() {
        let failure = run(
            &inputs(&["--asset", "a_7kq3nr2v0b1c", "--rows", "201"]),
            &context(),
        )
        .expect_err("201 rows is over the bound");
        let detail = failure.detail_value().cloned().unwrap_or(Value::Null);
        assert_eq!(detail["given"], json!(201), "the number given must be said");
        assert_eq!(
            detail["max"],
            json!(crate::MAX_PREVIEW_ROWS),
            "the bound must be said"
        );
        assert!(
            failure
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("200")),
            "the remedy must carry the bound"
        );
    }

    #[test]
    fn a_well_formed_preview_reaches_the_pairing_boundary() {
        let code = refusal(&[
            "--asset",
            "sys:design_attachment:att_1:rev_2",
            "--member",
            "Lot3/gis/poles.shp",
            "--pages",
            "5",
            "--rows",
            "200",
        ]);
        assert!(
            !code.starts_with("invalid_"),
            "a well-formed preview was refused locally as `{code}`"
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // `invoke` refuses an undeclared key, but only once a desktop has
        // paired — which no CI machine has. This is the one place the
        // handler is held against its own declaration, with every flag set.
        let payload = arguments(&inputs(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            " Lot3/sheets/boq.xlsx ",
            "--pages",
            "2",
            "--rows",
            "50",
        ]))
        .expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_PREVIEW, &payload), None);
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["asset", "member", "pages", "rows"]);
        assert_eq!(payload["member"], json!("Lot3/sheets/boq.xlsx"));
        assert_eq!(payload["pages"], json!(2));

        // A whole-asset preview sends no member at all rather than an empty
        // one; the two bounds arrive from their defaults.
        let whole =
            arguments(&inputs(&["--asset", "a_7kq3nr2v0b1c", "--member", "  "])).expect("valid");
        assert_eq!(
            whole,
            json!({
                "asset": "a_7kq3nr2v0b1c",
                "pages": crate::MAX_PREVIEW_PAGES,
                "rows": crate::MAX_PREVIEW_ROWS
            })
        );
    }

    #[test]
    fn a_document_renders_every_block_kind_and_ends_with_the_reason_it_was_cut() {
        let rows: Vec<Value> = (0..500)
            .map(|index| json!([format!("TX-{index:03}"), format!("{index}"), "durable"]))
            .collect();
        let long_body: String = (0..MAX_TEXT_LINES + 5)
            .map(|index| format!("line {index}\n"))
            .collect();
        let many_items: Vec<Value> = (0..MAX_TEXT_LINES + 2)
            .map(|index| json!(format!("item {index}")))
            .collect();
        let out = render(&json!({
            "schema": "ds.assets.preview_doc/v1",
            "asset_id": "a_7kq3nr2v0b1c",
            "member": "Lot3/sheets/boq.xlsx",
            "kind": "sheet",
            "format": "xlsx",
            "bounded": true,
            "truncated": true,
            "blocks": [
                { "type": "heading", "level": 2, "text": "Lot 3 bill of quantities" },
                { "type": "paragraph", "text": "Signed 2026-08-14.\nCountersigned 2026-08-19." },
                { "type": "paragraph", "text": long_body },
                { "type": "list", "items": ["poles", "conductor", "transformers"],
                  "ordered": false },
                { "type": "list", "items": many_items, "ordered": true },
                { "type": "grid", "title": "Sheet 1", "columns": ["name", "count", "status"],
                  "rows": rows, "rows_total": 500, "rows_shown": 500, "columns_total": 7 },
                { "type": "mail", "headers": [["From", "epc@example.test"],
                  ["Subject", "Lot 3"]], "text": "Attached.", "attachments": [
                  { "name": "boq.xlsx", "mime": "application/vnd.ms-excel", "bytes": 44112 }] },
                { "type": "geo", "decoder": "ds-io", "member": "Lot3/gis/poles.shp",
                  "companions": ["Lot3/gis/poles.dbf"], "feature_count": 1240,
                  "bbox": [29.1, -2.1, 30.2, -1.4], "refused": null },
                { "type": "meta", "entries": [["pages", "42"], ["producer", "LibreOffice"]] },
                { "type": "sparkline", "values": [1, 2, 3] }
            ],
            "notes": ["the first sheet only; switch sheets in the deck"],
        }));

        assert!(out.starts_with("a_7kq3nr2v0b1c · sheet/xlsx · Lot3/sheets/boq.xlsx\n"));
        assert!(out.contains("## Lot 3 bill of quantities"));
        assert!(out.contains("  Countersigned 2026-08-19"));
        assert!(
            out.contains("  … 5 lines more"),
            "a long body says what it cut: {out}"
        );
        assert!(out.contains("  - poles"));
        assert!(out.contains("  1. item 0"));
        assert!(
            out.contains("  … 2 items more"),
            "a long list says what it cut: {out}"
        );
        assert!(out.contains("  Sheet 1"));
        assert!(
            out.contains("name    count  status"),
            "columns must align: {out}"
        );
        assert_eq!(
            out.matches("TX-").count(),
            MAX_GRID_ROWS,
            "the grid must stop at its own bound"
        );
        assert!(out.contains("… 460 rows more of 500"), "{out}");
        assert!(out.contains("… 4 columns more"));
        assert!(
            out.lines()
                .any(|line| line.starts_with("  From") && line.ends_with("epc@example.test")),
            "a mail header is one aligned line: {out}"
        );
        assert!(out.contains("attachment: boq.xlsx"));
        assert!(out.contains("geo · decoder ds-io"));
        assert!(out.contains("1240 features"));
        assert!(out.contains("bbox 29.100000 -2.100000 30.200000 -1.400000"));
        assert!(out.contains("companions: Lot3/gis/poles.dbf"));
        assert!(
            out.lines()
                .any(|line| line.starts_with("  pages") && line.ends_with(" 42")),
            "a meta entry is one aligned line: {out}"
        );
        assert!(
            out.contains("[sparkline block, not rendered by this build]"),
            "an unknown block must be named, never dropped: {out}"
        );
        assert!(out.contains("note: the first sheet only"));
        assert!(
            out.ends_with("! truncated at a bound; the notes above say where\n"),
            "the reason must be the last line: {out}"
        );
        // Every cut this projection makes is said the same way, so one grep
        // finds them all.
        assert_eq!(
            out.matches("… ").count(),
            4,
            "body, list, grid rows, grid columns — and nothing cut silently: {out}"
        );
    }

    #[test]
    fn a_pdf_answers_its_note_and_this_renderer_never_pretends_to_draw_it() {
        let out = render(&json!({
            "schema": "ds.assets.preview_doc/v1",
            "asset_id": "a_7kq3nr2v0b1c",
            "kind": "doc",
            "format": "pdf",
            "bounded": true,
            "truncated": false,
            "blocks": [{ "type": "meta", "entries": [["pages", "42"], ["bytes", "811233"]] }],
            "notes": ["pdf pages are rendered by pdf.js in the host; this is the metadata card"],
        }));
        assert!(out.starts_with("a_7kq3nr2v0b1c · doc/pdf\n"));
        assert!(
            out.lines()
                .any(|line| line.starts_with("  pages") && line.ends_with(" 42")),
            "a meta entry is one aligned line: {out}"
        );
        assert!(out.contains("note: pdf pages are rendered by pdf.js in the host"));
        assert!(
            !out.contains("page 1"),
            "no page of a pdf is ever drawn here: {out}"
        );
    }

    #[test]
    fn a_refused_geo_block_says_why_and_counts_nothing() {
        let out = render(&json!({
            "schema": "ds.assets.preview_doc/v1",
            "asset_id": "a_7kq3nr2v0b1c",
            "kind": "geo",
            "format": "gpkg",
            "bounded": true,
            "truncated": false,
            "blocks": [{ "type": "geo", "decoder": "ds-io", "member": null,
                         "companions": [], "feature_count": null, "bbox": null,
                         "refused": "no SQLite reader in WASM; open it in the desktop" }],
            "notes": [],
        }));
        assert!(out.contains("refused: no SQLite reader in WASM"));
        assert!(!out.contains("feature"), "nothing was counted: {out}");
    }
}
