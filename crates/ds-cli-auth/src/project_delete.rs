//! `ds auth project delete`: the owner-only archive-then-delete of one whole
//! project (ds-brain docs/contracts/project-delete.md).
//!
//! ds-brain decides everything that can be decided: the inventory, each
//! member's disposition, the plan digest, the archive's one object, the
//! receipt and the delete. This host does the edge's part: it pages the
//! authorized export, writes the archive locally (resumably), uploads it
//! through the one minted session, re-reads the stored generation to verify
//! its sha256, and only then asks for the receipt and the apply.
//!
//! The archive is a zip whose first member is `index.json`; Firestore groups
//! are gzip-compressed NDJSON of native documents, archived objects are stored
//! as files. Survey media is never archived or deleted here: the index lists
//! it for the standalone media operation.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::ClientError;
use ds_client_core::project_deletion::Command as Door;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    AUTH_REVOKED_REFUSAL, IDENTITY_REFUSAL, LANE, PROFILE_DIGEST_REFUSAL, PROFILE_REFUSAL,
    PROFILE_UNSAFE_REFUSAL, PROJECT_ID, SIGNED_OUT_REFUSAL, STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL, TRANSIENT_REFUSAL, UNREADABLE_REFUSAL,
};

pub(crate) const FORBIDDEN: Refusal = Refusal {
    code: "project_delete_forbidden",
    when: "the signed-in principal lacks the platform owner authority (platform.admin)",
    remedy: "sign in as a platform owner (developer or platform_admin app role); project.delete is not enough",
};
const INVALID: Refusal = Refusal {
    code: "project_delete_invalid",
    when: "the project id, digest or request violates the delete contract",
    remedy: "use one exact project id and the plan digest --dry-run printed",
};
const NOT_FOUND: Refusal = Refusal {
    code: "project_delete_not_found",
    when: "no project with this id exists (or it is already deleted)",
    remedy: "check the id with ds pm project list; a finished delete's record is in the plan's record",
};
const PLAN_CHANGED: Refusal = Refusal {
    code: "project_delete_plan_changed",
    when: "the project changed since the plan digest you passed",
    remedy: "re-run with --dry-run, review the new plan and pass its digest",
};
const BLOCKED: Refusal = Refusal {
    code: "project_delete_blocked",
    when: "the plan has members with no archive or delete route",
    remedy: "read the plan's blockers; they need an owner decision or a new archive route",
};
const ARCHIVE_REQUIRED: Refusal = Refusal {
    code: "project_delete_archive_required",
    when: "nothing is deleted without a verified archive receipt for this exact digest",
    remedy: "re-run without --archive-only so the archive is built, uploaded and receipted first",
};
const ARCHIVE_MISMATCH: Refusal = Refusal {
    code: "project_delete_archive_mismatch",
    when: "the stored archive differs from its declaration (size, CRC32C or sha256)",
    remedy: "remove the local staging named in the detail and re-run; the stored object is never overwritten",
};
const IN_PROGRESS: Refusal = Refusal {
    code: "project_delete_in_progress",
    when: "another apply of this project is running, or the delete already began",
    remedy: "wait for it to end, then re-run the same command to resume",
};
const UNAVAILABLE: Refusal = Refusal {
    code: "project_delete_unavailable",
    when: "a store the inventory or delete needs is unavailable",
    remedy: "retry; a partial apply resumes with the same digest",
};
const GUARD: Refusal = Refusal {
    code: "project_delete_guard",
    when: "apply reached a member outside the project's own namespaces and stopped",
    remedy: "report it as a defect with the detail; nothing outside the project was deleted",
};
const DIGEST_REQUIRED: Refusal = Refusal {
    code: "project_delete_digest_required",
    when: "archiving and deleting need the plan digest you reviewed",
    remedy: "run with --dry-run first and pass its plan_digest as --plan-digest",
};
const LOCAL_IO: Refusal = Refusal {
    code: "project_delete_local_io",
    when: "the local archive staging could not be written or read",
    remedy: "free disk space or fix permissions on the staging directory named in the detail, then re-run",
};
const DOWNLOAD_MISMATCH: Refusal = Refusal {
    code: "project_delete_download_mismatch",
    when: "an archived object's bytes differ from its stored size or checksum",
    remedy: "re-run; the object is downloaded again from its pinned generation",
};
const UPLOAD_FAILED: Refusal = Refusal {
    code: "project_delete_upload_failed",
    when: "the archive upload did not complete",
    remedy: "re-run; the upload resumes from the bytes storage already committed",
};
const VERIFY_FAILED: Refusal = Refusal {
    code: "project_delete_verify_failed",
    when: "the stored archive's sha256 differs from the archive built here",
    remedy: "do not apply; report it with the detail",
};
const ARCHIVE_OUT_EXISTS: Refusal = Refusal {
    code: "project_delete_archive_out_exists",
    when: "--archive-out names an existing file with different bytes",
    remedy: "choose a new path; an existing file is never overwritten",
};
const INCOMPLETE: Refusal = Refusal {
    code: "project_delete_incomplete",
    when: "apply stopped with residuals it may not delete",
    remedy: "read the residuals; members written after the archive or denied deletes need an owner decision",
};

