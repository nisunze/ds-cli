//! `ds dsgrid-exchange sync` — write a working copy's edits back INTO the
//! PLS-CADD workspace it is linked to.
//!
//! This is the other write in the domain, and a narrower one than
//! `convert`: it never creates a folder. It rewrites, in place, exactly the
//! members whose engineering changed in DS — the DON for placements,
//! retypes and structure descriptions, a definition for its description
//! line, the XYZ for terrain — and touches nothing else. `.fea .pps .brk
//! .tin`, attachments, logos, the previous DON stay the bytes PLS-CADD
//! wrote, and the DON keeps the home it recorded, so PLS-CADD opens the
//! workspace without a "project moved" dialog.
//!
//! Three properties make it safe to point at the owner's real folder:
//!
//! * **The folder is re-digested first.** The link pinned the member tree;
//!   a folder whose bytes moved since — PLS-CADD saved, a file appeared —
//!   is refused before anything is planned. Nothing is ever merged.
//! * **The engine proves the edits twice.** The export planner verifies the
//!   edit set on the healed tree it was built for; the sync applies the same
//!   surgical writers to the original bytes and cross-checks every structure
//!   record and the DON home against that verified result.
//! * **`--dry-run` is the same plan.** It returns the member-level diff the
//!   write would produce, computed by the same call; only the file writes
//!   are skipped. A write needs `--yes`; both together is a refusal.
//!
//! Every write is atomic per member (temp file + rename in the workspace
//! directory), rewritten members are written in one pass, and a member that
//! fails to write stops the pass with the members already written named.
//! `--container bak` frames the synced tree as one exact-byte `.bak` beside
//! the folder instead of writing into it.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_dsgrid::model::pls_source::{
    MODEL_NOT_FROM_PLS, WORKSPACE_DIGEST_MOVED, WORKSPACE_NOT_FOUND, WORKSPACE_NOT_LINKED,
    WORKSPACE_NOT_THIS_PACKAGE,
};
use ds_cli_dsgrid::model::{pls_source, workspace};
use ds_grid_exchange::pls_cadd_workspace_sync::{
    SyncAction, WorkspaceSyncError, WorkspaceSyncInput, WorkspaceSyncPlan, plan_workspace_sync,
};
use ds_io::{PLS_CADD_DETERMINISTIC_BAK_TIMESTAMP, PLS_CADD_DETERMINISTIC_BAK_USER};
use serde_json::{Value, json};

use crate::{refusals, render, request};

/// Diff rows printed by default. A real workspace has a few dozen members;
/// the rewritten ones always come first, so the cut never hides a write.
const MEMBER_LIMIT: usize = 100;

pub static COMMAND: Command = Command {
    id: "dsgrid-exchange.sync",
    path: &["dsgrid-exchange", "sync"],
    contract: 1,
    summary: "Write a working copy's edits back into its linked PLS-CADD workspace.",
    purpose: "\
Rewrites, in place, only the members of the linked PLS-CADD 16.81 workspace \
whose engineering changed in the working copy — DON 57 (retypes, moves, \
structure descriptions in comment slot 1), STRUCT 13 (a definition's \
description line), XYZ 5 (terrain) — and leaves every other member \
byte-identical (.fea .pps .brk .tin, attachments, libraries), so PLS-CADD \
opens the workspace at its recorded home with no \"moved\" dialog. The folder \
is re-digested against the link first; the engine verifies the edit set on \
the healed export tree and cross-checks every structure record and the DON \
home in the patched original. --dry-run returns the same diff without \
writing. FEA/CRI pass through until contract 03; whole-design, capacity and \
STR spotting edits refuse by name. --into writes into a copy of the workspace.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<model-id>",
            "The linked working copy, by the id `ds dsgrid model list` reports.",
        )
        .required(),
        Arg::value(
            "into",
            "<folder>",
            "Write into this copy of the linked workspace instead (it must digest to the link).",
        ),
        Arg::value(
            "container",
            "<kind>",
            "Write members into the workspace folder, or frame the synced tree as one exact-byte .bak.",
        )
        .default("folder")
        .choices(request::CONTAINERS),
        Arg::value(
            "out",
            "<new.bak>",
            "With --container bak: the absent backup path, outside the workspace.",
        ),
        Arg::switch("dry-run", "Plan and verify; print the diff; write nothing."),
        workspace::LANE_ARG,
        workspace::ACCOUNT_ARG,
    ],
    output: "\
`mode`, `workspace`, `pinned_digest`, `pls_version`, the DON `home`, \
`edit_classes`, `structures_touched`, `summary` {rewritten, unchanged, \
added}, `members` rows {member, read/written TYPE VERSION UNITS, action, \
bytes and sha256 before/after, notes}, `unresolved_references` [{member, \
reference, class}], `verification`, `written`, `warnings`.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange sync --model local-5ff16cd0a3d6416b --account <uid> --dry-run --output json",
            note: "The diff a write would produce: which members, which versions, why.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange sync --model local-5ff16cd0a3d6416b --account <uid> --yes --output json",
            note: "Write into the linked workspace. Close PLS-CADD first.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange sync --model local-5ff16cd0a3d6416b --account <uid> --into /srv/sandbox/Nyamagabe --yes",
            note: "Write into a copy of the workspace that still digests to the link.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    search: &["write back", "pls-cadd", "in place", "round trip", "export workspace", "delta"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

