//! `ds design transformer backup` and `restore-deleted` — recovery of a
//! DELETED transformer from its verified delete backup.
//!
//! Deletion archives the transformer as one native `.dsgrid` object and
//! removes the working document; the backup ledger and `ds design transformer
//! inventory` name that object. ds-brain owns both recovery actions
//! (`read_transformer_backup`, `restore_deleted_transformer`): it verifies the
//! stored bytes, the native member digests and the package's project and
//! transformer, signs a short generation-pinned read, and recreates the
//! document inside one transaction that refuses an existing name or a
//! diverged version history. The core decodes, fetches and verifies the exact
//! bytes; this surface names the inputs, writes the optional file and shapes
//! the receipt. Contract: ds-brain `docs/contracts/transformer-retirement.md`
//! "Restoring a deleted transformer".

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DeletedBackupRequest, RetirementBackup};
use serde_json::{Value, json};

use super::{LANE_ARG, PROJECT_ARG};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "The one deleted transformer, by its exact name.",
)
.required();
const OBJECT: Arg = Arg::value(
    "object",
    "<transformers/…dsgrid>",
    "The backup object the inventory or backup ledger names for this deletion.",
)
.required();
const GENERATION: Arg = Arg::value(
    "generation",
    "<n>",
    "Pin one stored generation of the backup object; the live one when omitted.",
);
const SHA256: Arg = Arg::value(
    "sha256",
    "<hex>",
    "Pin the backup's SHA-256 as an earlier read returned it.",
);
const OUT: Arg = Arg::value(
    "out",
    "<path.dsgrid>",
    "Also save the verified backup bytes here.",
);
const OVERWRITE: Arg = Arg::switch("overwrite", "Replace --out if it already exists.");

pub const BACKUP_PIN_INVALID: Refusal = Refusal {
    code: "backup_pin_invalid",
    when: "--object is not a .dsgrid key under transformers/ in the configured backup bucket, or --generation/--sha256 is malformed",
    remedy: "copy the object, generation and sha256 exactly from `ds design transformer inventory` or an earlier backup read",
};
pub const BACKUP_NOT_FOUND: Refusal = Refusal {
    code: "backup_not_found",
    when: "no stored generation of that backup object exists",
    remedy: "read the current backup object from `ds design transformer inventory --transformer <name>`",
};
pub const BACKUP_UNVERIFIED: Refusal = Refusal {
    code: "backup_unverified",
    when: "the stored backup bytes, or the bytes fetched, do not match their recorded length and SHA-256",
    remedy: "do not restore from it; read another recorded backup of the transformer",
};
pub const BACKUP_IDENTITY_MISMATCH: Refusal = Refusal {
    code: "backup_identity_mismatch",
    when: "the backup package names another project or transformer",
    remedy: "name the transformer and project the backup was taken from",
};
pub const BACKUP_UNAVAILABLE: Refusal = Refusal {
    code: "backup_unavailable",
    when: "backup reading or signing is not configured on this deployment",
    remedy: "retry on a deployment whose report service carries transformer backup recovery",
};
pub const TRANSFORMER_EXISTS: Refusal = Refusal {
    code: "transformer_exists",
    when: "a transformer document with this name exists; restoration never overwrites",
    remedy: "inspect it with `ds design transformer inventory`; a retired one comes back with `ds design transformer restore`",
};
pub const HISTORY_DIVERGED: Refusal = Refusal {
    code: "history_diverged",
    when: "the retained version history no longer ends where the backup does (a later transformer of the same name rebuilt it)",
    remedy: "read the transformer's versions; restoring this backup would contradict them",
};
pub const SPECIAL_DOCUMENT: Refusal = Refusal {
    code: "special_document",
    when: "the name is a project-level special document, not a transformer",
    remedy: "name a transformer the inventory lists as deleted",
};
pub const OUTPUT_EXISTS: Refusal = Refusal {
    code: "output_exists",
    when: "--out already exists and --overwrite was not given, or it could not be written",
    remedy: "choose another --out path or pass --overwrite",
};

const RECOVERY_REFUSALS: [Refusal; 8] = [
    BACKUP_PIN_INVALID,
    BACKUP_NOT_FOUND,
    BACKUP_UNVERIFIED,
    BACKUP_IDENTITY_MISMATCH,
    BACKUP_UNAVAILABLE,
    TRANSFORMER_EXISTS,
    HISTORY_DIVERGED,
    SPECIAL_DOCUMENT,
];

