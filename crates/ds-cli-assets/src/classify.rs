//! `ds assets classify` — change one asset's kind, status, owner, folder or
//! sensitivity, audited.
//!
//! One invocation is one audited change. Every flag given travels as a single
//! patch, and a flag left out is a field left alone rather than a field
//! cleared — the same shape `ds pm task update` has, for the same reason: a
//! caller changing a status must not have to restate a classification it does
//! not know.
//!
//! Two things are refused here rather than after a round trip. An invocation
//! that names no change would still cost an audit row for nothing, and a
//! projected `sys:` row has no stored document to patch at all (D18) — the
//! thing to classify is the object it projects.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, CatalogueCommand, FOLDER_ARG, LANE_ARG};

const KIND_ARG: Arg =
    Arg::value("kind", "<kind>", "Override the kind the kernel inferred.").choices(crate::KINDS);

const STATUS_ARG: Arg = Arg::value(
    "status",
    "<status>",
    "fresh, durable (cite this one) or archive (hidden from the default listing).",
)
.choices(crate::STATUSES);

const OWNER_ARG: Arg = Arg::value(
    "owner",
    "<email>",
    "The accountable person, not the uploader.",
);

const SENSITIVITY_ARG: Arg = Arg::value(
    "sensitivity",
    "<class>",
    "The access class; loosening needs the capability for the class being left.",
)
.choices(crate::SENSITIVITIES);

const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why, for the audit row; expected when sensitivity loosens.",
);

const DOCUMENT_NUMBER_ARG: Arg = Arg::value(
    "document-number",
    "<number>",
    "Register the asset as a numbered document (correspondence.md): with --document-revision and --document-state, all three together.",
);
const DOCUMENT_REVISION_ARG: Arg = Arg::value(
    "document-revision",
    "<label>",
    "The document's revision label, e.g. B or 02.",
);
const DOCUMENT_STATE_ARG: Arg = Arg::value(
    "document-state",
    "<state>",
    "The document's state in the attachment vocabulary.",
)
.choices(crate::DOCUMENT_STATES);

const MAX_REASON_CHARS: usize = 500;

const INVALID_REASON: Refusal = Refusal {
    code: "invalid_reason",
    when: "--reason is empty, or longer than 500 characters",
    remedy: "give one short sentence for the audit row, e.g. --reason signed copy received",
};

/// The flags that actually move the row.
///
/// `--reason` is deliberately not one of them: it annotates the audit entry,
/// so an invocation carrying only a reason changes nothing and is refused
/// rather than audited.
const CHANGE_KEYS: &[&str] = &[
    "kind",
    "status",
    "owner",
    "folder_id",
    "sensitivity",
    "document",
];