const REFUSALS: &[Refusal] = &[
    FORBIDDEN,
    INVALID,
    NOT_FOUND,
    PLAN_CHANGED,
    BLOCKED,
    ARCHIVE_REQUIRED,
    ARCHIVE_MISMATCH,
    IN_PROGRESS,
    UNAVAILABLE,
    GUARD,
    DIGEST_REQUIRED,
    LOCAL_IO,
    DOWNLOAD_MISMATCH,
    UPLOAD_FAILED,
    VERIFY_FAILED,
    ARCHIVE_OUT_EXISTS,
    INCOMPLETE,
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    SIGNED_OUT_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
];

const PLAN_DIGEST: Arg = Arg::value(
    "plan-digest",
    "<sha256:digest>",
    "The plan digest --dry-run printed; required to archive or delete.",
);
const ARCHIVE_OUT: Arg = Arg::value(
    "archive-out",
    "<path>",
    "Also keep the verified archive at this new local path.",
);
const ARCHIVE_ONLY: Arg = Arg::switch(
    "archive-only",
    "Archive, upload, verify and receipt, then stop before deleting anything.",
);
const DRY_RUN: Arg = Arg::switch(
    "dry-run",
    "Print the plan, its counts and digest; nothing is archived or deleted.",
);

pub static COMMAND: Command = Command {
    id: "auth.project.delete",
    path: &["auth", "project", "delete"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Archive one whole project, then delete it: owner-only and audited.",
    purpose: "Needs the platform owner authority (platform.admin). --dry-run prints the complete plan: every member of the project in Firestore, the messaging database, storage and BigQuery, each marked archive (original), delete_only (mirror or derived), retained (survey media, never touched here; its references name the project whose prefix holds them) or blocked, with a plan digest. With --plan-digest and --yes it pages the authorized export, builds a zip archive (index.json first) in local staging, uploads it once to the deleted-projects bucket, re-reads the stored bytes to verify their sha256 and records the receipt; nothing is deleted before that. Without --archive-only it then deletes exactly the receipted plan in bounded batches until complete. Re-running the same command resumes an interrupted archive, upload or apply and never writes a second archive.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT_ID,
        DRY_RUN,
        PLAN_DIGEST,
        ARCHIVE_ONLY,
        ARCHIVE_OUT,
        LANE,
    ],
    output: "The plan (groups and totals by disposition, blockers, digest, archive object, retained media) and, when run, the archive receipt (object, generation, size, sha256) and the apply state, deleted counts and residuals.",
    examples: &[
        Example {
            command: "ds auth project delete --project <exact-id> --dry-run --output json",
            note: "Read the plan and its digest; nothing changes.",
            runnable: false,
        },
        Example {
            command: "ds auth project delete --project <exact-id> --plan-digest <sha256:digest> --archive-only --yes",
            note: "Archive and receipt only; the project is untouched.",
            runnable: false,
        },
        Example {
            command: "ds auth project delete --project <exact-id> --plan-digest <sha256:digest> --archive-out ./project.zip --yes",
            note: "Archive, keep a local copy, then delete.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[
        "delete project",
        "remove project",
        "purge project",
        "archive project",
        "project archive",
    ],
    requires: Requires::Server,
    availability: super::native_availability,
};

/// One project-delete action over the lane's credential.
pub(crate) trait Gateway {
    fn call(&mut self, command: &Door) -> Result<Value, Failure>;
}

/// Byte movement: pinned downloads and the resumable upload.
pub(crate) trait Bytes {
    /// Stream `url` into `sink`, refusing more than `limit` bytes.
    fn download(&mut self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<u64, Failure>;
    /// Drive the resumable session; `Ok(false)` means the session must be
    /// reopened (expired or rejected), `Ok(true)` that every byte is committed.
    fn upload(
        &mut self,
        session: &str,
        path: &Path,
        size: u64,
        sha256: &str,
    ) -> Result<bool, Failure>;
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = super::Lane::parse(inputs.require("lane")?)?;
    let mut gateway = NativeGateway::open(lane)?;
    let options = Options {
        project: inputs.require("project")?.to_owned(),
        dry_run: inputs.switch("dry-run"),
        plan_digest: inputs.value("plan-digest").map(str::to_owned),
        archive_only: inputs.switch("archive-only"),
        archive_out: inputs.value("archive-out").map(PathBuf::from),
        staging: staging_root()?,
    };
    let mut answer = execute(&mut gateway, &mut HttpBytes, &options)?;
    answer["lane"] = json!(lane.token());
    Ok(answer)
}

pub(crate) struct Options {
    pub project: String,
    pub dry_run: bool,
    pub plan_digest: Option<String>,
    pub archive_only: bool,
    pub archive_out: Option<PathBuf>,
    pub staging: PathBuf,
}

fn io(error: impl std::fmt::Display, path: &Path) -> Failure {
    Failure::unavailable(LOCAL_IO.code, format!("{}: {error}", path.display()))
        .remedy(LOCAL_IO.remedy)
        .detail(json!({ "path": path.display().to_string() }))
}

/// The plan summary every answer starts from.
fn summary(planned: &Value) -> Value {
    let plan = &planned["plan"];
    json!({
        "project_id": plan["project_id"],
        "plan_digest": plan["plan_digest"],
        "totals": plan["totals"],
        "groups": plan["groups"],
        "blockers": plan["blockers"],
        "archive": {"bucket": plan["archive_bucket"], "object": plan["archive_object"]},
        "retained_objects": plan["retained_objects"],
        "retained_objects_truncated": plan["retained_objects_truncated"],
        "retained_evidence": plan["retained_evidence"],
        "record": planned["record"],
    })
}

pub(crate) fn execute(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    options: &Options,
) -> Result<Value, Failure> {
    let pid = options.project.as_str();
    let planned = gateway.call(&Door::Plan {
        project_id: pid.to_owned(),
    })?;
    let digest = planned["plan"]["plan_digest"]
        .as_str()
        .ok_or_else(|| Failure::unavailable(UNREADABLE_REFUSAL.code, "the plan has no digest"))?
        .to_owned();
    let mut answer = json!({ "plan": summary(&planned), "mode": "plan" });
    if options.dry_run {
        return Ok(answer);
    }
    let Some(wanted) = options.plan_digest.as_deref() else {
        return Err(Failure::invalid(
            DIGEST_REQUIRED.code,
            "--plan-digest is required to archive or delete",
        )
        .remedy(DIGEST_REQUIRED.remedy)
        .detail(json!({ "plan_digest": digest }))
        .next(format!("ds auth project delete --project {pid} --dry-run")));
    };
    if wanted != digest {
        return Err(Failure::conflict(
            PLAN_CHANGED.code,
            "the project changed since the reviewed plan",
        )
        .remedy(PLAN_CHANGED.remedy)
        .detail(json!({ "reviewed": wanted, "current": digest }))
        .next(format!("ds auth project delete --project {pid} --dry-run")));
    }
    let record = &planned["record"];
    let receipted = record["archive"]["plan_digest"] == json!(digest);
    let receipt = if receipted {
        if let Some(out) = &options.archive_out {
            keep_remote_copy(gateway, bytes, pid, &digest, out)?;
        }
        record["archive"].clone()
    } else {
        archive(gateway, bytes, options, &planned, &digest)?
    };
    answer["mode"] = json!("archived");
    answer["archive_receipt"] = receipt;
    if options.archive_only {
        return Ok(answer);
    }
    let applied = apply(gateway, pid, &digest)?;
    answer["mode"] = json!("deleted");
    answer["apply"] = applied;
    Ok(answer)
}

fn apply(gateway: &mut dyn Gateway, pid: &str, digest: &str) -> Result<Value, Failure> {
    let mut last: Option<Value> = None;
    for _ in 0..200 {
        let result = gateway.call(&Door::Apply {
            project_id: pid.to_owned(),
            plan_digest: digest.to_owned(),
        })?;
        if result["complete"] == json!(true) {
            return Ok(result);
        }
        let stalled = last.as_ref().is_some_and(|previous| {
            previous["deleted"] == result["deleted"] && previous["residuals"] == result["residuals"]
        });
        if stalled
            && result["residuals"]
                .as_array()
                .is_some_and(|r| !r.is_empty())
        {
            return Err(
                Failure::conflict(INCOMPLETE.code, "the delete stopped with residuals")
                    .remedy(INCOMPLETE.remedy)
                    .detail(result),
            );
        }
        last = Some(result);
    }
    Err(
        Failure::unavailable(UNAVAILABLE.code, "the delete made no final progress")
            .remedy(UNAVAILABLE.remedy)
            .detail(last.unwrap_or(Value::Null)),
    )
}

// ---------------------------------------------------------------------------
// Archive
// ---------------------------------------------------------------------------

const SOURCES: [&str; 4] = ["firestore", "trash", "messaging", "objects"];

/// The staging of one project's one plan digest.
struct Staging {
    root: PathBuf,
}

impl Staging {
    fn new(base: &Path, pid: &str, digest: &str) -> Result<Self, Failure> {
        let hex = digest.trim_start_matches("sha256:");
        let root = base.join(pid).join(hex);
        fs::create_dir_all(root.join("members")).map_err(|e| io(e, &root))?;
        Ok(Self { root })
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn read(&self, name: &str) -> Option<Value> {
        let bytes = fs::read(self.path(name)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
    fn write(&self, name: &str, value: &Value) -> Result<(), Failure> {
        let path = self.path(name);
        let part = self.path(&format!("{name}.part"));
        fs::write(&part, serde_json::to_vec_pretty(value).unwrap_or_default())
            .and_then(|()| fs::rename(&part, &path))
            .map_err(|e| io(e, &path))
    }
    fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.path(name));
    }
}

/// One archived file and what the index says about it.
fn member(
    path: &str,
    store: &str,
    source: &str,
    documents: Option<u64>,
    file: &Path,
) -> Result<Value, Failure> {
    let (size, sha256, _) = hash_file(file)?;
    let mut value =
        json!({ "path": path, "store": store, "source": source, "size": size, "sha256": sha256 });
    if let Some(count) = documents {
        value["documents"] = json!(count);
    }
    Ok(value)
}

/// size, sha256 hex and GCS CRC32C (base64 big-endian) of one file.
fn hash_file(path: &Path) -> Result<(u64, String, String), Failure> {
    let mut file = File::open(path).map_err(|e| io(e, path))?;
    let mut sha = Sha256::new();
    let mut crc = 0_u32;
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer).map_err(|e| io(e, path))?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
        crc = crc32c::crc32c_append(crc, &buffer[..n]);
        size += n as u64;
    }
    Ok((size, format!("{:x}", sha.finalize()), crc_base64(crc)))
}

fn crc_base64(crc: u32) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(crc.to_be_bytes())
}