const SYNC_OWN: &[Refusal] = &[
    Refusal {
        code: "confirmation_required",
        when: "neither --dry-run nor --yes was supplied",
        remedy: "run --dry-run first; then repeat the same invocation with --yes",
    },
    Refusal {
        code: "mode_conflict",
        when: "--dry-run and --yes were both supplied",
        remedy: "choose exactly one mode",
    },
    WORKSPACE_NOT_LINKED,
    WORKSPACE_DIGEST_MOVED,
    WORKSPACE_NOT_THIS_PACKAGE,
    WORKSPACE_NOT_FOUND,
    MODEL_NOT_FROM_PLS,
    Refusal {
        code: "member_version_unsupported",
        when: "the workspace's DON is not DON 57 (member and version named)",
        remedy: "this sync writes the 16.81 member schemas only",
    },
    Refusal {
        code: "pls_version_writer_missing",
        when: "the workspace declares PLS-CADD 20.01 (or any version but 16.81)",
        remedy: "no 20.01 writer exists; keep the delivery workspace at 16.81",
    },
    Refusal {
        code: "sync_edit_class_unsupported",
        when: "the edits include a class with no in-place writer yet (whole-design, section projection, capacity values, STR spotting)",
        remedy: "keep to retypes, moves, descriptions and terrain, or export a new folder with `convert --target pls-folder`",
    },
    Refusal {
        code: "sync_edit_unrepresentable",
        when: "the export planner refuses an edit no characterized writer can express",
        remedy: "read detail.offenders; revise the edit in the working copy",
    },
    Refusal {
        code: "sync_verification_failed",
        when: "the patched DON disagrees with the verified export, or its home would change",
        remedy: "report this with the receipt; nothing was written",
    },
    Refusal {
        code: "workspace_open",
        when: "the workspace carries a PLS-CADD lock or the DS open marker",
        remedy: "close PLS-CADD and retry",
    },
    Refusal {
        code: "output_required",
        when: "--container bak without --out",
        remedy: "name an absent .bak path outside the workspace",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new .bak path; nothing is overwritten",
    },
    Refusal {
        code: "output_unwritable",
        when: "a rewritten member or the backup cannot be written",
        remedy: "check free space and permissions; detail names members already written",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the store holds bytes this build's engine cannot open as a package",
        remedy: "re-import the package",
    },
    ds_cli_dsgrid::folder::TOO_LARGE,
    ds_cli_dsgrid::folder::UNREADABLE,
];

static REFUSALS: [Refusal; SYNC_OWN.len() + workspace::REFUSALS.len()] =
    refusals::splice(&[SYNC_OWN, workspace::REFUSALS]);

enum Mode {
    DryRun,
    Write,
}