const fn refusals<const TOTAL: usize>(base: &[Refusal], own: &[Refusal]) -> [Refusal; TOTAL] {
    assert!(TOTAL == base.len() + RECOVERY_REFUSALS.len() + own.len());
    let mut out = [OUTPUT_EXISTS; TOTAL];
    let mut i = 0;
    while i < base.len() {
        out[i] = base[i];
        i += 1;
    }
    let mut j = 0;
    while j < RECOVERY_REFUSALS.len() {
        out[i + j] = RECOVERY_REFUSALS[j];
        j += 1;
    }
    i += RECOVERY_REFUSALS.len();
    let mut k = 0;
    while k < own.len() {
        out[i + k] = own[k];
        k += 1;
    }
    out
}

static READ_REFUSALS: [Refusal; 30] = refusals(super::NATIVE_READ_REFUSALS, &[OUTPUT_EXISTS]);
static WRITE_REFUSALS: [Refusal; 31] = refusals(super::NATIVE_WRITE_REFUSALS, &[]);

pub static BACKUP: Command = Command {
    id: "design.transformer.backup",
    path: &["design", "transformer", "backup"],
    contract: 1,
    summary: "Read a deleted transformer's verified backup, and save its bytes.",
    purpose: "\
Before restoring a deleted LV transformer, confirm what its delete backup \
holds. The report service reads the one backup object the inventory names \
(the live generation, or the pinned one), verifies its bytes, native member \
digests and LV profile, and requires it to name this project and transformer. \
The answer is its source revision, layers and exact receipt (bucket, object, \
generation, sha256, byte_length) — the pins `restore-deleted` needs. With \
--out the verified `.dsgrid` is fetched through a short generation-pinned \
signed read and saved only when its length and SHA-256 match. Needs \
transformer.delete; writes nothing in the project.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        TRANSFORMER,
        OBJECT,
        GENERATION,
        SHA256,
        OUT,
        OVERWRITE,
        LANE_ARG,
    ],
    output: "\
Lane and the named project, the transformer `name`, `source_revision`, \
`layers`, the verified `backup` receipt, and with --out the saved `file` \
(path, bytes, sha256). The signed read is spent inside ds and never shown.",
    examples: &[Example {
        command: "ds design transformer backup --project <id> --transformer TX-1 --object transformers/1788000000-<id>-TX-1.dsgrid --out ./TX-1.dsgrid --output json",
        note: "Pass the returned backup.generation and backup.sha256 to `restore-deleted`.",
        runnable: false,
    }],
    refusals: &READ_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[
        "deleted transformer backup",
        "download transformer backup",
        "recover deleted transformer",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static RESTORE_DELETED: Command = Command {
    id: "design.transformer.restore-deleted",
    path: &["design", "transformer", "restore-deleted"],
    contract: 1,
    summary: "Recreate a deleted transformer from its verified backup (needs --yes).",
    purpose: "\
Brings a DELETED LV transformer back from the delete backup `backup` read, \
pinned to that read's exact generation and SHA-256. Inside one transaction \
the report service refuses when any document holds the name or when the \
retained version history no longer ends where the backup does; otherwise it \
recreates the working document byte for byte and records who restored it. \
Retired (not deleted) transformers use `ds design transformer restore`. \
Needs transformer.delete.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        TRANSFORMER,
        OBJECT,
        GENERATION.required(),
        SHA256.required(),
        LANE_ARG,
    ],
    output: "\
Lane and the named project, and the `restoration`: name, lifecycle state, \
restored_at/by, source_revision, the new document version, layers and the \
backup receipt it was restored from.",
    examples: &[Example {
        command: "ds design transformer restore-deleted --project <id> --transformer TX-1 --object transformers/1788000000-<id>-TX-1.dsgrid --generation 1788000000123456 --sha256 <hex> --yes",
        note: "Read the pins first with `ds design transformer backup`.",
        runnable: false,
    }],
    refusals: &WRITE_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["restore deleted transformer", "undelete transformer"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn pin_invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(BACKUP_PIN_INVALID.code, message).remedy(BACKUP_PIN_INVALID.remedy)
}

fn generation(inputs: &Inputs) -> Result<Option<i64>, Failure> {
    inputs
        .value("generation")
        .map(|raw| {
            raw.trim()
                .parse::<i64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| pin_invalid("--generation is a positive integer"))
        })
        .transpose()
}

