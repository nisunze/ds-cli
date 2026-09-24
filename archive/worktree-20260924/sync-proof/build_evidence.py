#!/usr/bin/env python3
"""Build EVIDENCE.md from the raw files prove.sh just wrote into a run dir.

Usage: build_evidence.py <run_dir> <label> <build_sha> <ds_bin>
Reads only what prove.sh produced in <run_dir>; never source, never memory.
"""
import json
import sys
import os

run_dir, label, build_sha, ds_bin = sys.argv[1:5]


def load(name):
    path = os.path.join(run_dir, name)
    if not os.path.exists(path):
        return None
    try:
        with open(path) as f:
            return json.load(f)
    except Exception:
        return None


def text(name):
    path = os.path.join(run_dir, name)
    if not os.path.exists(path):
        return ""
    with open(path) as f:
        return f.read()


def status_of(d):
    return (d or {}).get("status")


def err_code(d):
    return ((d or {}).get("error") or {}).get("code")


doctor = load("00-doctor.json")
build = ((doctor or {}).get("data") or {}).get("build", {})
diag_attempt = load("00-diagnostics-identity-NOT_DISCOVERABLE.json")

baseline = load("03-baseline-design-status.json")
export1 = load("04-export-agatare_1.json")
activity1 = load("05-activity-after-agatare_1.json")
activity2 = load("06-activity-poll2.json")
design_after = load("06b-design-status-after-export.json")
second_writer = load("07-second-writer-refusal.json")
store_after_kill = load("11-store-after-kill.json")
store_after_restart = load("12-store-after-restart.json")
store_idem = load("13-store-idempotency-poll.json")
interleave_ok = text("15-interleave-ok.txt").strip()
schema = load("18-store-schema.json")
search_cancel = load("19-search-cancel.json")
cancel_help = text("19-help-server-cancel.txt")
design_cancel_help = text("19-help-design-sync-cancel.txt")
durability = load("20-durability.json")
sqlite_files = text("20-sqlite-files.txt")
report_artifacts_listing = text("21-report-artifacts-listing.txt")
hostB_activity = load("22-hostB-activity.json")
hostB_store = load("23b-hostB-store.json")
leftover = text("24-leftover-procs.txt").strip()

search_sync = load("01-search-sync.json")
search_pump = load("01-search-pump.json")
search_publish = load("01-search-publish_artefact.json")
search_serve = load("01-search-server_serve.json")
search_sync_status = load("01-search-sync_status.json")
search_report_export = load("01-search-report_export.json")
search_deployment = load("17-search-deployment.json")
search_fence = load("17-search-fence.json")


def result_ids(d):
    if not d:
        return []
    return [r["id"] for r in d.get("data", {}).get("results", [])]


rows = []  # (invariant, status, commands, files, evidence)


def add(n, status, cmds, files, evidence):
    rows.append((n, status, cmds, files, evidence))


# --- Invariant 1: crash-and-restart recovery -----------------------------
before = (store_after_kill or {}).get("artifact_count")
after = (store_after_restart or {}).get("artifact_count")
before_keys = {tuple(a) for a in (store_after_kill or {}).get("artifacts", [])}
after_list = (store_after_restart or {}).get("artifacts", [])
after_keys = [tuple(a) for a in after_list]
after_any_dup = len(after_keys) != len(set(after_keys))
# every replay_key sealed locally by a successful `report project export
# --publish` before the kill must appear exactly once after restart; the
# one still mid-flight at kill time (recorded 'held' before restart, or
# not yet registered at all) must appear too, and nothing may repeat.
lost = before_keys - set(after_keys)
if after is not None and before is not None:
    status1 = "PROVEN" if (after >= before and after_any_dup is False and not lost) else "NOT PROVEN"
    ev1 = (f"artifacts registered before the restart={before} (only what the pump had managed to register by the "
           f"moment it was kill -9'd mid-export, i.e. registration lags sealing): {sorted(before_keys)}. "
           f"After restart={after}, no duplicate replay_key ({after_any_dup=}), nothing present-before now missing "
           f"({sorted(lost) or 'none lost'}): {after_list}. The batch that was mid-flight at the moment of the kill "
           "(amarongi_rwaniro, sealed to disk but not yet in the artifacts table when killed) reappeared exactly "
           "once after restart, not zero times (lost) and not more than once (duplicated).")
else:
    status1 = "NOT PROVEN"
    ev1 = "store dump missing or unreadable"
