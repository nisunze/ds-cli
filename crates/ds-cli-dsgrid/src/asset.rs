//! `ds dsgrid asset` and `ds dsgrid project asset` — the files a `.dsgrid`
//! carries beside its tables.
//!
//! A package holds content-addressed assets: v1's original PLS-CADD upload
//! (`pls-original-workspace.bak`), the round-trip backup, native resource
//! files, evidence registries, and attachments such as the backup delivered
//! for a submitted version. These commands list them, save one to a file, and
//! write a new package with one attachment added, replaced or removed.
//!
//! Every answer comes from `ds_grid_exchange::package_assets`: which leaves
//! are protected, whether an attachment is stale, whether a PLS-CADD backup
//! is characterised, and the proof that a repack changed nothing else. This
//! module reads files, reaches the project owner for an exact revision, maps
//! refusals and writes new files that never overwrite anything.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::package_assets::{
    self, AssetError, AttachRequest, DetachRequest, PackageAssetEntry,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::package::{self, DEFAULT_LIMIT, MAX_PACKAGE_BYTES, parse_limit, take};

// ── Refusals ──────────────────────────────────────────────────────────────

const ASSET_NOT_FOUND: Refusal = Refusal {
    code: "asset_not_found",
    when: "the package carries no asset under --leaf, or --replace names no attachment",
    remedy: "list the package's assets and use a listed leaf; omit --replace for a new attachment",
};
const ASSET_LEAF_AMBIGUOUS: Refusal = Refusal {
    code: "asset_leaf_ambiguous",
    when: "two different payloads carry --leaf",
    remedy: "the package is inconsistent; re-convert it so each leaf is carried once",
};
const ASSET_LEAF_INVALID: Refusal = Refusal {
    code: "asset_leaf_invalid",
    when: "--leaf is not a portable file name",
    remedy: "pass a bare file name without / \\ : * ? \" < > |",
};
const ASSET_LEAF_PROTECTED: Refusal = Refusal {
    code: "asset_leaf_protected",
    when: "--leaf belongs to the model: a PLS-CADD source backup, a resource, a registry, or an asset no attachment record claims",
    remedy: "choose a new leaf, e.g. pls-delivered-workspace.bak; model-owned assets are never replaced",
};
const ASSET_LEAF_COLLIDES: Refusal = Refusal {
    code: "asset_leaf_collides",
    when: "--leaf ends in pls-source-workspace.bak, pls-original-workspace.bak or pls-source-baseline.sha256",
    remedy: "choose a leaf that does not end in a reserved PLS-CADD leaf",
};
const ASSET_LEAF_EXISTS: Refusal = Refusal {
    code: "asset_leaf_exists",
    when: "an attachment already uses --leaf",
    remedy: "pass --replace to supersede it, or choose another leaf",
};
const ASSET_NOT_ATTACHMENT: Refusal = Refusal {
    code: "asset_not_attachment",
    when: "--leaf is carried but is not a version attachment",
    remedy: "only attachments detach; the asset list marks them protected: false",
};
const ASSET_ROLE_INVALID: Refusal = Refusal {
    code: "asset_role_invalid",
    when: "--role is malformed, names a model-owned asset, or does not pair with --leaf",
    remedy: "use a lowercase token; pls-delivered-workspace.bak takes pls_cadd_delivered_workspace",
};
const ASSET_FILE_INVALID: Refusal = Refusal {
    code: "asset_file_invalid",
    when: "--file is missing, empty, unreadable or above 512 MiB",
    remedy: "pass the exact file to attach",
};
const ASSET_PLS_INVALID: Refusal = Refusal {
    code: "asset_pls_invalid",
    when: "a pls_cadd_* role's --file is not a PLS-CADD backup whose DON members resolve to one characterised version",
    remedy: "attach a .bak PLS-CADD saved; `ds dsgrid-exchange inspect` names its version",
};
const PACKAGE_DIGEST_MISMATCH: Refusal = Refusal {
    code: "asset_package_digest_mismatch",
    when: "the package's SHA-256 differs from --expected-sha256",
    remedy: "list the package you meant again and pass its current digest",
};
const PACKAGE_INVALID: Refusal = Refusal {
    code: "asset_package_invalid",
    when: "the package fails verification, needs library releases, or carries an invalid registry",
    remedy: "run `ds dsgrid validate`; library-pinned packages are not edited here",
};
const PACKAGE_UNSUPPORTED: Refusal = Refusal {
    code: "asset_package_unsupported",
    when: "the package carries derived/ or history/ members a repack would drop",
    remedy: "re-export the package without them, then attach",
};
const CONTENT_CHANGED: Refusal = Refusal {
    code: "asset_content_changed",
    when: "the repacked package did not reproduce the model exactly",
    remedy: "nothing was written; report this with the package digest",
};
const NOT_A_PACKAGE: Refusal = Refusal {
    code: "not_a_dsgrid_package",
    when: "the bytes are not a readable .dsgrid container",
    remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
};
const MANIFEST_UNREADABLE: Refusal = Refusal {
    code: "manifest_unreadable",
    when: "the package manifest does not match this build's schema",
    remedy: "rebuild the package with a matching ds-network release",
};
const INVALID_LIMIT: Refusal = Refusal {
    code: "invalid_limit",
    when: "--limit is not a whole number in 1..5000",
    remedy: "pass a limit inside the range, or omit it for the default of 50",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "output_exists",
    when: "--out already exists",
    remedy: "choose a new path; asset commands never overwrite a file",
};
const OUTPUT_UNWRITABLE: Refusal = Refusal {
    code: "output_unwritable",
    when: "--out cannot be created or fully written",
    remedy: "choose a new writable path; a partial file is removed",
};