fn mode(inputs: &Inputs, context: &Context) -> Result<Mode, Failure> {
    match (inputs.switch("dry-run"), context.confirmed) {
        (true, false) => Ok(Mode::DryRun),
        (false, true) => Ok(Mode::Write),
        (true, true) => Err(Failure::invalid(
            "mode_conflict",
            "--dry-run and --yes cannot be combined",
        )
        .remedy("choose exactly one mode")),
        (false, false) => Err(Failure::invalid(
            "confirmation_required",
            "choose a non-writing dry run or confirm the write into the workspace",
        )
        .remedy("run with --dry-run first; then repeat with --yes")),
    }
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let mode = mode(inputs, context)?;
    let id = inputs.require("model")?.trim().to_owned();
    let container = inputs.value("container").unwrap_or("folder");
    let backup_out = match (container, inputs.value("out")) {
        ("bak", Some(out)) => Some(checked_backup_output(out)?),
        ("bak", None) => {
            return Err(Failure::invalid(
                "output_required",
                "--container bak needs --out <new.bak>",
            )
            .remedy("name an absent .bak path outside the workspace"));
        }
        _ => None,
    };

    let opened = pls_source::open(inputs, &id)?;
    let link = opened.row.pls_source.clone().ok_or_else(|| {
        Failure::invalid(
            "workspace_not_linked",
            format!("`{id}` is not linked to a PLS-CADD workspace"),
        )
        .remedy(WORKSPACE_NOT_LINKED.remedy)
        .next(format!(
            "ds dsgrid model link --model {id} --workspace <folder>"
        ))
    })?;
    let source = pls_source::pls_source(&id, &opened.package)?;
    let target = inputs
        .value("into")
        .unwrap_or(link.path.as_str())
        .to_string();
    let workspace_read = pls_source::read_workspace(&target)?;
    if matches!(mode, Mode::Write) && backup_out.is_none() {
        ensure_workspace_closed(&workspace_read.path)?;
    }

    let plan = plan_workspace_sync(&WorkspaceSyncInput {
        snapshot: &opened.package.snapshot,
        bindings: &source.bindings,
        preserved_members: &source.preserved_members,
        original_members: &source.original_members,
        workspace_members: &workspace_read.members,
        ingest_options: source.ingest_options.clone(),
        pinned_digest: &link.digest,
    })
    .map_err(|error| sync_failure(error, &id, &target))?;

    let mut warnings = Vec::new();
    if let Some(hint) = &workspace_read.streamed_volume {
        warnings.push(format!("streamed_volume: {hint}"));
    }
    for row in &plan.members {
        if row.action == SyncAction::Unchanged && !row.notes.is_empty() {
            warnings.push(format!("{}: {}", row.member, row.notes.join("; ")));
        }
    }

    let (mode_token, written) = match (mode, backup_out) {
        (Mode::DryRun, _) => ("dry_run", Vec::new()),
        (Mode::Write, Some(out)) => {
            let archive_name = out
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("sync.bak")
                .to_string();
            let bytes = plan
                .backup_container(
                    &workspace_read.members,
                    &archive_name,
                    PLS_CADD_DETERMINISTIC_BAK_USER,
                    PLS_CADD_DETERMINISTIC_BAK_TIMESTAMP,
                )
                .map_err(|detail| {
                    Failure::failed(
                        "output_unwritable",
                        "the backup container could not be framed",
                    )
                    .remedy("read detail")
                    .detail(json!({ "detail": detail }))
                })?;
            write_new(&out, &bytes)?;
            warnings.push(
                "container bak: PLS-CADD Restore lands the members at a new home; expect the \"project moved\" dialog after Restore — the folder container is the no-dialog path"
                    .to_string(),
            );
            ("backup", vec![out.to_string_lossy().into_owned()])
        }
        (Mode::Write, None) => ("written", write_members(&workspace_read.path, &plan)?),
    };

    Ok(project(
        &plan,
        &id,
        &workspace_read,
        &link.digest,
        mode_token,
        written,
        warnings,
    ))
}