fn receipt_json(backup: &RetirementBackup) -> Value {
    json!({
        "bucket": backup.bucket(),
        "object": backup.object(),
        "generation": backup.generation(),
        "sha256": backup.sha256(),
        "byte_length": backup.byte_length(),
    })
}

fn write_out(path: &str, bytes: &[u8], overwrite: bool) -> Result<(), Failure> {
    let refused = |message: String| {
        Failure::invalid(OUTPUT_EXISTS.code, message).remedy(OUTPUT_EXISTS.remedy)
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .map_err(|error| refused(format!("--out {path} could not be created: {error}")))?;
    std::io::Write::write_all(&mut file, bytes)
        .map_err(|error| refused(format!("--out {path} could not be written: {error}")))
}

pub fn run_backup(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = super::named_project(inputs)?;
    let request = DeletedBackupRequest::read(
        inputs.require("transformer")?.to_owned(),
        inputs.require("object")?.to_owned(),
        generation(inputs)?,
        inputs.value("sha256").map(str::to_owned),
    )
    .map_err(|error| pin_invalid(error.to_string()))?;
    let out = inputs.value("out");
    if let Some(path) = out
        && !inputs.switch("overwrite")
        && std::path::Path::new(path).exists()
    {
        return Err(
            Failure::invalid(OUTPUT_EXISTS.code, format!("--out {path} already exists"))
                .remedy(OUTPUT_EXISTS.remedy),
        );
    }
    let headless = ds_cli_auth::device::deleted_transformer_backup_for_project(
        inputs.require("lane")?,
        &project,
        &request,
        out.is_some(),
    )?;
    let read = headless.result();
    let mut output = super::named_project_receipt(headless.lane(), headless.project_id());
    output["name"] = json!(read.name());
    output["source_revision"] = json!(read.source_revision());
    output["layers"] = json!(read.layers());
    output["backup"] = receipt_json(read.backup());
    if let Some(path) = out {
        let bytes = read.bytes().ok_or_else(|| {
            Failure::invalid(
                BACKUP_UNVERIFIED.code,
                "the verified backup bytes were not fetched",
            )
            .remedy(BACKUP_UNVERIFIED.remedy)
        })?;
        write_out(path, bytes, inputs.switch("overwrite"))?;
        output["file"] = json!({
            "path": path,
            "bytes": bytes.len(),
            "sha256": read.backup().sha256(),
        });
    }
    Ok(output)
}

pub fn render_backup(data: &Value) -> String {
    let backup = &data["backup"];
    let mut out = format!(
        "{} backup · revision {} · {} layer(s)\n  {} gen {} · {} bytes\n  sha256 {}\n",
        data["name"].as_str().unwrap_or("?"),
        data["source_revision"],
        data["layers"].as_array().map(Vec::len).unwrap_or(0),
        backup["object"].as_str().unwrap_or("?"),
        backup["generation"],
        backup["byte_length"],
        backup["sha256"].as_str().unwrap_or("?"),
    );
    if let Some(path) = data["file"]["path"].as_str() {
        out.push_str(&format!("  saved {path}\n"));
    }
    out
}

pub fn run_restore_deleted(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = super::named_project(inputs)?;
    let generation = generation(inputs)?.ok_or_else(|| pin_invalid("--generation is required"))?;
    let request = DeletedBackupRequest::restore(
        inputs.require("transformer")?.to_owned(),
        inputs.require("object")?.to_owned(),
        generation,
        inputs.require("sha256")?.to_owned(),
    )
    .map_err(|error| pin_invalid(error.to_string()))?;
    let headless = ds_cli_auth::device::restore_deleted_transformer_for_project(
        inputs.require("lane")?,
        &project,
        &request,
    )?;
    let restored = headless.result();
    let mut output = super::named_project_receipt(headless.lane(), headless.project_id());
    output["restoration"] = json!({
        "name": restored.name(),
        "state": restored.state(),
        "restored_at": restored.restored_at(),
        "restored_by": restored.restored_by(),
        "source_revision": restored.source_revision(),
        "version": restored.version(),
        "layers": restored.layers(),
        "backup": receipt_json(restored.backup()),
    });
    Ok(output)
}

pub fn render_restore_deleted(data: &Value) -> String {
    let restored = &data["restoration"];
    format!(
        "restored {} ({}) · version {} · from {} gen {}\n",
        restored["name"].as_str().unwrap_or("?"),
        restored["state"].as_str().unwrap_or("?"),
        restored["version"],
        restored["backup"]["object"].as_str().unwrap_or("?"),
        restored["backup"]["generation"],
    )
}