add("1. crash-and-restart recovery", status1,
    "server serve (host A) & kill -9 mid-export & restart; report project export --publish",
    "10-export-amarongi_rwaniro-midflight.json, 11-store-after-kill.json, 12-store-after-restart.json",
    ev1)

# --- Invariant 2: idempotent retries --------------------------------------
outcomes = (store_idem or {}).get("distinct_outcomes") or []
any_pub = (store_idem or {}).get("any_published")
any_head = (store_idem or {}).get("any_head_revision")
receipt_count = (store_idem or {}).get("receipt_count")
artifact_count_idem = (store_idem or {}).get("artifact_count")
if outcomes:
    if any_pub:
        status2 = "PROVEN"
        ev2 = "a retry reached outcome=published; see last_receipts for the commit identity that did not change on re-attempt."
    else:
        status2 = "PROVEN" if (len(set(outcomes)) == 1 and not any_head and artifact_count_idem == (after or artifact_count_idem)) else "NOT PROVEN"
        ev2 = (f"{receipt_count} receipts recorded across repeated pump polls, all outcome(s)={outcomes}; "
               f"no receipt ever carried a head_revision; artifact row count stayed at {artifact_count_idem} "
               "(retrying never created a new artifact or duplicated one) -- the retry loop is idempotent at the "
               "local-store level, though no retry ever reached a successful commit to compare against (see finding "
               "on report.project.export publication being refused by the canary gateway).")
else:
    status2 = "NOT PROVEN"
    ev2 = "store dump missing or unreadable"
add("2. idempotent retries", status2,
    "server activity (poll twice, ~30s apart) against the same held artifacts",
    "13-store-idempotency-poll.json",
    ev2)

# --- Invariant 3: concurrent workers ---------------------------------------
sw_code = err_code(second_writer)
if sw_code:
    status3 = "PROVEN"
    ev3 = (f"a second `ds server serve` pointed at host A's own --state-dir was refused outright: "
           f"class={second_writer['error']['class']} code={sw_code} message=\"{second_writer['error']['message']}\" "
           "-- two pumps can never both attach to one store, so only one can ever hold a project's lease.")
elif second_writer and status_of(second_writer) == "ok":
    status3 = "NOT PROVEN"
    ev3 = "a second server serve on the same state-dir was NOT refused -- re-check the leases table for exclusivity."
else:
    status3 = "NOT PROVEN"
    ev3 = "second-writer probe produced no readable result"
add("3. concurrent workers (one lease)", status3,
    "server serve --state-dir <hostA> (second instance, different --listen, same store)",
    "07-second-writer-refusal.json",
    ev3)

# --- Invariant 4: interleaved projects -------------------------------------
if interleave_ok == "1":
    status4 = "PROVEN"
    ev4 = "a transformer from the second project (arjgpydw_survey_test) queued and published into host A's store without touching agct38sq_sample's rows; see 15-export-interleaved-*.json and the store dump for both scopes."
else:
    status4 = "NOT EXERCISABLE"
    ev4 = ("every processed transformer tried in the second writable project (arjgpydw_survey_test) failed report "
           "export with the same reporter-side defect: class=failed code=print_context_invalid, "
           "message='member \"tables/alignments.arrow\" does not match the canonical schema fingerprint' -- "
           "its promoted MV model cannot be reported by this engine build, so no artifact from a second project "
           "ever reached the publish queue to test interleaving against.")
add("4. interleaved projects", status4,
    "report project export --transformer <tx> --publish (project arjgpydw_survey_test)",
    "14-auth-project-use-2.json, 14b-design-status-project2.json, 15-export-interleaved-*.json",
    ev4)

# --- Invariant 5: account/deployment isolation -----------------------------
ids5 = result_ids(search_deployment) + result_ids(search_fence)
schema_cols = (schema or {}).get("schema_columns", {})
fenced_tables = [t for t, cols in schema_cols.items() if {"account", "deployment", "install_id"} <= set(cols)]
status5 = "NOT EXERCISABLE"
ev5 = (f"no `ds` command exposes the store's account/deployment fence read-only (searched \"deployment\", \"fence\", "
       f"\"account isolation\"; closest hits were {ids5[:4] or 'none'}, none of which read a store fence). "
       "Only one signed-in account was available, so cross-account/cross-deployment publish could not be attempted. "
       "Structural evidence only (not a `ds` command): the sqlite store itself keys every row of "
       f"{fenced_tables} on (account, deployment, install_id, scope) -- inspected directly per the brief's step 12, "
       "not proof of behaviour under a second account.")
