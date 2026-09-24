#!/usr/bin/env bash
# prove.sh <path-to-ds> [label]
#
# Blind, re-runnable proof of the sync-up claim ("an artefact computed on one
# host converges on every other host, reliably") against one installed `ds`
# build. Discovers nothing from source: only --help, `capabilities --search`,
# and the files/DB the binary itself writes.
#
# Writes runs/<label>/EVIDENCE.md plus every raw *.json/*.txt it produced.
# Re-run against a different ds build with: ./prove.sh /path/to/other-ds
set -uo pipefail

DS_BIN="${1:?usage: prove.sh <path-to-ds> [label]}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BUILD_JSON=$("$DS_BIN" doctor --output json 2>/dev/null)
BUILD_SHA=$(echo "$BUILD_JSON" | python3 -c "import json,sys;print(json.load(sys.stdin)['data']['build']['source_sha'][:7])" 2>/dev/null || echo "unknown")
LABEL="${2:-canary-$BUILD_SHA}"
RUN_DIR="$SCRIPT_DIR/runs/$LABEL"

echo "[prove.sh] ds binary: $DS_BIN"
echo "[prove.sh] label: $LABEL"
echo "[prove.sh] run dir: $RUN_DIR"

rm -rf "$RUN_DIR"
mkdir -p "$RUN_DIR/hostA" "$RUN_DIR/hostB"
chmod 700 "$RUN_DIR/hostA" "$RUN_DIR/hostB"
cd "$RUN_DIR"

LANE=canary
PROJECT=agct38sq_sample
PROJECT2=arjgpydw_survey_test
ORIGINAL_PROJECT=""   # captured below, restored at the end
TX1=agatare_1
TX2=akaderege_kigoma
TX3=akamana
TX4=amarongi_rwaniro

HOSTA="$RUN_DIR/hostA"
HOSTB="$RUN_DIR/hostB"

