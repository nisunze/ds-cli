//! `ds assets classify` — change one asset's kind, status, owner, folder or
//! sensitivity, audited.
//!
//! One invocation is one audited change. Every flag given travels as a single
//! patch, and a flag left out is a field left alone rather than a field
//! cleared — the same shape `ds work task update` has, for the same reason: a
//! caller changing a status must not have to restate a classification it does
//! not know.
//!
//! Two things are refused here rather than after a round trip. An invocation
//! that names no change would still cost an audit row for nothing, and a
//! projected `sys:` row has no stored document to patch at all (D18) — the
//! thing to classify is the object it projects.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, DESCRIPTOR_ARG, FOLDER_ARG};

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

/// The longest `--reason` one audit row carries.
///
/// A reason is read by a person reconciling an access change months later, so
/// it is a sentence rather than a document; anything longer is refused here
/// instead of being silently cut on the way to the audit sink.
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
const CHANGE_KEYS: &[&str] = &["kind", "status", "owner", "folder", "sensitivity"];

pub static COMMAND: Command = Command {
    id: "assets.classify",
    path: &["assets", "classify"],
    contract: 1,
    summary: "Change one asset's kind, status, owner, folder or sensitivity.",
    purpose: "\
Applies every flag given as one audited catalogue change through ds-brain, \
which decides whether the caller may make it: loosening sensitivity needs the \
capability for the class being left, and a person's override is never \
re-inferred away. Nothing given, nothing sent — a flag you omit is untouched. \
Projected sys: rows are read-only in this slice and refused by name. Refused \
offline, because a queued access change is a queued exposure.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        ASSET_ARG,
        KIND_ARG,
        STATUS_ARG,
        OWNER_ARG,
        FOLDER_ARG,
        SENSITIVITY_ARG,
        REASON_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`asset` — the row after the change — the `audit_id` recorded for it, and \
`changed`: the names of the fields that actually moved.",
    examples: &[Example {
        command: "ds assets classify --asset a_7kq3nr2v0b1c --status durable --sensitivity restricted --reason signed-copy --yes",
        note: "Status and class land together or not at all; the audit row names the reason.",
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
        crate::INVALID_FOLDER_PATH,
        INVALID_REASON,
        crate::PROJECTED_ASSET_READ_ONLY,
        crate::NOTHING_TO_UPDATE,
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
        crate::UNKNOWN_FOLDER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The patch, validated locally, in the exact keys the operation declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let asset = crate::asset_id(inputs.require("asset")?, "asset")?;
    if crate::is_projected(&asset) {
        return Err(Failure::invalid(
            "projected_asset_read_only",
            format!("`{asset}` is a projected system row, which has no stored document to change"),
        )
        .remedy(crate::PROJECTED_ASSET_READ_ONLY.remedy)
        .detail(json!({ "asset": asset })));
    }

    let mut arguments = Map::new();
    arguments.insert("asset".into(), json!(asset));

    // kind, status and sensitivity are closed at the parser, so a value that
    // reaches here is one of the contract's own words and needs no second
    // vocabulary check.
    for flag in ["kind", "status", "sensitivity"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    if let Some(owner) = inputs.value("owner") {
        arguments.insert("owner".into(), json!(owner.trim()));
    }
    if let Some(folder) = inputs.value("folder") {
        let folder = crate::folder_path(folder, "folder")?;
        if let Some(root) = crate::folder::system_root(&folder) {
            return Err(crate::folder::system_folder_refusal(
                "folder", &folder, root,
            ));
        }
        arguments.insert("folder".into(), json!(folder));
    }
    if let Some(reason) = inputs.value("reason") {
        arguments.insert("reason".into(), json!(audit_reason(reason)?));
    }

    // `--asset` alone, or with only a reason, is a write that writes nothing:
    // one audit row, one round trip and no change. It costs a local refusal
    // instead.
    if !CHANGE_KEYS.iter().any(|key| arguments.contains_key(*key)) {
        return Err(Failure::invalid(
            "nothing_to_update",
            "no kind, status, owner, folder or sensitivity flag was given",
        )
        .remedy(crate::NOTHING_TO_UPDATE.remedy)
        .next("ds assets classify --help"));
    }

    Ok(Value::Object(arguments))
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
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_CLASSIFY,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
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
    out.push_str(&warnings(data));
    out
}

/// The application's own warnings, under the headline.
///
/// A warning is not a refusal: the change applied. But a classification that
/// widened what a folder's children inherit, or left a link pointing at
/// something the new class hides, is exactly what a caller running unattended
/// needs told — so no write here renders without passing them through.
fn warnings(data: &Value) -> String {
    let mut out = String::new();
    for warning in data["warnings"].as_array().into_iter().flatten() {
        let text = warning
            .as_str()
            .or_else(|| warning["message"].as_str())
            .or_else(|| warning["code"].as_str())
            .unwrap_or("the application returned a warning with no message");
        out.push_str(&format!("  ! {text}\n"));
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
    fn a_patch_that_changes_nothing_never_reaches_the_project() {
        // A write that writes nothing still costs an audit row and a round
        // trip. `--reason` alone is the interesting case: it is a real flag
        // that changes no field.
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
    fn a_projected_row_is_refused_by_name_before_the_bridge() {
        let failure = refused(&[
            "--asset",
            "sys:design_attachment:att_1:rev_2",
            "--status",
            "durable",
        ]);
        assert_eq!(failure.code(), "projected_asset_read_only");
        assert!(
            failure
                .remedy_text()
                .expect("remedy")
                .contains("act on the source object")
        );
    }

    #[test]
    fn a_system_folder_is_not_a_classification_target() {
        // System folders are projected from the project's own inventories, so
        // filing a document into one would be filing it into something that
        // is rebuilt from elsewhere on the next refresh.
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
        assert!(
            arguments(&parse(&[
                "--asset",
                "a_7kq3nr2v0b1c",
                "--folder",
                "contracts/2026/epc"
            ]))
            .is_ok()
        );
    }

    #[test]
    fn an_audit_reason_is_a_sentence_and_is_never_cut_silently() {
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
        let payload = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--status",
            "durable",
            "--reason",
            "  signed copy received  ",
        ]))
        .expect("valid");
        assert_eq!(payload["reason"], json!("signed copy received"));
    }

    #[test]
    fn the_vocabularies_are_closed_at_the_parser() {
        let tokens: Vec<String> = ["--asset", "a_7kq3nr2v0b1c", "--status", "settled"]
            .iter()
            .map(|token| (*token).to_string())
            .collect();
        assert_eq!(
            ds_cli_contract::parse(&COMMAND, &tokens)
                .expect_err("must refuse")
                .code(),
            "invalid_choice"
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        let payload = arguments(&parse(&[
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
        assert_eq!(undeclared_key(&crate::ASSETS_CLASSIFY, &payload), None);
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
                "asset",
                "folder",
                "kind",
                "owner",
                "reason",
                "sensitivity",
                "status"
            ]
        );
        // A flag nobody gave is absent, not null: the application's own
        // default is the answer to a question that was not asked.
        let minimal = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--status",
            "archive",
        ]))
        .expect("valid");
        assert_eq!(
            minimal.as_object().expect("object").keys().len(),
            2,
            "an omitted flag must not travel"
        );
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
