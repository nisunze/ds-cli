//! `ds assets folder` — declare a folder, or change its defaults or name.
//!
//! A folder here is a document, not a path derived from the assets inside it
//! (D3). That is what lets it exist while empty, carry a sensitivity its new
//! children inherit, and be renamed without touching a single asset.
//!
//! The other half of the tree is not declared at all: the system roots below
//! are projected at read time from inventories the project already holds
//! (§2.5, D15). Naming one here is refused locally and by name, because a
//! declaration under a projected root would be a second folder authority —
//! the one thing §12.12 forbids — and it would vanish on the next refresh.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const PATH_ARG: Arg = Arg::value(
    "path",
    "<path>",
    "The folder's path, e.g. contracts/2026/epc; created when it does not exist.",
)
.required();

const SENSITIVITY_ARG: Arg = Arg::value(
    "sensitivity",
    "<class>",
    "Default class new children inherit; loosening needs the leaving class's capability.",
)
.choices(crate::SENSITIVITIES);

const STATUS_ARG: Arg =
    Arg::value("status", "<status>", "Default status new children take.").choices(crate::STATUSES);

const RENAME_TO_ARG: Arg = Arg::value(
    "rename-to",
    "<name>",
    "New name for the last path segment; children keep their ids.",
);

/// The roots the read-time projection owns (§2.5).
///
/// This is the domain's one copy of the list: `classify` refuses the same set
/// for the same reason, and reads it from here rather than keeping a second
/// one that could drift.
pub const SYSTEM_FOLDER_ROOTS: &[&str] = &[
    "Transformers",
    "MV models",
    "Survey",
    "Project data",
    "My data",
    "Project work",
    "Reports",
    "Solar",
    "Local data",
    "Prints",
    "Unclassified",
];

/// The system root a folder path falls under, if any.
///
/// Matched without case, because a caller who types `transformers/…` means the
/// root they saw in `ds assets tree` and deserves the refusal that names it —
/// not a second, near-identical folder declared beside a projected one.
pub fn system_root(path: &str) -> Option<&'static str> {
    let head = path.split('/').next().unwrap_or_default();
    SYSTEM_FOLDER_ROOTS
        .iter()
        .copied()
        .find(|root| root.eq_ignore_ascii_case(head))
}

/// The refusal a system folder earns, wherever it was named.
pub fn system_folder_refusal(flag: &str, path: &str, root: &str) -> Failure {
    let where_it_is = if path.eq_ignore_ascii_case(root) {
        format!("names the system folder `{root}`")
    } else {
        format!("names `{path}`, inside the system folder `{root}`")
    };
    Failure::invalid(
        "projected_asset_read_only",
        format!(
            "`--{flag}` {where_it_is}, which the application projects from what the project already holds rather than stores"
        ),
    )
    .remedy(crate::PROJECTED_ASSET_READ_ONLY.remedy)
    .detail(json!({ "path": path, "system_root": root }))
}