/// A Firestore group template as one archive file name:
/// `eds_project/{pid}/forms/*/entries` -> `eds_project.pid.forms._.entries`.
fn group_file(group: &str) -> String {
    group
        .replace("{pid}", "pid")
        .replace("{conversation}", "conversation")
        .replace('*', "_")
        .replace('/', ".")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect()
}

/// An object name as a relative archive path; refuses traversal.
fn object_path(bucket: &str, name: &str) -> Option<String> {
    if name.is_empty()
        || name.starts_with('/')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || name.contains('\\')
        || bucket.contains('/')
    {
        return None;
    }
    Some(format!("objects/{bucket}/{name}"))
}

fn export_pages(
    gateway: &mut dyn Gateway,
    pid: &str,
    source: &str,
    mut each: impl FnMut(&Value) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let mut cursor: Option<String> = None;
    for _ in 0..1_000_000 {
        let page = gateway.call(&Door::Export {
            project_id: pid.to_owned(),
            source: source.to_owned(),
            cursor: cursor.clone(),
        })?;
        for item in page["items"].as_array().into_iter().flatten() {
            each(item)?;
        }
        if page["done"] == json!(true) {
            return Ok(());
        }
        let next = page["next_cursor"].as_str().unwrap_or("").to_owned();
        if next.is_empty() || Some(&next) == cursor.as_ref() {
            return Err(Failure::unavailable(
                UNREADABLE_REFUSAL.code,
                "an export page did not advance",
            ));
        }
        cursor = Some(next);
    }
    Err(Failure::unavailable(
        UNREADABLE_REFUSAL.code,
        "the export did not end",
    ))
}