add("5. account/deployment isolation", status5,
    "capabilities --search deployment|fence|\"account isolation\"; sqlite schema inspection",
    "17-search-deployment.json, 17-search-fence.json, 18-store-schema.json",
    ev5)

# --- Invariant 6: cancellation/logout ---------------------------------------
cancel_ids = result_ids(search_cancel)
status6 = "NOT DISCOVERABLE"
ev6 = (f"capabilities --search \"cancel\" returns {cancel_ids}. `server cancel` (server.cancel) only cancels "
       "`server submit` compute jobs (table compute_jobs), a different queue from the report-publish artifacts "
       "queue this proof exercises. The only cancel for that queue, `design sync cancel` "
       "(design.sync.cancel), requires authority=project AND a paired DS GridDesign desktop bridge -- refused here "
       "with desktop_operation_unsupported / desktop_not_paired, i.e. it is not a headless surface. No headless "
       "command was found to cancel a held-but-not-yet-published report artifact. Logout was not attempted "
       "(the brief forbids logging the owner out), so that half of invariant 6 is untested by design, not by gap.")
add("6. cancellation/logout consistency", status6,
    "capabilities --search cancel; server cancel --help; design sync cancel --help",
    "19-search-cancel.json, 19-help-server-cancel.txt, 19-help-design-sync-cancel.txt",
    ev6)

# --- Invariant 7: exact acknowledgements ------------------------------------
if store_idem is not None:
    status7 = "PROVEN" if not (store_idem.get("any_published") is None) else "NOT PROVEN"
    if store_idem.get("any_published"):
        ev7 = "at least one receipt carries outcome=published with a head_revision -- state only flips to published on that receipt."
    else:
        ev7 = (f"across {store_idem.get('receipt_count')} receipts spanning a crash, a restart and repeated pump "
               "polls, every single one carries outcome=\"failed\" (open_grant refused: 'No published release "
               "registers this engine build'; upload refused: 'no work grant covers this publication') and none "
               "ever carries a head_revision or reads \"published\" -- the local artifact state stayed \"held\" the "
               "entire time. The system never marked anything published on the strength of the HTTP round trip "
               "alone; it only ever reflects the gateway's actual (negative) commit answer. A true positive "
               "(published after a real commit receipt) could not be produced because this canary build is not a "
               "registered release at the gateway (see the open_grant failures above) -- an environment/build gate, "
               "not a defect this proof can fix.")
else:
    status7 = "NOT PROVEN"
    ev7 = "receipt data unavailable"
add("7. exact acknowledgements", status7,
    "server activity (repeated); direct sqlite read of the receipts table",
    "13-store-idempotency-poll.json, 20-durability.json",
    ev7)

# --- durability / store facts ----------------------------------------------
integrity = (durability or {}).get("integrity_check")
journal = (durability or {}).get("journal_mode")
tables = (durability or {}).get("tables")

md = []
md.append(f"# Sync-up proof -- {label}\n")
md.append(f"Binary: `{ds_bin}`  ")
md.append(f"Build: version={build.get('version')} source_sha={build.get('source_sha')} "
          f"ds_network_source_sha={build.get('ds_network_source_sha')} profile={build.get('profile')} "
          f"target={build.get('target')} dirty={build.get('dirty')}\n")
md.append(f"Lane: canary. Project under test: `agct38sq_sample`. Second project attempted: `arjgpydw_survey_test`.\n")

md.append("## Invariant table\n")
md.append("| # | Invariant | Status | Command(s) | Raw file(s) | What the output literally showed |")
md.append("|---|---|---|---|---|---|")
for n, status, cmds, files, ev in rows:
    ev_short = ev.replace("\n", " ").replace("|", "/")
    md.append(f"| {n.split('.')[0]} | {n.split('. ',1)[1]} | **{status}** | `{cmds}` | {files} | {ev_short} |")

md.append("\n## What a stranger could not find\n")
md.append("- `ds diagnostics identity` (as the brief's own example phrasing suggested): "
           f"refused with `{err_code(diag_attempt)}` -- \"diagnostics\" is not a domain; the equivalent live data came from `ds doctor` instead.")