fn sync_failure(error: WorkspaceSyncError, id: &str, target: &str) -> Failure {
    match error {
        WorkspaceSyncError::DigestMoved { pinned, actual } => Failure::conflict(
            "workspace_digest_moved",
            format!("`{target}` changed since `{id}` was linked to it"),
        )
        .remedy(WORKSPACE_DIGEST_MOVED.remedy)
        .detail(json!({ "pinned": pinned, "actual": actual })),
        WorkspaceSyncError::OriginMismatch { package, pinned } => Failure::conflict(
            "workspace_not_this_package",
            format!("the link on `{id}` does not name the workspace its package came from"),
        )
        .remedy(WORKSPACE_NOT_THIS_PACKAGE.remedy)
        .detail(json!({ "package_origin_digest": package, "pinned": pinned })),
        WorkspaceSyncError::MemberVersionUnsupported {
            member,
            type_name,
            version,
            expected,
        } => Failure::invalid(
            "member_version_unsupported",
            format!("{member} is TYPE='{type_name}' VERSION='{version}'; this sync writes {expected}"),
        )
        .remedy("this sync writes the 16.81 member schemas only")
        .detail(json!({ "member": member, "type": type_name, "version": version, "expected": expected })),
        WorkspaceSyncError::PlsVersionWriterMissing { declared } => Failure::invalid(
            "pls_version_writer_missing",
            format!("the workspace declares PLS-CADD {declared}; no in-place writer exists for it"),
        )
        .remedy("no 20.01 writer exists; keep the delivery workspace at 16.81")
        .detail(json!({ "declared": declared, "writer": "16.81" })),
        WorkspaceSyncError::EditClassUnsupported { classes, offenders } => Failure::invalid(
            "sync_edit_class_unsupported",
            format!(
                "the working copy carries {} edit class(es) with no in-place writer: {}",
                classes.len(),
                classes.join(", ")
            ),
        )
        .remedy("keep this cut's edits to retypes, moves, descriptions and terrain, or export a new folder with `ds dsgrid-exchange convert --target pls-folder`")
        .detail(json!({
            "classes": classes,
            "offenders": offenders.iter().map(|offender| json!({
                "table": offender.table, "entity": offender.entity, "detail": offender.detail,
            })).collect::<Vec<_>>(),
        })),
        WorkspaceSyncError::Export(export) => {
            let offenders = match &export {
                ds_grid_exchange::pls_cadd_workspace_export::WorkspaceExportError::UnsupportedEditClass { offenders } => offenders
                    .iter()
                    .map(|offender| json!({
                        "table": offender.table, "entity": offender.entity, "detail": offender.detail,
                    }))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            Failure::invalid(
                "sync_edit_unrepresentable",
                "the export planner refused the edit set",
            )
            .remedy("read detail.offenders; revise the edit in the working copy")
            .detail(json!({ "engine": export.to_string(), "offenders": offenders }))
        }
        WorkspaceSyncError::Member { member, detail } => Failure::failed(
            "sync_verification_failed",
            format!("{member}: {detail}"),
        )
        .remedy("report this with the receipt; nothing was written")
        .detail(json!({ "member": member, "detail": detail })),
        WorkspaceSyncError::HomeChanged(detail) | WorkspaceSyncError::Verification(detail) => {
            Failure::failed("sync_verification_failed", detail)
                .remedy("report this with the receipt; nothing was written")
        }
    }
}

fn ensure_workspace_closed(root: &Path) -> Result<(), Failure> {
    if root.join(".ds-workspace-open").exists() {
        return Err(Failure::conflict(
            "workspace_open",
            "the workspace carries the DS open marker",
        )
        .remedy("close PLS-CADD and retry"));
    }
    let lock = std::fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
        });
    if let Some(lock) = lock {
        return Err(Failure::conflict(
            "workspace_open",
            format!("PLS-CADD lock present: {}", lock.display()),
        )
        .remedy("close PLS-CADD and retry"));
    }
    Ok(())
}