/// Splice refusal lists at compile time so each vocabulary is written once.
const fn join<const N: usize>(parts: &[&[Refusal]]) -> [Refusal; N] {
    let mut out = [NOT_A_PACKAGE; N];
    let mut written = 0;
    let mut part = 0;
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            out[written] = parts[part][index];
            written += 1;
            index += 1;
        }
        part += 1;
    }
    assert!(written == N, "refusal list length");
    out
}

/// Reading a local package: the domain's shared vocabulary (which already
/// names `not_a_dsgrid_package`).
const LOCAL: &[Refusal] = package::SHARED_REFUSALS;
/// Reaching the project owner for an exact revision.
const NATIVE: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const READ: &[Refusal] = &[ASSET_NOT_FOUND, ASSET_LEAF_AMBIGUOUS, ASSET_LEAF_INVALID];
const WRITE: &[Refusal] = &[OUTPUT_EXISTS, OUTPUT_UNWRITABLE];
const EDIT: &[Refusal] = &[
    PACKAGE_DIGEST_MISMATCH,
    PACKAGE_INVALID,
    PACKAGE_UNSUPPORTED,
    ASSET_LEAF_PROTECTED,
    ASSET_LEAF_COLLIDES,
    ASSET_NOT_FOUND,
    ASSET_LEAF_INVALID,
    CONTENT_CHANGED,
];
const ATTACH_ONLY: &[Refusal] = &[
    ASSET_LEAF_EXISTS,
    ASSET_ROLE_INVALID,
    ASSET_FILE_INVALID,
    ASSET_PLS_INVALID,
];

/// Listing reads the manifest, the registries and the resources table.
const LISTING: &[Refusal] = &[
    INVALID_LIMIT,
    PACKAGE_INVALID,
    ASSET_LEAF_AMBIGUOUS,
    MANIFEST_UNREADABLE,
];

const LIST_REFUSALS: [Refusal; LOCAL.len() + LISTING.len()] = join(&[LOCAL, LISTING]);
const EXTRACT_REFUSALS: [Refusal; LOCAL.len() + READ.len() + WRITE.len() + 1] =
    join(&[LOCAL, READ, WRITE, &[PACKAGE_INVALID]]);
const ATTACH_REFUSALS: [Refusal; LOCAL.len() + EDIT.len() + ATTACH_ONLY.len() + WRITE.len()] =
    join(&[LOCAL, EDIT, ATTACH_ONLY, WRITE]);