pub static COMMAND: Command = Command {
    id: "assets.folder",
    path: &["assets", "folder"],
    contract: 1,
    summary: "Declare a folder, or change its defaults or name.",
    purpose: "\
Creates the declared folder at --path when it does not exist, or updates it \
when it does: a default sensitivity and status its new children inherit, or a \
new name. A folder is a document, not a path derived from its children, so it \
can be empty and can be renamed without touching them. Loosening a folder \
default needs the same capability as loosening an asset. System folders are \
projected, not declared, and are refused by name.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        PATH_ARG,
        SENSITIVITY_ARG,
        STATUS_ARG,
        RENAME_TO_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`folder` — the declared folder with `folder_id`, `path`, `name`, `parent`, \
`default_sensitivity`, `default_status` and `counts`.",
    examples: &[Example {
        command: "ds assets folder --path contracts/2026/epc --sensitivity confidential --yes",
        note: "Every asset ingested into it afterwards is confidential unless a stricter class is named.",
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
        crate::INVALID_FOLDER_PATH,
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
        crate::UNKNOWN_FOLDER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The declaration, validated locally, in the exact keys the operation
/// declares.
///
/// There is no `nothing_to_update` here and there must not be: `--path` alone
/// is the whole of a create, which is what this command does by default. A
/// caller who names an existing folder and no change has declared a folder
/// that already exists, and the idempotent answer is the folder.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let path = crate::folder_path(inputs.require("path")?, "path")?;
    if let Some(root) = system_root(&path) {
        return Err(system_folder_refusal("path", &path, root));
    }

    let mut arguments = Map::new();
    arguments.insert("path".into(), json!(path));
    // Both vocabularies are closed at the parser; an absent flag leaves the
    // folder's existing default alone rather than resetting it.
    for flag in ["sensitivity", "status"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    if let Some(rename) = inputs.value("rename-to") {
        arguments.insert("rename_to".into(), json!(new_name(rename)?));
    }

    Ok(Value::Object(arguments))
}

/// `--rename-to` names the last segment only.
///
/// A rename moves a name; the children keep their ids and their folder. A
/// path here would quietly be a move instead, so it is refused rather than
/// interpreted — and so is an empty name, which would leave the folder with
/// no name at all.
fn new_name(raw: &str) -> Result<String, Failure> {
    let name = crate::folder_path(raw, "rename-to")?;
    if name.contains('/') {
        return Err(Failure::invalid(
            "invalid_folder_path",
            "`--rename-to` is one folder name, not a path",
        )
        .remedy("pass the folder's new name only, e.g. --rename-to epc-2026")
        .detail(json!({ "given": raw })));
    }
    Ok(name)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_FOLDER,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    let folder = &data["folder"];
    let mut out = format!(
        "{}  {}\n",
        folder["path"].as_str().unwrap_or("?"),
        folder["folder_id"].as_str().unwrap_or("—"),
    );
    out.push_str(&format!(
        "  new children: {} · {}\n",
        folder["default_sensitivity"].as_str().unwrap_or("—"),
        folder["default_status"].as_str().unwrap_or("—"),
    ));
    if let Some(counts) = folder.get("counts").filter(|counts| !counts.is_null()) {
        out.push_str(&format!("  counts: {counts}\n"));
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
    fn every_projected_root_is_refused_by_name_before_the_bridge() {
        for root in SYSTEM_FOLDER_ROOTS {
            let failure = refused(&["--path", root]);
            assert_eq!(
                failure.code(),
                "projected_asset_read_only",
                "`{root}` was accepted as a declarable folder"
            );
            assert!(failure.message().contains(root));
        }
        // Nested, and typed in another case, is the same root.
        assert_eq!(
            refused(&["--path", "Transformers/AGASHARU/reports"]).code(),
            "projected_asset_read_only"
        );
        assert_eq!(
            refused(&["--path", "mv models/Feeder 3"]).code(),
            "projected_asset_read_only"
        );
        // A user folder that merely starts with the same letters is not one.
        assert!(arguments(&parse(&["--path", "Reports 2026"])).is_ok());
        assert!(arguments(&parse(&["--path", "contracts/Reports"])).is_ok());
    }

    #[test]
    fn a_create_needs_nothing_but_a_path() {
        // `create` is what this command does by default, so there is no
        // `nothing_to_update` here: --path alone is a complete invocation.
        let payload = arguments(&parse(&["--path", "contracts/2026/epc"])).expect("valid");
        assert_eq!(payload, json!({ "path": "contracts/2026/epc" }));
    }

    #[test]
    fn a_rename_names_one_segment_and_is_never_empty() {
        for bad in ["", "   ", "epc/2026", "..", "/epc"] {
            assert_eq!(
                refused(&["--path", "contracts/2026/epc", "--rename-to", bad]).code(),
                "invalid_folder_path",
                "`{bad}` was accepted as a new folder name"
            );
        }
        let payload = arguments(&parse(&[
            "--path",
            "contracts/2026/epc",
            "--rename-to",
            " epc-2026 ",
        ]))
        .expect("valid");
        assert_eq!(payload["rename_to"], json!("epc-2026"));
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        let payload = arguments(&parse(&[
            "--path",
            "contracts/2026/epc",
            "--sensitivity",
            "confidential",
            "--status",
            "durable",
            "--rename-to",
            "epc-2026",
        ]))
        .expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_FOLDER, &payload), None);
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["path", "rename_to", "sensitivity", "status"]);
    }

    #[test]
    fn the_human_projection_reports_the_defaults_children_will_inherit() {
        let rendered = render(&json!({
            "folder": {
                "folder_id": "f_12", "path": "contracts/2026/epc", "name": "epc",
                "default_sensitivity": "confidential", "default_status": "fresh",
                "counts": { "assets": 4 }
            }
        }));
        assert!(rendered.contains("contracts/2026/epc  f_12"));
        assert!(rendered.contains("new children: confidential · fresh"));
        assert!(rendered.contains("counts:"));
    }

    #[test]
    fn this_write_cannot_be_reached_without_explicit_confirmation() {
        // The gate itself lives once, in `ds`'s dispatch, and reads exactly
        // this declaration — so the declaration is the part a test inside
        // this crate can hold. A confirmation-gated command may also take no
        // positional argument, because `--yes` must never be the thing that
        // shifts what an operand means.
        assert!(COMMAND.effect.needs_confirmation());
        assert!(COMMAND.confirmation_required_for(&parse(&["--path", "contracts/2026/epc"])));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.kind != ArgKind::Positional)
        );
    }
}