pub static COMMAND: Command = Command {
    id: "assets.classify",
    path: &["assets", "classify"],
    contract: 2,
    summary: "Change an asset's kind, status, owner, folder, class; register a doc.",
    purpose: "\
Applies every flag given as one audited catalogue change through ds-brain, \
which decides whether the caller may make it: loosening sensitivity needs the \
capability for the class being left, and a person's override is never \
re-inferred away. Nothing given, nothing sent — a flag you omit is untouched. \
Projected sys: rows are read-only and refused by name. The change is pinned to \
the row's current version, so a row that moved is refused rather than \
overwritten. Headless: the selected project of the signed-in native \
credential, no window.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ASSET_ARG,
        KIND_ARG,
        STATUS_ARG,
        OWNER_ARG,
        FOLDER_ARG,
        SENSITIVITY_ARG,
        DOCUMENT_NUMBER_ARG,
        DOCUMENT_REVISION_ARG,
        DOCUMENT_STATE_ARG,
        REASON_ARG,
        LANE_ARG,
    ],
    output: "\
`asset` — the row after the change — the `audit_id` recorded for it, and \
`changed`: the names of the fields that actually moved.",
    examples: &[Example {
        command: "ds assets classify --asset a_7kq3nr2v0b1c --document-number GTP-001 --document-revision B --document-state issued --yes",
        note: "The asset is now a registered document a transmittal record can carry.",
        runnable: false,
    }],
    refusals: &crate::refusals::<30>(&[
        crate::INVALID_ASSET_ID,
        crate::INVALID_FOLDER_PATH,
        crate::PROJECTED_ASSET_READ_ONLY,
        crate::NOTHING_TO_UPDATE,
        crate::CONFIRMATION_REQUIRED,
        crate::UNKNOWN_FOLDER,
        crate::INVALID_DOCUMENT_REGISTRATION,
        INVALID_REASON,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The patch, validated locally, in the exact keys the catalogue's
/// `classify` reads. A folder is carried as its path here and resolved to
/// its id at the door.
type ClassificationArgs = (String, Map<String, Value>, Option<String>);

fn arguments(inputs: &Inputs) -> Result<ClassificationArgs, Failure> {
    let asset = crate::asset_id(inputs.require("asset")?, "asset")?;
    if crate::is_projected(&asset) {
        return Err(Failure::invalid(
            "projected_asset_read_only",
            format!("`{asset}` is a projected system row, which has no stored document to change"),
        )
        .remedy(crate::PROJECTED_ASSET_READ_ONLY.remedy)
        .detail(json!({ "asset": asset })));
    }

    let mut patch = Map::new();
    // kind, status and sensitivity are closed at the parser, so a value that
    // reaches here is one of the contract's own words and needs no second
    // vocabulary check.
    for flag in ["kind", "status", "sensitivity"] {
        if let Some(value) = inputs.value(flag) {
            patch.insert(flag.into(), json!(value));
        }
    }
    if let Some(owner) = inputs.value("owner") {
        patch.insert(
            "owner".into(),
            json!({ "kind": "user", "id": owner.trim() }),
        );
    }
    let mut folder = None;
    if let Some(path) = inputs.value("folder") {
        let path = crate::folder_path(path, "folder")?;
        if let Some(root) = crate::folder::system_root(&path) {
            return Err(crate::folder::system_folder_refusal("folder", &path, root));
        }
        // Resolved to its id at the door; a placeholder marks the change.
        patch.insert("folder_id".into(), Value::Null);
        folder = Some(path);
    }
    let document = (
        inputs.value("document-number").map(str::trim),
        inputs.value("document-revision").map(str::trim),
        inputs.value("document-state").map(str::trim),
    );
    match document {
        (None, None, None) => {}
        (Some(number), Some(revision), Some(state))
            if !number.is_empty() && !revision.is_empty() =>
        {
            patch.insert(
                "document".into(),
                json!({ "number": number, "revision_label": revision, "state": state }),
            );
        }
        _ => {
            return Err(Failure::invalid(
                crate::INVALID_DOCUMENT_REGISTRATION.code,
                "a document registration needs --document-number, --document-revision and --document-state together",
            )
            .remedy(crate::INVALID_DOCUMENT_REGISTRATION.remedy)
            .next("ds assets classify --help"));
        }
    }
    if let Some(reason) = inputs.value("reason") {
        patch.insert("reason".into(), json!(audit_reason(reason)?));
    }

    // `--asset` alone, or with only a reason, is a write that writes nothing:
    // one audit row, one round trip and no change. It costs a local refusal
    // instead.
    if !CHANGE_KEYS.iter().any(|key| patch.contains_key(*key)) {
        return Err(Failure::invalid(
            "nothing_to_update",
            "no kind, status, owner, folder, sensitivity or document flag was given",
        )
        .remedy(crate::NOTHING_TO_UPDATE.remedy)
        .next("ds assets classify --help"));
    }

    Ok((asset, patch, folder))
}

fn audit_reason(raw: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let characters = trimmed.chars().count();
    if trimmed.is_empty() || characters > MAX_REASON_CHARS {
        return Err(Failure::invalid(
            "invalid_reason",
            format!("`--reason` must be 1 to {MAX_REASON_CHARS} characters"),
        )
        .remedy(INVALID_REASON.remedy)
        .detail(json!({ "given_chars": characters, "max": MAX_REASON_CHARS })));
    }
    Ok(trimmed.to_string())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (asset_id, mut patch, folder) = arguments(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    if let Some(path) = folder {
        patch.insert("folder_id".into(), json!(crate::folder_id(lane, &path)?));
    }
    // The row's current version pins the change: a row that moved between
    // this read and the write is refused by the catalogue, never overwritten.
    let current = crate::catalogue(
        lane,
        &CatalogueCommand::Get {
            asset_id: asset_id.clone(),
        },
    )?;
    if patch.contains_key("document") {
        return Ok(ds_cli_auth::shared_assets(
            lane,
            &ds_cli_auth::SharedAssetsCommand::Classify {
                asset_id,
                expected_version: current["version"].as_i64().unwrap_or(1),
                patch,
            },
        )?
        .into_result());
    }
    let text = |key: &str| patch.get(key).and_then(Value::as_str).map(str::to_owned);
    crate::catalogue(
        lane,
        &CatalogueCommand::Classify {
            asset_id,
            kind: text("kind"),
            status: text("status"),
            sensitivity: text("sensitivity"),
            folder_id: text("folder_id"),
            owner: patch
                .get("owner")
                .and_then(|v| v["id"].as_str())
                .map(str::to_owned),
            reason: text("reason"),
            expected_version: current["version"].as_u64().unwrap_or(1),
        },
    )
}

pub fn render(data: &Value) -> String {
    let changed: Vec<&str> = data["changed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let row = &data["asset"];
    let mut out = format!(
        "classified {} · {}\n",
        row["asset_id"].as_str().unwrap_or("?"),
        if changed.is_empty() {
            "no field moved".to_string()
        } else {
            changed.join(", ")
        },
    );
    if let Some(audit) = data["audit_id"].as_str().filter(|audit| !audit.is_empty()) {
        out.push_str(&format!("  audit {audit}\n"));
    }
    if row.is_object() {
        out.push_str(&crate::asset_line(row));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        if let Some(message) = warning["message"].as_str() {
            out.push_str(&format!("  ! {message}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::spec::ArgKind;

    fn parse(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn refused(tokens: &[&str]) -> Failure {
        arguments(&parse(tokens)).expect_err("must refuse")
    }

    #[test]
    fn a_patch_that_changes_nothing_never_reaches_the_project() {
        assert_eq!(
            refused(&["--asset", "a_7kq3nr2v0b1c"]).code(),
            "nothing_to_update"
        );
        assert_eq!(
            refused(&["--asset", "a_7kq3nr2v0b1c", "--reason", "tidying"]).code(),
            "nothing_to_update"
        );
        assert!(
            arguments(&parse(&[
                "--asset",
                "a_7kq3nr2v0b1c",
                "--status",
                "durable"
            ]))
            .is_ok()
        );
    }

    #[test]
    fn a_projected_row_and_a_system_folder_are_refused_by_name_before_the_door() {
        let failure = refused(&[
            "--asset",
            "sys:design_attachment:att_1:rev_2",
            "--status",
            "durable",
        ]);
        assert_eq!(failure.code(), "projected_asset_read_only");
        for path in ["Transformers/AGASHARU/reports", "prints/local"] {
            assert_eq!(
                refused(&["--asset", "a_7kq3nr2v0b1c", "--folder", path]).code(),
                "projected_asset_read_only",
                "`{path}` was accepted as a classification target"
            );
        }
        assert_eq!(
            refused(&["--asset", "a_7kq3nr2v0b1c", "--folder", "contracts//epc"]).code(),
            "invalid_folder_path"
        );
        let (_, patch, folder) = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--folder",
            "contracts/2026/epc",
        ]))
        .expect("valid");
        assert_eq!(folder.as_deref(), Some("contracts/2026/epc"));
        assert!(
            patch.contains_key("folder_id"),
            "the folder travels as its id once resolved"
        );
    }

    #[test]
    fn a_document_registration_is_whole_or_refused() {
        let asset = ["--asset", "a_7kq3nr2v0b1c"];
        for partial in [
            vec!["--document-number", "GTP-001"],
            vec!["--document-number", "GTP-001", "--document-revision", "B"],
            vec!["--document-state", "issued"],
        ] {
            let mut tokens: Vec<&str> = asset.to_vec();
            tokens.extend(partial.iter());
            assert_eq!(
                refused(&tokens).code(),
                "invalid_document_registration",
                "{partial:?}"
            );
        }
        let (asset_id, patch, _) = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--document-number",
            "GTP-001",
            "--document-revision",
            "B",
            "--document-state",
            "issued",
        ]))
        .expect("valid");
        assert_eq!(asset_id, "a_7kq3nr2v0b1c");
        assert_eq!(
            patch["document"],
            json!({ "number": "GTP-001", "revision_label": "B", "state": "issued" })
        );
        let tokens: Vec<String> = [
            "--asset",
            "a_7kq3nr2v0b1c",
            "--document-number",
            "x",
            "--document-revision",
            "1",
            "--document-state",
            "signed",
        ]
        .iter()
        .map(|token| (*token).to_string())
        .collect();
        assert_eq!(
            ds_cli_contract::parse(&COMMAND, &tokens)
                .expect_err("closed vocabulary")
                .code(),
            "invalid_choice"
        );
    }

    #[test]
    fn an_audit_reason_is_a_sentence_and_the_owner_is_a_user() {
        let long = "x".repeat(MAX_REASON_CHARS + 1);
        for bad in ["", "   ", long.as_str()] {
            assert_eq!(
                refused(&[
                    "--asset",
                    "a_7kq3nr2v0b1c",
                    "--status",
                    "durable",
                    "--reason",
                    bad
                ])
                .code(),
                "invalid_reason"
            );
        }
        let (_, patch, _) = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--status",
            "durable",
            "--owner",
            " lead@example.com ",
            "--reason",
            "  signed copy received  ",
        ]))
        .expect("valid");
        assert_eq!(patch["reason"], json!("signed copy received"));
        assert_eq!(patch["owner"]["id"], json!("lead@example.com"));
    }

    #[test]
    fn the_status_vocabulary_is_closed_at_the_parser() {
        let tokens = ["--asset", "a_7kq3nr2v0b1c", "--status", "settled"]
            .iter()
            .map(|token| (*token).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            ds_cli_contract::parse(&COMMAND, &tokens)
                .expect_err("invalid status")
                .code(),
            "invalid_choice"
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        let (asset, payload, folder) = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--kind",
            "doc",
            "--status",
            "durable",
            "--owner",
            "commercial@example.com",
            "--folder",
            "contracts/2026/epc",
            "--sensitivity",
            "confidential",
            "--reason",
            "signed copy received",
        ]))
        .expect("valid");
        assert_eq!(asset, "a_7kq3nr2v0b1c");
        assert_eq!(folder.as_deref(), Some("contracts/2026/epc"));
        let mut keys: Vec<&str> = payload.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "folder_id",
                "kind",
                "owner",
                "reason",
                "sensitivity",
                "status"
            ]
        );
        // A flag nobody gave is absent, not null: the application's own
        // default is the answer to a question that was not asked.
        let (_, minimal, _) = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--status",
            "archive",
        ]))
        .expect("valid");
        assert_eq!(minimal.keys().len(), 1, "an omitted flag must not travel");
    }

    #[test]
    fn the_human_projection_reports_the_fields_that_moved_and_the_audit_row() {
        let rendered = render(&json!({
            "asset": { "asset_id": "a_7kq3nr2v0b1c", "name": "EPC Lot 3.pdf", "folder": "contracts",
                       "kind": "doc", "status": "durable", "sensitivity": "confidential", "bytes": 12 },
            "audit_id": "aud_9",
            "changed": ["status", "sensitivity"],
            "warnings": [{ "message": "two links now point at a hidden document" }]
        }));
        assert!(rendered.contains("classified a_7kq3nr2v0b1c · status, sensitivity"));
        assert!(rendered.contains("audit aud_9"));
        assert!(rendered.contains("contracts/EPC Lot 3.pdf"));
        assert!(rendered.contains("! two links now point at a hidden document"));
        // Nothing moved is said, not hidden.
        assert!(render(&json!({ "changed": [] })).contains("no field moved"));
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
            "--status",
            "durable"
        ])));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.kind != ArgKind::Positional)
        );
    }
}