/// Export one document source into gzip NDJSON files, one per group.
fn export_documents(
    gateway: &mut dyn Gateway,
    staging: &Staging,
    pid: &str,
    source: &str,
) -> Result<Vec<Value>, Failure> {
    let dir = staging.path(&format!("members/{source}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| io(e, &dir))?;
    let mut writers: BTreeMap<String, (flate2::write::GzEncoder<BufWriter<File>>, u64, String)> =
        BTreeMap::new();
    export_pages(gateway, pid, source, |item| {
        let group = item["group"].as_str().unwrap_or("").to_owned();
        let store = item["store"].as_str().unwrap_or(source).to_owned();
        let document = item
            .get("document")
            .filter(|d| d.is_object())
            .ok_or_else(|| {
                Failure::unavailable(
                    UNREADABLE_REFUSAL.code,
                    "an export item has no native document",
                )
            })?;
        if !writers.contains_key(&group) {
            let path = dir.join(format!("{}.ndjson.gz", group_file(&group)));
            let file = File::create(&path).map_err(|e| io(e, &path))?;
            writers.insert(
                group.clone(),
                (
                    flate2::write::GzEncoder::new(
                        BufWriter::new(file),
                        flate2::Compression::default(),
                    ),
                    0,
                    store,
                ),
            );
        }
        let (writer, count, _) = writers.get_mut(&group).expect("inserted above");
        let mut line = serde_json::to_vec(document).unwrap_or_default();
        line.push(b'\n');
        writer.write_all(&line).map_err(|e| io(e, &dir))?;
        *count += 1;
        Ok(())
    })?;
    let mut members = Vec::new();
    for (group, (writer, count, store)) in writers {
        let mut inner = writer.finish().map_err(|e| io(e, &dir))?;
        inner.flush().map_err(|e| io(e, &dir))?;
        drop(inner);
        let name = format!("members/{source}/{}.ndjson.gz", group_file(&group));
        let mut entry = member(&name, &store, &group, Some(count), &staging.path(&name))?;
        entry["group"] = json!(group);
        members.push(entry);
    }
    Ok(members)
}

/// Download every archived object of the export (skipping intact copies) and
/// list every retained one for the index.
fn export_objects(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    staging: &Staging,
    pid: &str,
) -> Result<(Vec<Value>, Vec<Value>), Failure> {
    let mut items = Vec::new();
    export_pages(gateway, pid, "objects", |item| {
        items.push(item.clone());
        Ok(())
    })?;
    let mut members = Vec::new();
    let mut retained = Vec::new();
    for item in items {
        let object = &item["object"];
        let bucket = object["bucket"].as_str().unwrap_or("");
        let name = object["name"].as_str().unwrap_or("");
        let generation = object["generation"].as_i64().unwrap_or(0);
        let size = object["size"].as_u64().unwrap_or(0);
        let source = format!("gs://{bucket}/{name}#{generation}");
        if object["retained"] == json!(true) {
            retained.push(json!({
                "source": source, "group": item["group"], "size": size,
                "reason": object["reason"],
            }));
            continue;
        }
        let relative = object_path(bucket, name).ok_or_else(|| {
            Failure::unavailable(
                UNREADABLE_REFUSAL.code,
                format!("unsafe object name {name}"),
            )
        })?;
        let path = staging.path(&format!("members/{relative}"));
        let want_crc = object["crc32c"].as_str().unwrap_or("");
        let intact = hash_file(&path)
            .map(|(s, _, crc)| s == size && (want_crc.is_empty() || crc == want_crc))
            .unwrap_or(false);
        if !intact {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| io(e, parent))?;
            }
            let url = object["url"].as_str().unwrap_or("");
            let mut file = BufWriter::new(File::create(&path).map_err(|e| io(e, &path))?);
            let got = bytes.download(url, size, &mut file)?;
            file.flush().map_err(|e| io(e, &path))?;
            drop(file);
            let (written, _, crc) = hash_file(&path)?;
            if got != size || written != size || (!want_crc.is_empty() && crc != want_crc) {
                let _ = fs::remove_file(&path);
                return Err(Failure::unavailable(
                    DOWNLOAD_MISMATCH.code,
                    format!("{source} did not verify"),
                )
                .remedy(DOWNLOAD_MISMATCH.remedy)
                .detail(json!({ "source": source, "size": size, "received": written })));
            }
        }
        let name = format!("members/{relative}");
        let mut entry = member(&name, "storage", &source, None, &staging.path(&name))?;
        entry["group"] = item["group"].clone();
        members.push(entry);
    }
    Ok((members, retained))
}