/// Write every rewritten member atomically into the workspace: the bytes go
/// to a temp file in the member's directory, then rename over the member.
/// A member's bytes are never half-written.
fn write_members(root: &Path, plan: &WorkspaceSyncPlan) -> Result<Vec<String>, Failure> {
    let mut written = Vec::with_capacity(plan.rewritten.len());
    for (relative, bytes) in &plan.rewritten {
        let target = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        let dir = target.parent().unwrap_or(root);
        let result = (|| -> std::io::Result<()> {
            let mut staged = tempfile::NamedTempFile::new_in(dir)?;
            staged.write_all(bytes)?;
            staged.flush()?;
            staged.as_file().sync_all()?;
            staged.persist(&target).map_err(|error| error.error)?;
            Ok(())
        })();
        if let Err(error) = result {
            return Err(Failure::failed(
                "output_unwritable",
                format!("cannot write `{}`", target.display()),
            )
            .remedy(
                "check free space and permissions; members already written are listed in detail",
            )
            .detail(json!({ "detail": error.kind().to_string(), "written": written })));
        }
        written.push(relative.clone());
    }
    Ok(written)
}

fn checked_backup_output(raw: &str) -> Result<PathBuf, Failure> {
    let output = PathBuf::from(raw);
    if output.exists() {
        return Err(
            Failure::conflict("output_exists", format!("`{raw}` already exists"))
                .remedy("choose a new .bak path"),
        );
    }
    if !output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bak"))
    {
        return Err(
            Failure::invalid("output_required", "--out must have a .bak extension")
                .remedy("name an absent .bak path outside the workspace"),
        );
    }
    Ok(if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&output))
            .unwrap_or(output)
    })
}

fn write_new(output: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| {
            Failure::failed(
                "output_unwritable",
                format!("cannot create `{}`", output.display()),
            )
            .remedy("choose a writable absent path")
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(Failure::failed(
            "output_unwritable",
            format!("could not finish writing `{}`", output.display()),
        )
        .remedy("check free space and permissions; the partial file was removed")
        .detail(json!({ "detail": error.kind().to_string() })));
    }
    Ok(())
}

fn identity_json(
    identity: Option<&ds_grid_exchange::pls_cadd_workspace_sync::NativeMemberIdentity>,
) -> Value {
    match identity {
        None => Value::Null,
        Some(identity) => json!({
            "family": identity.family,
            "version": identity.version,
            "units": identity.units,
            "label": identity.label(),
        }),
    }
}