const DETACH_REFUSALS: [Refusal; LOCAL.len() + EDIT.len() + WRITE.len() + 1] =
    join(&[LOCAL, EDIT, WRITE, &[ASSET_NOT_ATTACHMENT]]);
const PROJECT_LIST_REFUSALS: [Refusal; NATIVE.len() + LISTING.len() + 1] =
    join(&[NATIVE, LISTING, &[NOT_A_PACKAGE]]);
const PROJECT_EXTRACT_REFUSALS: [Refusal; NATIVE.len() + READ.len() + WRITE.len() + 2] =
    join(&[NATIVE, READ, WRITE, &[NOT_A_PACKAGE, PACKAGE_INVALID]]);

// ── Declared inputs ───────────────────────────────────────────────────────

const PATH: Arg = Arg::value(
    "path",
    "<file.dsgrid>",
    "The local .dsgrid package to read.",
)
.required();
const LEAF: Arg = Arg::value(
    "leaf",
    "<leaf>",
    "Exact asset file name, e.g. pls-original-workspace.bak.",
)
.required();
const OUT_FILE: Arg =
    Arg::value("out", "<file>", "New file to write; never overwritten.").required();
const OUT_PACKAGE: Arg = Arg::value(
    "out",
    "<new.dsgrid>",
    "New package to write; never overwritten.",
)
.required();
const EXPECTED_SHA256: Arg = Arg::value(
    "expected-sha256",
    "<sha256>",
    "SHA-256 of --path you inspected; a changed package refuses.",
);
const LIMIT: Arg = Arg::value("limit", "<n>", "Cap the listed assets.").default(DEFAULT_LIMIT);
const PROJECT: Arg =
    Arg::value("project", "<ds-project>", "Exact project for this request.").required();
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const MODEL: Arg = Arg::value("model", "<id>", "Exact project model ID.").required();
const REVISION: Arg = Arg::value(
    "revision",
    "<id>",
    "Exact immutable revision ID from dsgrid project versions.",
)
.required();

// ── Commands ──────────────────────────────────────────────────────────────

pub static LIST: Command = Command {
    id: "dsgrid.asset.list",
    path: &["dsgrid", "asset", "list"],
    contract: 1,
    summary: "List the files a .dsgrid carries: source .bak, evidence, attachments.",
    purpose: "Find the PLS-CADD backups and other files inside a local model package before extracting one. Names each asset's leaf, SHA-256, size, owner role and whether it is protected; v1's original upload is pls-original-workspace.bak. An attachment such as a delivered backup is bound to the snapshot it was attached to, and stale says a later revision inherited it. Reads the manifest only.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PATH, LIMIT],
    output: "Model identity, package SHA-256, asset counts, bounded assets (leaf, sha256, byte_len, role, protected, attachment binding) and more when truncated.",
    examples: &[Example {
        command: "ds dsgrid asset list --path ./model.dsgrid",
        note: "Every asset with its owner; the original .bak is protected.",
        runnable: false,
    }],
    refusals: &LIST_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["package assets", "original bak", "embedded backup"],
    requires: Requires::Server,
    availability: available,
};

pub static EXTRACT: Command = Command {
    id: "dsgrid.asset.extract",
    path: &["dsgrid", "asset", "extract"],
    contract: 1,
    summary: "Save one file a local .dsgrid carries, e.g. its original .bak.",
    purpose: "Recover the exact bytes of one package asset, such as v1's original upload pls-original-workspace.bak or a delivered backup, to open in PLS-CADD or hand over. Verifies them against the manifest's SHA-256 and size before writing a new file. A leaf carried by two payloads is refused, never guessed.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PATH, LEAF, OUT_FILE],
    output: "Leaf, role, verified SHA-256 and byte count, and the new file.",
    examples: &[Example {
        command: "ds dsgrid asset extract --path ./model.dsgrid --leaf pls-original-workspace.bak --out ./v1-original.bak",
        note: "The exact incoming PLS-CADD backup.",
        runnable: false,
    }],
    refusals: &EXTRACT_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["extract bak", "original workspace", "save backup"],
    requires: Requires::Server,
    availability: available,
};