log() { echo "[$(date -u +%H:%M:%S)Z] $*" >&2; }
jq_status() { python3 -c "import json,sys
try:
  print(json.load(open(sys.argv[1])).get('status'))
except Exception:
  print('unreadable')" "$1" 2>/dev/null; }

attempt_export() { # tx outdir outfile server_state_dir
  local tx=$1 outdir=$2 outfile=$3 state=$4
  rm -rf "$outdir"
  local i
  for i in 1 2 3 4 5; do
    "$DS_BIN" report project export --transformer "$tx" --out-dir "$outdir" \
      --publish --server-state-dir "$state" --lane "$LANE" --output json \
      > "$outfile" 2>&1
    if [ "$(jq_status "$outfile")" == "ok" ]; then
      log "export $tx: ok (attempt $i)"
      return 0
    fi
    log "export $tx: attempt $i failed ($(python3 -c "import json;print(json.load(open('$outfile')).get('error',{}).get('code'))" 2>/dev/null))"
    rm -rf "$outdir"
    sleep 3
  done
  return 1
}

dump_store() { # sqlite_path -> json on stdout
  python3 - "$1" <<'PY'
import sqlite3, json, sys
path = sys.argv[1]
try:
    c = sqlite3.connect(path)
    tables = [r[0] for r in c.execute("select name from sqlite_master where type='table'").fetchall()]
    out = {"path": path, "tables": tables}
    out["integrity_check"] = c.execute("pragma integrity_check").fetchall()
    out["journal_mode"] = c.execute("pragma journal_mode").fetchall()
    if "artifacts" in tables:
        out["artifact_count"] = c.execute("select count(*) from artifacts").fetchone()[0]
        out["artifacts"] = c.execute("select engine,operation,state,replay_key from artifacts").fetchall()
    if "receipts" in tables:
        rows = [json.loads(r[0]) for r in c.execute("select row from receipts order by at_ms").fetchall()]
        out["receipt_count"] = len(rows)
        out["distinct_outcomes"] = sorted({r.get("outcome") for r in rows})
        out["any_published"] = any(r.get("outcome") == "published" for r in rows)
        out["any_head_revision"] = any(r.get("head_revision") for r in rows)
        out["last_receipts"] = rows[-6:]
    if "leases" in tables:
        out["leases"] = [json.loads(r[0]) for r in c.execute("select row from leases").fetchall()]
    if "meta" in tables:
        out["meta"] = c.execute("select * from meta").fetchall()
    schema = {}
    for t in tables:
        schema[t] = [r[1] for r in c.execute(f"pragma table_info({t})").fetchall()]
    out["schema_columns"] = schema
    print(json.dumps(out, indent=2, default=str))
except Exception as e:
    print(json.dumps({"error": str(e)}))
PY
}

# ---------------------------------------------------------------------------
# 0. identity / doctor
# ---------------------------------------------------------------------------
log "step 0: identity/doctor"
echo "$BUILD_JSON" > 00-doctor.json
"$DS_BIN" --version > 00-version.txt 2>&1
"$DS_BIN" diagnostics identity --output json > 00-diagnostics-identity-NOT_DISCOVERABLE.json 2>&1  # expected: unknown_domain

# ---------------------------------------------------------------------------
# 1. discovery
# ---------------------------------------------------------------------------
log "step 1: capability discovery"
for q in "sync" "publish artefact" "pump" "server serve" "sync status" "report export" "cancel" "identity" "deployment" "fence"; do
  slug=$(echo "$q" | tr ' ' '_')
  "$DS_BIN" capabilities --search "$q" --output json > "01-search-${slug}.json" 2>&1
done
"$DS_BIN" server --help > 01-help-server.txt 2>&1
"$DS_BIN" report --help > 01-help-report.txt 2>&1
"$DS_BIN" design --help > 01-help-design.txt 2>&1
"$DS_BIN" desktop --help > 01-help-desktop.txt 2>&1
"$DS_BIN" server serve --help > 01-help-server-serve.txt 2>&1
"$DS_BIN" server status --help > 01-help-server-status.txt 2>&1
"$DS_BIN" server activity --help > 01-help-server-activity.txt 2>&1
"$DS_BIN" server cancel --help > 01-help-server-cancel.txt 2>&1
"$DS_BIN" report project export --help > 01-help-report-project-export.txt 2>&1
"$DS_BIN" design status --help > 01-help-design-status.txt 2>&1
"$DS_BIN" design sync status --help > 01-help-design-sync-status.txt 2>&1
"$DS_BIN" design sync cancel --help > 01-help-design-sync-cancel.txt 2>&1
"$DS_BIN" desktop sync status --help > 01-help-desktop-sync-status.txt 2>&1

# ---------------------------------------------------------------------------
# auth: capture whatever project is selected now, so we can restore it
# ---------------------------------------------------------------------------
"$DS_BIN" auth status --lane "$LANE" --output json > 02-auth-status.json 2>&1
ORIGINAL_PROJECT=$(python3 -c "import json;print(json.load(open('02-auth-status.json'))['data']['auth_context']['selected_project']['ds_project'])" 2>/dev/null || echo "")
log "originally selected project: ${ORIGINAL_PROJECT:-<none>}"
"$DS_BIN" auth project list --lane "$LANE" --output json > 02-auth-project-list.json 2>&1
"$DS_BIN" auth project use --lane "$LANE" --project "$PROJECT" --output json > 02-auth-project-use.json 2>&1

# ---------------------------------------------------------------------------
# 2. baseline
# ---------------------------------------------------------------------------
log "step 2: baseline design status for $TX1"
"$DS_BIN" design status --lane "$LANE" --transformer "$TX1" --output json > 03-baseline-design-status.json 2>&1

# ---------------------------------------------------------------------------
# 3. host A up, compute + publish-enqueue
# ---------------------------------------------------------------------------
log "step 3: start host A pump"
nohup "$DS_BIN" server serve --lane "$LANE" --listen 127.0.0.1:19801 \
  --state-dir "$HOSTA" --workers 2 --per-project 1 --output json \
  > hostA-serve.log 2>&1 &
echo $! > hostA-serve.pid
sleep 2
cat hostA-serve.log

attempt_export "$TX1" "$HOSTA/out-$TX1" 04-export-$TX1.json "$HOSTA" || log "export $TX1 never succeeded"
"$DS_BIN" server activity --project "$PROJECT" --lane "$LANE" --state-dir "$HOSTA" --output json > 05-activity-after-$TX1.json 2>&1
sleep 6
"$DS_BIN" server activity --project "$PROJECT" --lane "$LANE" --state-dir "$HOSTA" --output json > 06-activity-poll2.json 2>&1

# ---------------------------------------------------------------------------
# 4. converge check (design status again)
# ---------------------------------------------------------------------------
"$DS_BIN" design status --lane "$LANE" --transformer "$TX1" --output json > 06b-design-status-after-export.json 2>&1

# ---------------------------------------------------------------------------
# invariant 3: concurrent pumps on one store -- second writer must be refused
# ---------------------------------------------------------------------------
log "step: invariant 3 (concurrent pumps, same state-dir)"
"$DS_BIN" server serve --lane "$LANE" --listen 127.0.0.1:19802 \
  --state-dir "$HOSTA" --workers 1 --output json \
  > 07-second-writer-refusal.json 2>&1

# ---------------------------------------------------------------------------
# invariant 1: crash-and-restart recovery
# ---------------------------------------------------------------------------
log "step: invariant 1 (crash mid-flight)"
attempt_export "$TX2" "$HOSTA/out-$TX2" 08-export-$TX2.json "$HOSTA" || true
attempt_export "$TX3" "$HOSTA/out-$TX3" 09-export-$TX3.json "$HOSTA" || true

(
  "$DS_BIN" report project export --transformer "$TX4" --out-dir "$HOSTA/out-$TX4" \
    --publish --server-state-dir "$HOSTA" --lane "$LANE" --output json \
    > 10-export-$TX4-midflight.json 2>&1
) &
EXPID=$!
sleep 0.3
kill -9 "$(cat hostA-serve.pid)" 2>/dev/null
wait $EXPID 2>/dev/null

dump_store "$HOSTA/store.sqlite" > 11-store-after-kill.json

nohup "$DS_BIN" server serve --lane "$LANE" --listen 127.0.0.1:19801 \
  --state-dir "$HOSTA" --workers 2 --per-project 1 --output json \
  > hostA-serve-restart.log 2>&1 &
echo $! > hostA-serve.pid
sleep 25
dump_store "$HOSTA/store.sqlite" > 12-store-after-restart.json

# ---------------------------------------------------------------------------
# invariant 2: idempotent retries -- let the pump retry the same held
# artifacts across at least two poll cycles and compare identity/count
# ---------------------------------------------------------------------------
log "step: invariant 2 (idempotent retries, second poll window)"
sleep 30
dump_store "$HOSTA/store.sqlite" > 13-store-idempotency-poll.json

# ---------------------------------------------------------------------------
# invariant 4: interleaved projects
# ---------------------------------------------------------------------------
log "step: invariant 4 (interleaved second project)"
"$DS_BIN" auth project use --lane "$LANE" --project "$PROJECT2" --output json > 14-auth-project-use-2.json 2>&1
"$DS_BIN" design status --lane "$LANE" --output json > 14b-design-status-project2.json 2>&1
INTERLEAVE_OK=0
for tx in $(python3 -c "
import json
d = json.load(open('14b-design-status-project2.json'))
rows = d.get('data',{}).get('transformers',[])
names = [r['name'] for r in rows if (r.get('process_metadata') or {}).get('status') == 'success']
print(' '.join(names[:4]))
" 2>/dev/null); do
  if attempt_export "$tx" "$HOSTA/out-interleaved-$tx" "15-export-interleaved-$tx.json" "$HOSTA"; then
    INTERLEAVE_OK=1
    break
  fi
done
echo "$INTERLEAVE_OK" > 15-interleave-ok.txt
"$DS_BIN" auth project use --lane "$LANE" --project "$PROJECT" --output json > 16-auth-project-use-back.json 2>&1

# ---------------------------------------------------------------------------
# invariant 5: account/deployment isolation -- read-only fence discovery only
# ---------------------------------------------------------------------------
log "step: invariant 5 (isolation, read-only discovery)"
"$DS_BIN" capabilities --search "deployment" --output json > 17-search-deployment.json 2>&1
"$DS_BIN" capabilities --search "fence" --output json > 17-search-fence.json 2>&1
"$DS_BIN" capabilities --search "account isolation" --output json > 17-search-account-isolation.json 2>&1
dump_store "$HOSTA/store.sqlite" > 18-store-schema.json  # already includes schema_columns

# ---------------------------------------------------------------------------
# invariant 6: cancellation/logout -- do NOT log out; discovery only
# ---------------------------------------------------------------------------
log "step: invariant 6 (cancellation, discovery only, no logout)"
"$DS_BIN" capabilities --search "cancel" --output json > 19-search-cancel.json 2>&1
"$DS_BIN" server cancel --help > 19-help-server-cancel.txt 2>&1
"$DS_BIN" design sync cancel --help > 19-help-design-sync-cancel.txt 2>&1

# ---------------------------------------------------------------------------
# step 12: durability of the store itself + download/cache side
# ---------------------------------------------------------------------------
log "step 12: durability"
find "$HOSTA" -name '*.sqlite*' > 20-sqlite-files.txt 2>&1
dump_store "$HOSTA/store.sqlite" > 20-durability.json
find "$HOSTA/report-artifacts" -maxdepth 1 > 21-report-artifacts-listing.txt 2>&1
ls -la "$HOSTA" > 21-hostA-state-dir-listing.txt 2>&1

# ---------------------------------------------------------------------------
# 7. host B: pull vs recompute
# ---------------------------------------------------------------------------
log "step 7: host B"
nohup "$DS_BIN" server serve --lane "$LANE" --listen 127.0.0.1:19803 \
  --state-dir "$HOSTB" --workers 2 --per-project 1 --output json \
  > hostB-serve.log 2>&1 &
echo $! > hostB-serve.pid
sleep 2
"$DS_BIN" server activity --project "$PROJECT" --lane "$LANE" --state-dir "$HOSTB" --output json > 22-hostB-activity.json 2>&1
"$DS_BIN" design status --lane "$LANE" --transformer "$TX1" --output json > 23-design-status-final.json 2>&1
dump_store "$HOSTB/store.sqlite" > 23b-hostB-store.json 2>&1

# ---------------------------------------------------------------------------
# cleanup: stop every host this script started; restore original project
# ---------------------------------------------------------------------------
log "cleanup"
kill -9 "$(cat hostA-serve.pid)" 2>/dev/null
kill -9 "$(cat hostB-serve.pid)" 2>/dev/null
sleep 1
pgrep -af "state-dir $RUN_DIR" > 24-leftover-procs.txt 2>&1
if [ -n "$ORIGINAL_PROJECT" ]; then
  "$DS_BIN" auth project use --lane "$LANE" --project "$ORIGINAL_PROJECT" --output json > 25-auth-project-restore.json 2>&1
fi

# ---------------------------------------------------------------------------
# EVIDENCE.md
# ---------------------------------------------------------------------------
log "writing EVIDENCE.md"
python3 "$SCRIPT_DIR/build_evidence.py" "$RUN_DIR" "$LABEL" "$BUILD_SHA" "$DS_BIN" > EVIDENCE.md
log "done: $RUN_DIR/EVIDENCE.md"
cat 24-leftover-procs.txt