fn project(
    plan: &WorkspaceSyncPlan,
    id: &str,
    workspace_read: &pls_source::ReadWorkspace,
    pinned_digest: &str,
    mode: &str,
    written: Vec<String>,
    warnings: Vec<String>,
) -> Value {
    // Rewritten and added rows first, then unchanged, so a cut never hides a
    // write.
    let mut rows: Vec<&_> = plan.members.iter().collect();
    rows.sort_by_key(|row| match row.action {
        SyncAction::Rewritten => 0,
        SyncAction::Added => 1,
        SyncAction::Unchanged => 2,
    });
    let total = rows.len();
    let withheld = total.saturating_sub(MEMBER_LIMIT);
    let members: Vec<Value> = rows
        .iter()
        .take(MEMBER_LIMIT)
        .map(|row| {
            json!({
                "member": row.member,
                "read": identity_json(row.read.as_ref()),
                "written": identity_json(row.written.as_ref()),
                "action": row.action.token(),
                "bytes_before": row.bytes_before,
                "bytes_after": row.bytes_after,
                "sha256_before": row.sha256_before,
                "sha256_after": row.sha256_after,
                "notes": row.notes,
            })
        })
        .collect();
    let count = |action: SyncAction| {
        plan.members
            .iter()
            .filter(|row| row.action == action)
            .count()
    };

    let mut answer = json!({
        "mode": mode,
        "model": id,
        "workspace": workspace_read.path.to_string_lossy(),
        "pinned_digest": pinned_digest,
        "pls_version": plan.pls_version,
        "home": {
            "member": plan.home.member,
            "filename_header": plan.home.filename_header,
            "home_dir": plan.home.home_dir,
        },
        "edit_classes": plan.edit_classes,
        "structures_touched": plan.structures_touched,
        "structure_types_touched": plan.structure_types_touched,
        "summary": {
            "rewritten": count(SyncAction::Rewritten),
            "unchanged": count(SyncAction::Unchanged),
            "added": count(SyncAction::Added),
            "members": total,
        },
        "members": members,
        "unresolved_references": plan.unresolved_references.iter().map(|reference| json!({
            "member": reference.containing_member,
            "reference": reference.reference,
            "class": reference.class,
        })).collect::<Vec<_>>(),
        "verification": {
            "healed_export_verified": plan.verification.healed_export_verified,
            "structures_cross_checked": plan.verification.structures_cross_checked,
            "terrain_points_cross_checked": plan.verification.terrain_points_cross_checked,
            "home_preserved": plan.verification.home_preserved,
            "members_byte_identical": plan.verification.members_byte_identical,
            "level": "proposal",
        },
        "written": written,
        "warnings": warnings,
    });
    if withheld > 0 {
        answer["more"] = json!({
            "members_withheld": withheld,
            "next": "every rewritten member is listed; the withheld rows are unchanged",
        });
    }
    answer
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}  {} → {}\n  PLS-CADD {} · home {}\n  {} rewritten · {} unchanged · {} added\n",
        data["mode"].as_str().unwrap_or(""),
        data["model"].as_str().unwrap_or(""),
        data["workspace"].as_str().unwrap_or(""),
        data["pls_version"].as_str().unwrap_or("?"),
        data["home"]["filename_header"].as_str().unwrap_or("?"),
        data["summary"]["rewritten"],
        data["summary"]["unchanged"],
        data["summary"]["added"],
    );
    if let Some(rows) = data["members"].as_array() {
        out.push('\n');
        for row in rows {
            let action = row["action"].as_str().unwrap_or("");
            if action == "unchanged" && row["notes"].as_array().is_none_or(Vec::is_empty) {
                continue;
            }
            out.push_str(&format!(
                "  {:<10} {:<36} {:<14} {:>9} → {:>9} B\n",
                action,
                row["member"].as_str().unwrap_or(""),
                row["read"]["label"].as_str().unwrap_or("opaque"),
                row["bytes_before"],
                row["bytes_after"],
            ));
            for note in row["notes"].as_array().into_iter().flatten() {
                out.push_str(&format!("             {}\n", note.as_str().unwrap_or("")));
            }
        }
        let unchanged_silent = rows
            .iter()
            .filter(|row| {
                row["action"].as_str() == Some("unchanged")
                    && row["notes"].as_array().is_none_or(Vec::is_empty)
            })
            .count();
        if unchanged_silent > 0 {
            out.push_str(&format!(
                "  {unchanged_silent} more member(s) unchanged, byte-identical\n"
            ));
        }
    }
    if let Some(refs) = data["unresolved_references"]
        .as_array()
        .filter(|refs| !refs.is_empty())
    {
        out.push_str(&format!(
            "\nUNRESOLVED EXTERNAL REFERENCES ({})\n",
            refs.len()
        ));
        for reference in refs {
            out.push_str(&format!(
                "  {:<24} {:<22} {}\n",
                reference["member"].as_str().unwrap_or(""),
                reference["class"].as_str().unwrap_or(""),
                reference["reference"].as_str().unwrap_or(""),
            ));
        }
    }
    let verification = &data["verification"];
    out.push_str(&format!(
        "\nVERIFICATION (level {})\n  export verified {} · {} structures cross-checked · home preserved {} · {} members byte-identical\n",
        verification["level"].as_str().unwrap_or("proposal"),
        verification["healed_export_verified"],
        verification["structures_cross_checked"],
        verification["home_preserved"],
        verification["members_byte_identical"],
    ));
    if let Some(written) = data["written"]
        .as_array()
        .filter(|written| !written.is_empty())
    {
        out.push_str("\nWRITTEN\n");
        for path in written {
            out.push_str(&format!("  {}\n", path.as_str().unwrap_or("")));
        }
    }
    render::list(&mut out, "WARNINGS", &data["warnings"]);
    out
}