pub static ATTACH: Command = Command {
    id: "dsgrid.asset.attach",
    path: &["dsgrid", "asset", "attach"],
    contract: 1,
    summary: "Attach a delivered PLS-CADD .bak to a new copy of a .dsgrid.",
    purpose: "Carry the backup delivered for a version inside the model package, bound to the snapshot it describes, before publishing that version. Writes a new package; the model, bindings and every other asset are proven unchanged. The delivered backup is pls-delivered-workspace.bak with role pls_cadd_delivered_workspace, and any pls_cadd_* role must be a characterised PLS-CADD backup. Protected leaves are refused; --replace supersedes an attachment. A version already published cannot gain package assets: attach its delivery with design attachment publish.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        PATH,
        LEAF,
        Arg::value("file", "<file>", "The file to attach, at most 512 MiB.").required(),
        Arg::value(
            "role",
            "<role>",
            "What the file is, e.g. pls_cadd_delivered_workspace.",
        )
        .required(),
        Arg::switch("replace", "Supersede the attachment already under --leaf."),
        OUT_PACKAGE,
        EXPECTED_SHA256,
    ],
    output: "Attach receipt: input and result package identities, the attached and any replaced payload, the attachment record, PLS-CADD version evidence, and the new package.",
    examples: &[Example {
        command: "ds dsgrid asset attach --path ./v2.dsgrid --leaf pls-delivered-workspace.bak --file ./submitted.bak --role pls_cadd_delivered_workspace --out ./v2-delivered.dsgrid",
        note: "The submitted backup, bound to v2's snapshot.",
        runnable: false,
    }],
    refusals: &ATTACH_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["attach bak", "delivered workspace", "submission backup"],
    requires: Requires::Server,
    availability: available,
};

pub static DETACH: Command = Command {
    id: "dsgrid.asset.detach",
    path: &["dsgrid", "asset", "detach"],
    contract: 1,
    summary: "Remove one attachment from a new copy of a .dsgrid.",
    purpose: "Drop an attachment that no longer belongs with a model, such as a delivered backup inherited from an earlier revision. Writes a new package without it and its record; the model and every other asset are proven unchanged. Model-owned assets, including the original and round-trip PLS-CADD backups, are refused.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PATH, LEAF, OUT_PACKAGE, EXPECTED_SHA256],
    output: "Detach receipt: input and result package identities, the removed payload and its bound snapshot, and the new package.",
    examples: &[],
    refusals: &DETACH_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["remove attachment", "stale delivery"],
    requires: Requires::Server,
    availability: available,
};