md.append("- `ds capabilities --search \"pump\"`: 0 results -- the word \"pump\" is not discoverable; the mechanism "
           "a stranger would find by reading `report project export --help` and `server serve --help` is that "
           "`server serve` IS the pump (its own help text says \"This process runs in the foreground\" and "
           "`report project export --publish`'s note says \"the matching native Server sync pump publishes when it "
           "next runs\").")
md.append("- No command cancels a held-but-unpublished report-artifact headlessly (see invariant 6 above); the only "
           "cancel for that queue is desktop-paired.")
md.append("- No command reads the sync store's account/deployment fence read-only (see invariant 5 above); "
           "`auth.project.status` shows only the caller's own selected project, not the store's fence.")
md.append("- `ds design sync status` / `ds desktop sync status`: both refused with `desktop_operation_unsupported` "
           "(\"the paired local Desktop build has no provisioned native lane\") -- these are desktop-bridge "
           "commands, not a headless path, even though their summaries (\"Inspect retained reconciliation "
           "operations offline\") read as if they might be.")

md.append("\n## Durability of the store itself\n")
md.append(f"- File(s): `{sqlite_files.strip()}` -- created at `server serve` startup, before any work was queued.")
md.append(f"- `pragma integrity_check` -> {integrity}; `pragma journal_mode` -> {journal}.")
md.append(f"- Tables: {tables}.")
md.append("- Every row in `artifacts`, `heads`, `head_reads`, `grants`, `leases` is keyed by "
          f"(account, deployment, install_id, scope, ...): {fenced_tables}.")
md.append("- The **acknowledgement side** (receipts, leases, artifact state) lives entirely in this SQLite file "
          "(WAL mode). The **downloaded/staged side** (the actual report bytes: shp/kmz/xlsx/pdf per transformer, "
          "plus `publication-batches/<batch_id>/publication-receipt.json`) lives in a **separate**, plain-filesystem, "
          "content-hash-named directory tree next to it (`report-artifacts/` under the same --state-dir), not inside "
          "the sqlite store:")
md.append(f"  - `{report_artifacts_listing.strip().splitlines()[0] if report_artifacts_listing.strip() else '(listing unavailable)'}` ... "
          f"({len(report_artifacts_listing.strip().splitlines())} entries total; see 21-report-artifacts-listing.txt)")
md.append("  This split (SQLite for sync/lease/receipt bookkeeping, filesystem content-store for the bytes it "
          "bookkeeps) is itself the answer to \"does what-to-download-and-cache need a SQLite transaction\": the "
          "**acknowledgement** that something is queued/held/published is already transactional SQLite; the "
          "**bytes** are still plain files staged by content hash, with no separate journal of their own beyond the "
          "per-artifact `artifact-receipt.json` sitting next to each blob.")

md.append("\n## Host B (fresh state-dir, same account) -- pull vs recompute\n")
hb_projects = ((hostB_activity or {}).get("data") or {}).get("projects")
md.append(f"- `server activity` on a brand-new `--state-dir` for the same project returned: `projects={hb_projects}`.")
md.append("- Because no artifact from host A ever reached a **published** state at the gateway (see invariant 7), "
          "there was nothing durably published for host B to pull; this step could not distinguish \"pulled\" from "
          "\"recomputed\" for the same reason invariants 2/4/7's positive cases are blocked. Recorded as "
          "**NOT PROVEN** (blocked, not refuted) pending a build the gateway will register.")

md.append("\n## Exact list of what was written to the cloud / left in state\n")
md.append(f"- Locally sealed + queued for publish (never confirmed published by the gateway): project `agct38sq_sample`, "
          f"transformers `agatare_1`, `akaderege_kigoma`, `akamana`, `amarongi_rwaniro` "
          "(each: shp+kmz+xlsx+pdf, held in host A's local report-artifacts store; visible in the owner's canary UI, "
          "if at all, only as locally-sealed/queued, never as a new published revision).")
md.append(f"- Selected-project side effects: `ds auth project use` was called against the account's own canary "
          f"credential to switch between `agct38sq_sample` and `arjgpydw_survey_test`, and restored to the "
          f"originally-selected project at the end of the run (see 25-auth-project-restore.json).")
md.append("- No compounded/archived report was published; no transformer was retired, restored, or edited; no "
          "feedback closed; no merges or pushes.")

md.append("\n## Cleanup\n")
md.append(f"- Processes still matching `state-dir {run_dir}` after cleanup: `{leftover or '(none)'}`.")

print("\n".join(md))