/// Build (or reuse) the local archive: export every source once, write the
/// index, then the zip with the index first.
pub(crate) fn build_archive(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    staging_base: &Path,
    planned: &Value,
    digest: &str,
) -> Result<(PathBuf, Value), Failure> {
    let pid = planned["plan"]["project_id"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let staging = Staging::new(staging_base, &pid, digest)?;
    let built = staging.path("archive.zip");
    if let Some(facts) = staging.read("archive.json")
        && hash_file(&built).is_ok_and(|(size, sha, _)| {
            facts["size"] == json!(size) && facts["sha256"] == json!(sha)
        })
    {
        return Ok((built, facts));
    }
    let mut progress = staging.read("progress.json").unwrap_or_else(|| json!({}));
    if progress["plan_digest"] != json!(digest) {
        progress = json!({ "plan_digest": digest, "done": {} });
    }
    for source in SOURCES {
        if progress["done"].get(source).is_some() {
            continue;
        }
        let entry = if source == "objects" {
            let (members, retained) = export_objects(gateway, bytes, &staging, &pid)?;
            json!({ "members": members, "retained": retained })
        } else {
            json!({ "members": export_documents(gateway, &staging, &pid, source)? })
        };
        progress["done"][source] = entry;
        staging.write("progress.json", &progress)?;
    }
    let mut members: Vec<Value> = Vec::new();
    let mut retained: Vec<Value> = Vec::new();
    for source in SOURCES {
        members.extend(
            progress["done"][source]["members"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        retained.extend(
            progress["done"][source]["retained"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
    }
    members.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    // The index names each member by its path inside the zip.
    let listed: Vec<Value> = members
        .iter()
        .map(|entry| {
            let mut entry = entry.clone();
            let inner = entry["path"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches("members/")
                .to_owned();
            entry["path"] = json!(inner);
            entry
        })
        .collect();
    let plan = &planned["plan"];
    let index = json!({
        "schema": "ds.project-archive/v1",
        "project_id": pid,
        "plan_digest": digest,
        "totals": plan["totals"],
        "groups": plan["groups"],
        "retained_evidence": plan["retained_evidence"],
        "members": listed,
        "retained": retained,
    });
    let index_bytes = serde_json::to_vec_pretty(&index).unwrap_or_default();
    let index_sha256 = format!("{:x}", Sha256::digest(&index_bytes));
    let part = staging.path("archive.zip.part");
    {
        let file = File::create(&part).map_err(|e| io(e, &part))?;
        let mut zip = zip::ZipWriter::new(BufWriter::new(file));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        zip.start_file("index.json", stored)
            .map_err(|e| io(e, &part))?;
        zip.write_all(&index_bytes).map_err(|e| io(e, &part))?;
        for entry in &members {
            let path = entry["path"].as_str().unwrap_or("");
            let name = path.trim_start_matches("members/");
            zip.start_file(name, stored).map_err(|e| io(e, &part))?;
            let mut source = File::open(staging.path(path)).map_err(|e| io(e, &part))?;
            std::io::copy(&mut source, &mut zip).map_err(|e| io(e, &part))?;
        }
        let mut inner = zip.finish().map_err(|e| io(e, &part))?;
        inner.flush().map_err(|e| io(e, &part))?;
    }
    fs::rename(&part, &built).map_err(|e| io(e, &built))?;
    let (size, sha256, crc32c) = hash_file(&built)?;
    let facts = json!({
        "size": size, "sha256": sha256, "crc32c": crc32c, "index_sha256": index_sha256,
        "members": members.len(), "retained": retained.len(),
    });
    staging.write("archive.json", &facts)?;
    staging.remove("session.json");
    Ok((built, facts))
}

fn archive(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    options: &Options,
    planned: &Value,
    digest: &str,
) -> Result<Value, Failure> {
    let pid = options.project.as_str();
    let (built, facts) = build_archive(gateway, bytes, &options.staging, planned, digest)?;
    let staging = Staging::new(&options.staging, pid, digest)?;
    let size = facts["size"].as_u64().unwrap_or(0);
    let sha256 = facts["sha256"].as_str().unwrap_or("").to_owned();
    upload(gateway, bytes, &staging, pid, digest, &built, &facts)?;

    // Verify the stored generation by reading it back.
    let read = gateway.call(&Door::ArchiveRead {
        project_id: pid.to_owned(),
        plan_digest: digest.to_owned(),
    })?;
    let generation = read["attrs"]["generation"].as_i64().unwrap_or(0);
    let url = read["url"].as_str().unwrap_or("");
    let mut hasher = HashingSink::default();
    let got = bytes.download(url, size, &mut hasher)?;
    let stored = format!("{:x}", hasher.sha.finalize());
    if got != size || stored != sha256 {
        return Err(Failure::failed(
            VERIFY_FAILED.code,
            "the stored archive does not match the built archive",
        )
        .remedy(VERIFY_FAILED.remedy)
        .detail(json!({ "built": sha256, "stored": stored, "size": size, "read": got })));
    }
    let record = gateway.call(&Door::ArchiveReceipt {
        project_id: pid.to_owned(),
        plan_digest: digest.to_owned(),
        generation,
        sha256: sha256.clone(),
    })?;
    if let Some(out) = &options.archive_out {
        keep_local_copy(&built, out, &sha256)?;
    }
    let mut receipt = record["archive"].clone();
    receipt["members"] = facts["members"].clone();
    receipt["retained"] = facts["retained"].clone();
    receipt["staging"] = json!(staging.root.display().to_string());
    Ok(receipt)
}

/// Upload through the persisted session when one exists (storage resumes from
/// its committed offset), else through a newly minted one. An object that
/// already exists is never written again.
fn upload(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    staging: &Staging,
    pid: &str,
    digest: &str,
    built: &Path,
    facts: &Value,
) -> Result<(), Failure> {
    let size = facts["size"].as_u64().unwrap_or(0);
    let sha256 = facts["sha256"].as_str().unwrap_or("");
    for _ in 0..3 {
        let session = match staging.read("session.json") {
            Some(saved) if saved["sha256"] == json!(sha256) => {
                saved["session_uri"].as_str().unwrap_or("").to_owned()
            }
            _ => {
                let opened = gateway.call(&Door::ArchiveOpen {
                    project_id: pid.to_owned(),
                    plan_digest: digest.to_owned(),
                    size,
                    sha256: sha256.to_owned(),
                    crc32c: facts["crc32c"].as_str().unwrap_or("").to_owned(),
                    index_sha256: facts["index_sha256"].as_str().unwrap_or("").to_owned(),
                })?;
                if opened["exists"] == json!(true) {
                    staging.remove("session.json");
                    return Ok(());
                }
                let uri = opened["session_uri"].as_str().unwrap_or("").to_owned();
                staging.write(
                    "session.json",
                    &json!({ "session_uri": uri, "sha256": sha256 }),
                )?;
                uri
            }
        };
        if bytes.upload(&session, built, size, sha256)? {
            staging.remove("session.json");
            return Ok(());
        }
        staging.remove("session.json");
    }
    Err(Failure::unavailable(
        UPLOAD_FAILED.code,
        "the archive upload session kept expiring",
    )
    .remedy(UPLOAD_FAILED.remedy))
}

#[derive(Default)]
struct HashingSink {
    sha: Sha256,
}

impl Write for HashingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sha.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn keep_local_copy(built: &Path, out: &Path, sha256: &str) -> Result<(), Failure> {
    if out.exists() {
        return match hash_file(out) {
            Ok((_, existing, _)) if existing == sha256 => Ok(()),
            _ => Err(Failure::conflict(
                ARCHIVE_OUT_EXISTS.code,
                format!("{} exists", out.display()),
            )
            .remedy(ARCHIVE_OUT_EXISTS.remedy)),
        };
    }
    fs::copy(built, out).map(|_| ()).map_err(|e| io(e, out))
}

/// The archive was receipted earlier (possibly elsewhere): keep a local copy
/// by reading the stored generation and checking it against the receipt.
fn keep_remote_copy(
    gateway: &mut dyn Gateway,
    bytes: &mut dyn Bytes,
    pid: &str,
    digest: &str,
    out: &Path,
) -> Result<(), Failure> {
    if out.exists() {
        return Err(Failure::conflict(
            ARCHIVE_OUT_EXISTS.code,
            format!("{} exists", out.display()),
        )
        .remedy(ARCHIVE_OUT_EXISTS.remedy));
    }
    let read = gateway.call(&Door::ArchiveRead {
        project_id: pid.to_owned(),
        plan_digest: digest.to_owned(),
    })?;
    let size = read["attrs"]["size"].as_u64().unwrap_or(0);
    let declared = read["attrs"]["metadata"]["sha256"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let part = out.with_extension("part");
    let mut file = BufWriter::new(File::create(&part).map_err(|e| io(e, &part))?);
    bytes.download(read["url"].as_str().unwrap_or(""), size, &mut file)?;
    file.flush().map_err(|e| io(e, &part))?;
    drop(file);
    let (_, sha, _) = hash_file(&part)?;
    if sha != declared {
        let _ = fs::remove_file(&part);
        return Err(Failure::failed(
            VERIFY_FAILED.code,
            "the stored archive does not match its receipt",
        )
        .remedy(VERIFY_FAILED.remedy));
    }
    fs::rename(&part, out).map_err(|e| io(e, out))
}

/// Where archives are staged: `$XDG_STATE_HOME/ds/project-delete`, else
/// `~/.local/state/ds/project-delete` (`%LOCALAPPDATA%` on Windows).
fn staging_root() -> Result<PathBuf, Failure> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            }
        })
        .ok_or_else(|| {
            Failure::unavailable(LOCAL_IO.code, "no state directory for archive staging")
                .remedy(LOCAL_IO.remedy)
        })?;
    Ok(base.join("ds").join("project-delete"))
}

// ---------------------------------------------------------------------------
// Native adapters
// ---------------------------------------------------------------------------

enum NativeGateway {
    Device(Box<super::device::DeviceSession>),
    User(Box<super::NativeClient>),
}

impl NativeGateway {
    fn open(lane: super::Lane) -> Result<Self, Failure> {
        if let Some(device) = super::restored_device_session(lane)? {
            return Ok(Self::Device(Box::new(device)));
        }
        let profile = super::profile::load(lane)?;
        let store = super::NativeRefreshStore::open()?;
        let mut client = super::Client::new(profile, super::NativeTransport, store);
        super::require_restore_before_context(&mut client)?;
        Ok(Self::User(Box::new(client)))
    }
}

impl Gateway for NativeGateway {
    fn call(&mut self, command: &Door) -> Result<Value, Failure> {
        let result = match self {
            Self::Device(device) => device.project_deletion(command),
            Self::User(client) => client.project_deletion(command, super::now()),
        };
        result.map_err(map_error)
    }
}

/// A route refusal as the refusal this command declares for its code.
pub(crate) fn map_error(error: ClientError) -> Failure {
    let Some(refusal) = error.service_refusal() else {
        return super::map_client(error);
    };
    let code = refusal.code().unwrap_or("");
    let message = refusal
        .message()
        .unwrap_or("the project delete was refused")
        .to_owned();
    // The route's own codes, each the refusal this command declares for it.
    let failure = match code {
        "project_delete_forbidden" => {
            Failure::unauthorized(FORBIDDEN.code, message).remedy(FORBIDDEN.remedy)
        }
        "project_delete_invalid" => Failure::invalid(INVALID.code, message).remedy(INVALID.remedy),
        "project_delete_not_found" => {
            Failure::invalid(NOT_FOUND.code, message).remedy(NOT_FOUND.remedy)
        }
        "project_delete_plan_changed" => {
            Failure::conflict(PLAN_CHANGED.code, message).remedy(PLAN_CHANGED.remedy)
        }
        "project_delete_blocked" => Failure::conflict(BLOCKED.code, message).remedy(BLOCKED.remedy),
        "project_delete_archive_required" => {
            Failure::conflict(ARCHIVE_REQUIRED.code, message).remedy(ARCHIVE_REQUIRED.remedy)
        }
        "project_delete_archive_mismatch" => {
            Failure::conflict(ARCHIVE_MISMATCH.code, message).remedy(ARCHIVE_MISMATCH.remedy)
        }
        "project_delete_in_progress" => {
            Failure::conflict(IN_PROGRESS.code, message).remedy(IN_PROGRESS.remedy)
        }
        "project_delete_unavailable" => {
            Failure::unavailable(UNAVAILABLE.code, message).remedy(UNAVAILABLE.remedy)
        }
        "project_delete_guard" => Failure::failed(GUARD.code, message).remedy(GUARD.remedy),
        _ => return super::map_client(error),
    };
    let mut details = Map::new();
    details.insert("http_status".into(), json!(refusal.status()));
    for (key, value) in refusal.details() {
        let value = match value {
            ds_client_core::ServiceRefusalDetail::Text(text) => json!(text),
            ds_client_core::ServiceRefusalDetail::Integer(integer) => json!(integer),
            ds_client_core::ServiceRefusalDetail::Flag(flag) => json!(flag),
        };
        details.insert(key.clone(), value);
    }
    failure.detail(Value::Object(details))
}

struct HttpBytes;

impl Bytes for HttpBytes {
    fn download(&mut self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<u64, Failure> {
        if !url.starts_with("https://") {
            return Err(Failure::unavailable(
                UNREADABLE_REFUSAL.code,
                "a download URL is not https",
            ));
        }
        let seconds = 120 + limit / (1024 * 1024);
        let mut response = ureq::get(url)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(seconds)))
            .build()
            .call()
            .map_err(|e| {
                Failure::unavailable(TRANSIENT_REFUSAL.code, e.to_string())
                    .remedy(TRANSIENT_REFUSAL.remedy)
            })?;
        if response.status() != 200 {
            return Err(Failure::unavailable(
                DOWNLOAD_MISMATCH.code,
                format!("download answered {}", response.status()),
            )
            .remedy(DOWNLOAD_MISMATCH.remedy));
        }
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(limit.max(1))
            .reader();
        std::io::copy(&mut reader, sink).map_err(|e| {
            Failure::unavailable(DOWNLOAD_MISMATCH.code, e.to_string())
                .remedy(DOWNLOAD_MISMATCH.remedy)
        })
    }

    fn upload(
        &mut self,
        session: &str,
        path: &Path,
        size: u64,
        sha256: &str,
    ) -> Result<bool, Failure> {
        use ds_sync_runtime::native_transfer::{NativeTransferOutcome, drive_verified_output};
        let file = File::open(path).map_err(|e| io(e, path))?;
        let result = drive_verified_output(
            session,
            size,
            sha256,
            file,
            None,
            None,
            &|| false,
            &mut |_| {},
        )
        .map_err(|e| Failure::unavailable(UPLOAD_FAILED.code, e).remedy(UPLOAD_FAILED.remedy))?;
        match result.outcome {
            NativeTransferOutcome::Done => Ok(true),
            NativeTransferOutcome::Reopen => Ok(false),
            NativeTransferOutcome::Failed if result.retryable => Ok(false),
            NativeTransferOutcome::Failed => Err(Failure::unavailable(
                UPLOAD_FAILED.code,
                format!("the upload failed: {:?}", result.cause),
            )
            .remedy(UPLOAD_FAILED.remedy)),
        }
    }
}

pub fn render(data: &Value) -> String {
    let plan = &data["plan"];
    let mut out = format!(
        "{}  {}\n",
        plan["project_id"].as_str().unwrap_or("project"),
        plan["plan_digest"].as_str().unwrap_or("")
    );
    for disposition in ["archive", "delete_only", "retained", "blocked"] {
        let totals = &plan["totals"][disposition];
        out.push_str(&format!(
            "  {disposition:<12} {:>8} members  {:>12} bytes\n",
            totals["count"].as_i64().unwrap_or(0),
            totals["bytes"].as_i64().unwrap_or(0)
        ));
    }
    for group in plan["groups"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "    {:<11} {:>7}  {:<10} {}\n",
            group["disposition"].as_str().unwrap_or(""),
            group["count"].as_i64().unwrap_or(0),
            group["store"].as_str().unwrap_or(""),
            group["location"].as_str().unwrap_or("")
        ));
    }
    for blocker in plan["blockers"].as_array().into_iter().flatten() {
        out.push_str(&format!("  BLOCKED {}\n", blocker.as_str().unwrap_or("")));
    }
    out.push_str(&format!(
        "  archive -> gs://{}/{}\n",
        plan["archive"]["bucket"].as_str().unwrap_or(""),
        plan["archive"]["object"].as_str().unwrap_or("")
    ));
    if let Some(receipt) = data.get("archive_receipt") {
        out.push_str(&format!(
            "  archived  generation {}  {} bytes  sha256 {}\n",
            receipt["generation"],
            receipt["size"],
            receipt["sha256"].as_str().unwrap_or("")
        ));
    }
    if let Some(apply) = data.get("apply") {
        out.push_str(&format!(
            "  {}  deleted {}\n",
            apply["state"].as_str().unwrap_or(""),
            apply["deleted"]
        ));
    }
    out
}

#[cfg(test)]
#[path = "project_delete_tests.rs"]
mod tests;