pub static PROJECT_LIST: Command = Command {
    id: "dsgrid.project.asset.list",
    path: &["dsgrid", "project", "asset", "list"],
    contract: 1,
    summary: "List the files a project model version carries, e.g. its .bak.",
    purpose: "See what an exact governed revision carries, such as v1's original PLS-CADD upload, without a Desktop. Downloads that revision headlessly, verifies its SHA-256 and size, and lists its assets as dsgrid asset list does. Nothing is written to disk.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, LANE, MODEL, REVISION, LIMIT],
    output: "Project, model and revision, verified package SHA-256, asset counts, bounded assets and more when truncated.",
    examples: &[],
    refusals: &PROJECT_LIST_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["version assets", "original bak"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static PROJECT_EXTRACT: Command = Command {
    id: "dsgrid.project.asset.extract",
    path: &["dsgrid", "project", "asset", "extract"],
    contract: 1,
    summary: "Save one file from a project model version, e.g. v1's original .bak.",
    purpose: "Recover the exact PLS-CADD backup or other asset of a governed revision without a Desktop, for example the incoming v1 pls-original-workspace.bak. Downloads and verifies the revision, then verifies the asset against its manifest SHA-256 and size before writing a new file.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, LANE, MODEL, REVISION, LEAF, OUT_FILE],
    output: "Project, model and revision, verified package and asset SHA-256, byte count and the new file.",
    examples: &[],
    refusals: &PROJECT_EXTRACT_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["download original bak", "version backup"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// Always available: the exchange owner is linked into this binary.
fn available() -> Availability {
    Availability::Available
}

// ── Handlers ──────────────────────────────────────────────────────────────

pub fn list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("path")?;
    let limit = parse_limit(inputs.value("limit"))?;
    let bytes = read_package(path)?;
    let mut answer = listing(&bytes, limit)?;
    answer.insert("path".into(), json!(path));
    Ok(Value::Object(answer))
}

pub fn extract(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("path")?;
    let bytes = read_package(path)?;
    let mut answer = extracted(&bytes, inputs.require("leaf")?, inputs.require("out")?)?;
    answer.insert("path".into(), json!(path));
    Ok(Value::Object(answer))
}

pub fn attach(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("path")?;
    let out = inputs.require("out")?;
    let package_bytes = read_package(path)?;
    let file = inputs.require("file")?;
    let bytes = read_attachment(file)?;
    let expected = expected_digest(inputs, &package_bytes);
    let output = package_assets::attach_package_asset(&AttachRequest {
        package_bytes: &package_bytes,
        expected_package_sha256: &expected,
        leaf: inputs.require("leaf")?,
        bytes: &bytes,
        role: inputs.require("role")?,
        replace: inputs.switch("replace"),
        libraries: &[],
    })
    .map_err(refusal)?;
    crate::apply::write_new(out, &output.package_bytes)?;
    let mut answer = receipt(&output.receipt)?;
    answer.insert("path".into(), json!(path));
    answer.insert("file".into(), json!(file));
    answer.insert("out".into(), json!(out));
    Ok(Value::Object(answer))
}

pub fn detach(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("path")?;
    let out = inputs.require("out")?;
    let package_bytes = read_package(path)?;
    let expected = expected_digest(inputs, &package_bytes);
    let output = package_assets::detach_package_asset(&DetachRequest {
        package_bytes: &package_bytes,
        expected_package_sha256: &expected,
        leaf: inputs.require("leaf")?,
        libraries: &[],
    })
    .map_err(refusal)?;
    crate::apply::write_new(out, &output.package_bytes)?;
    let mut answer = receipt(&output.receipt)?;
    answer.insert("path".into(), json!(path));
    answer.insert("out".into(), json!(out));
    Ok(Value::Object(answer))
}

pub fn project_list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = parse_limit(inputs.value("limit"))?;
    let (source, bytes) = download(inputs)?;
    let mut answer = listing(&bytes, limit)?;
    answer.insert("source".into(), source);
    Ok(Value::Object(answer))
}

pub fn project_extract(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let leaf = inputs.require("leaf")?;
    let out = inputs.require("out")?;
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(
            Failure::conflict(OUTPUT_EXISTS.code, format!("`{out}` already exists"))
                .remedy(OUTPUT_EXISTS.remedy),
        );
    }
    let (source, bytes) = download(inputs)?;
    let mut answer = extracted(&bytes, leaf, out)?;
    answer.insert("source".into(), source);
    Ok(Value::Object(answer))
}

// ── Shared plumbing ───────────────────────────────────────────────────────

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_package(path: &str) -> Result<Vec<u8>, Failure> {
    package::read_bytes(path).map_err(|failure| {
        if failure.code() == "model_not_found" {
            failure.remedy("check --path; it takes a .dsgrid file")
        } else {
            failure
        }
    })
}

fn read_attachment(path: &str) -> Result<Vec<u8>, Failure> {
    let invalid = |message: String| {
        Failure::invalid(ASSET_FILE_INVALID.code, message).remedy(ASSET_FILE_INVALID.remedy)
    };
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("cannot read `{path}`: {}", error.kind())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PACKAGE_BYTES {
        return Err(invalid(format!(
            "`{path}` is not a non-empty file of at most 512 MiB"
        )));
    }
    std::fs::read(path).map_err(|error| invalid(format!("cannot read `{path}`: {}", error.kind())))
}

/// The digest the caller pinned, or the digest of the bytes just read.
fn expected_digest(inputs: &Inputs, package_bytes: &[u8]) -> String {
    inputs
        .value("expected-sha256")
        .map(str::to_owned)
        .unwrap_or_else(|| sha256_hex(package_bytes))
}

/// The exact revision's verified bytes, from the project owner.
fn download(inputs: &Inputs) -> Result<(Value, Vec<u8>), Failure> {
    let mut receipt = ds_cli_auth::grid_models_for_project(
        inputs.require("lane")?,
        inputs.require("project")?,
        &ds_cli_auth::GridModelsCommand::Download {
            model: inputs.require("model")?.into(),
            revision: inputs.require("revision")?.into(),
        },
    )?;
    let bytes = receipt.bytes.take().ok_or_else(|| {
        Failure::failed(
            PACKAGE_INVALID.code,
            "the verified owner returned no package",
        )
        .remedy(PACKAGE_INVALID.remedy)
    })?;
    Ok((receipt.data, bytes))
}

fn counts(entries: &[PackageAssetEntry]) -> Value {
    let attachments = entries.iter().filter_map(|entry| entry.attachment.as_ref());
    json!({
        "assets": entries.len(),
        "protected": entries.iter().filter(|entry| entry.protected).count(),
        "attachments": attachments.clone().count(),
        "stale_attachments": attachments.filter(|binding| binding.stale).count(),
    })
}

fn listing(bytes: &[u8], limit: usize) -> Result<Map<String, Value>, Failure> {
    let entries = package_assets::list_package_assets(bytes).map_err(refusal)?;
    let manifest = package::read_manifest("the package", bytes)?;
    let mut answer = Map::new();
    answer.insert("package_sha256".into(), json!(sha256_hex(bytes)));
    answer.insert("byte_len".into(), json!(bytes.len()));
    answer.insert(
        "model".into(),
        json!({
            "id": manifest.model.model_id.as_str(),
            "revision": manifest.model.model_revision,
            "fingerprint": manifest.model.snapshot_fingerprint,
        }),
    );
    answer.insert("counts".into(), counts(&entries));
    let (shown, withheld) = take(entries, limit);
    answer.insert("assets".into(), json!(shown));
    if withheld > 0 {
        answer.insert(
            "more".into(),
            json!({ "truncated": [{ "field": "assets", "withheld": withheld, "limit": limit }] }),
        );
    }
    Ok(answer)
}

fn extracted(bytes: &[u8], leaf: &str, out: &str) -> Result<Map<String, Value>, Failure> {
    let asset = package_assets::extract_package_asset(bytes, leaf).map_err(refusal)?;
    // Role and binding come from the same manifest the bytes were verified
    // against; the listing names exactly one entry for a leaf that extracted.
    let listed = package_assets::list_package_assets(bytes).map_err(refusal)?;
    let digest = sha256_hex(&asset.bytes);
    let entry = listed
        .iter()
        .find(|entry| entry.leaf == leaf && entry.sha256 == digest);
    crate::apply::write_new(out, &asset.bytes)?;
    let mut answer = Map::new();
    answer.insert("package_sha256".into(), json!(sha256_hex(bytes)));
    answer.insert("leaf".into(), json!(leaf));
    answer.insert("role".into(), json!(entry.map(|entry| entry.role.as_str())));
    answer.insert("sha256".into(), json!(digest));
    answer.insert("byte_len".into(), json!(asset.bytes.len()));
    if let Some(binding) = entry.and_then(|entry| entry.attachment.as_ref()) {
        answer.insert("attachment".into(), json!(binding));
    }
    answer.insert("verified".into(), json!(true));
    answer.insert("out".into(), json!(out));
    Ok(answer)
}

fn receipt(receipt: &package_assets::AssetEditReceipt) -> Result<Map<String, Value>, Failure> {
    match serde_json::to_value(receipt) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(
            Failure::failed(CONTENT_CHANGED.code, "the receipt could not be encoded")
                .remedy(CONTENT_CHANGED.remedy),
        ),
    }
}

/// The exchange owner's refusal, under this command family's stable code.
fn refusal(error: AssetError) -> Failure {
    let message = error.to_string();
    match error {
        AssetError::PackageDigestMismatch { expected, actual } => {
            Failure::conflict(PACKAGE_DIGEST_MISMATCH.code, message)
                .remedy(PACKAGE_DIGEST_MISMATCH.remedy)
                .detail(json!({ "expected": expected, "actual": actual }))
        }
        AssetError::PackageUnreadable { .. } => {
            Failure::invalid(NOT_A_PACKAGE.code, message).remedy(NOT_A_PACKAGE.remedy)
        }
        AssetError::Package(_) => {
            Failure::invalid(PACKAGE_INVALID.code, message).remedy(PACKAGE_INVALID.remedy)
        }
        AssetError::PackageUnsupported { .. } => {
            Failure::invalid(PACKAGE_UNSUPPORTED.code, message).remedy(PACKAGE_UNSUPPORTED.remedy)
        }
        AssetError::LeafInvalid { .. } => {
            Failure::invalid(ASSET_LEAF_INVALID.code, message).remedy(ASSET_LEAF_INVALID.remedy)
        }
        AssetError::LeafProtected { .. } => {
            Failure::invalid(ASSET_LEAF_PROTECTED.code, message).remedy(ASSET_LEAF_PROTECTED.remedy)
        }
        AssetError::LeafCollides { .. } => {
            Failure::invalid(ASSET_LEAF_COLLIDES.code, message).remedy(ASSET_LEAF_COLLIDES.remedy)
        }
        AssetError::LeafExists { .. } => {
            Failure::conflict(ASSET_LEAF_EXISTS.code, message).remedy(ASSET_LEAF_EXISTS.remedy)
        }
        AssetError::AssetNotFound { .. } => {
            Failure::invalid(ASSET_NOT_FOUND.code, message).remedy(ASSET_NOT_FOUND.remedy)
        }
        AssetError::AmbiguousLeaf { digests, .. } => {
            Failure::conflict(ASSET_LEAF_AMBIGUOUS.code, message)
                .remedy(ASSET_LEAF_AMBIGUOUS.remedy)
                .detail(json!({ "digests": digests }))
        }
        AssetError::NotAnAttachment { .. } => {
            Failure::invalid(ASSET_NOT_ATTACHMENT.code, message).remedy(ASSET_NOT_ATTACHMENT.remedy)
        }
        AssetError::RoleInvalid { .. } => {
            Failure::invalid(ASSET_ROLE_INVALID.code, message).remedy(ASSET_ROLE_INVALID.remedy)
        }
        AssetError::EmptyAttachment => {
            Failure::invalid(ASSET_FILE_INVALID.code, message).remedy(ASSET_FILE_INVALID.remedy)
        }
        AssetError::PlsBackupInvalid { .. } => {
            Failure::invalid(ASSET_PLS_INVALID.code, message).remedy(ASSET_PLS_INVALID.remedy)
        }
        AssetError::ContentChanged { .. } => {
            Failure::failed(CONTENT_CHANGED.code, message).remedy(CONTENT_CHANGED.remedy)
        }
    }
}

// ── Human presentation ────────────────────────────────────────────────────

pub fn render_list(data: &Value) -> String {
    let mut out = format!(
        "{}  rev {}  {}\n  {} assets · {} attachments · {} stale\n",
        data["model"]["id"].as_str().unwrap_or("?"),
        data["model"]["revision"],
        data["model"]["fingerprint"].as_str().unwrap_or("?"),
        data["counts"]["assets"],
        data["counts"]["attachments"],
        data["counts"]["stale_attachments"],
    );
    for asset in data["assets"].as_array().into_iter().flatten() {
        let status = match asset.get("attachment") {
            Some(binding) if binding["stale"].as_bool() == Some(true) => "attachment, stale",
            Some(_) => "attachment",
            None => "protected",
        };
        out.push_str(&format!(
            "  {:<40}  {:<32}  {:>10}  {status}\n",
            asset["leaf"].as_str().unwrap_or("?"),
            asset["role"].as_str().unwrap_or("?"),
            asset["byte_len"],
        ));
    }
    if let Some(more) = data.get("more") {
        out.push_str(&format!("more: {more}\n"));
    }
    out
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}
