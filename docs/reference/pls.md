# `ds pls` — reference

Tier-4 reference. `ds pls <command> --help` is the contract.

## What this domain is

Eight of `ds-grid-tasks`' typed file tasks plus one exact-byte `ds-io` backup
writer, exposed as commands. The task crate
exists so a host does not have to know how a `.don`, a `.012` or a workspace's
reference closure is read: it takes a typed request, loads the exact bytes,
checks identity, calls the owning operation, and returns a bounded typed
result.

So this domain is thin on purpose. It parses no PLS format, resolves no
reference and compares no station. Those live behind the owner boundaries.

One family is not a file task: `ds pls desktop …` drives PLS-CADD 16.81 itself
on its Windows desktop, through the PowerShell drivers that own that work. See
[PLS-CADD on its desktop](#pls-cadd-on-its-desktop-ds-pls-desktop).

## Shaded and unshaded workspace variants

`shading-variants` takes one digest-pinned native backup and an absent output
root. The backup filename is the delivery project name: `Final Huye
Gisagara.bak` produces root project files named `Final Huye Gisagara.*`, even
when the archived shell used a placeholder such as `A Project`.

The output contains `shaded/`, `unshaded/`, and `shading-variants.json`.
Both workspaces receive the same name and reference healing. The shaded copy
preserves presentation bytes. The unshaded copy changes only characterized
PLS-Pole attachment and drafting slots; structure, material, strength,
capacity, and every byte outside those categories remain unchanged.

Run once without `--source-sha256` to obtain the current digest, then pin it
and confirm the create-new output:

```bash
ds pls shading-variants --backup './Final Huye Gisagara.bak' --out './Final Huye Gisagara variants' --output json
ds pls shading-variants --backup './Final Huye Gisagara.bak' --source-sha256 'sha256:…' --out './Final Huye Gisagara variants' --yes --output json
```

The receipt proves the changed model set and carries before/after digests.
Fresh native Restore/reopen remains a separate acceptance gate.

## Complete backup creation

`backup-create` takes a closed, already portable workspace and an absent
`.bak` path outside that workspace. It reads every member twice around native
framing and refuses if the inventory or bytes move during the operation. The
`ds-io` writer validates typed paths, creates native directory records,
compresses the container, and self-extracts it to prove exact member-byte
recovery before `ds` publishes the new file.

```bash
ds pls reference-closure --workspace './PLS-CADD WORKSPACE' --findings-only --output json
ds pls backup-create --workspace './PLS-CADD WORKSPACE' --out ./submission.bak --yes --output json
```

This command does no path healing or native member conversion. Its receipt
therefore sets `member_bytes_preserved: true`, `path_healing_performed: false`,
and `native_restore_reopen_accepted: false`. A fresh native PLS-CADD Restore,
reopen, post-Restore closure and expected-count comparison remain separate
submission gates.

## Digest pinning is not optional

`compare-don` is **digest-pinned**: the task requires an expected `sha256:` for
each source and refuses without one. That is the guard working — it forces a
caller to state what they think the file is, so a comparison run later against
a changed file fails instead of quietly answering about different bytes.

Obtaining a pin does not require shelling out. Run it without pins and the
refusal hands both back:

```
$ ds pls compare-don --baseline ./issued.don --candidate ./revised.don
invalid_input: this comparison is digest-pinned; both sources need an expected SHA-256
  → pin the digests below with --baseline-sha256 and --candidate-sha256
  observed: baseline: sha256:fd7dcf…; candidate: sha256:a91e04…
```

This does not weaken the pin. The task still recomputes and compares at run
time; the digest's value is that it was recorded when a decision was made and
re-checked when the work runs. `pls_compare_don_refuses_a_wrong_digest` proves
the check is real.

The tasks also require **absolute** paths. `ds` canonicalizes what you pass
rather than making that a sharp edge.

## Three outcomes, not two

`compare-don` does not answer "are these different". It separates:

| Count | Meaning |
|---|---|
| `agreeing` | same structure at the position, same library name |
| `name_equivalent` | same structure, different name, reconciled by a declared `--equivalent` |
| `differing` | a different structure at that position |

Positions are matched by station within `--tolerance` metres (default 1.0), so
a resurveyed alignment does not read as a wholesale substitution.

## Paging bounds come from the tasks

The tasks bound their result sizes tightly and refuse anything larger:

| Command | Bound | Source of the number |
|---|---|---|
| `pole-capacity read` | 64 items | `MAX_DESCRIBE_LIMIT`, a public const, referenced directly |
| `reference-closure` | 32 translations | read from the task's published request schema at run time |

Neither number is written out in this repository. That is deliberate: the
first version of this domain defaulted `--limit` to 50 for both, and every
`ds pls reference-closure` call failed with the task's own `invalid_limit`
because 50 is over its bound of 32. A copied bound is a bound that drifts.

`pls_pole_capacity_limit_default_is_inside_the_task_bound` holds the line for
the default specifically, because a default over the bound makes the bare
command unusable while every flag-driven call still works.

## A task's refusal is not a `ds` refusal

When a task declines, `ds` returns `task_refused` and puts the task's own code
and detail in `detail`:

```json
{"code":"task_refused","detail":{"code":"invalid_limit","detail":"limit must be between 1 and 32"}}
```

The task's code is deliberately not promoted to `error.code`. `ds` documents
the codes *it* can emit as a closed set; a task's vocabulary is its own and may
grow without notice. A caller branches on `task_refused` and reads
`detail.code` for the specific reason — and in human mode both are printed, so
a remedy that says "read detail" has something on screen to read.

`terrain-reconcile`, `deviation-labels`, and `delivery-verify` are the bounded
exception: each
maps the owner task's larger diagnostic vocabulary into the short, declared
operator codes visible in its live command contract. The original owner code
remains in `detail["task-code"]`. This keeps common repairs branchable without
letting every low-level parser condition become a permanent CLI contract.

## Terrain waterfall and visible deviation labels

Both commands take one closed workspace, a JSON/GeoJSON point batch, ordered
routes, and an absent output path. Point input may be an array,
`{"points":[...]}`, a DS `command.rows` envelope, or a Point
FeatureCollection. Rows carry native projected `x_m`, `y_m`, `z_m` (or XYZ
geometry), with optional `code`/`feature_class`, `flag`, and `description`.
Routes may be `{"routes":[{"id":...,"coordinates":[...]}]}` or a
LineString FeatureCollection. Multipart geometry refuses.

Run the exact same evidence twice, changing only the mode:

```bash
ds pls terrain-reconcile \
  --workspace ./baseline \
  --points ./points.json \
  --routes ./routes.geojson \
  --horizontal-crs 'EDCL Rwanda TM' \
  --vertical-datum 'project surveyed TIN' \
  --out ./reconciled \
  --dry-run --output json

ds pls terrain-reconcile \
  --workspace ./baseline \
  --points ./points.json \
  --routes ./routes.geojson \
  --horizontal-crs 'EDCL Rwanda TM' \
  --vertical-datum 'project surveyed TIN' \
  --out ./reconciled \
  --yes --output json
```

The terrain task appends the corrected batch as distinct evidence, including
coincident rows. Baseline active records and the inactive block remain exact.
Only surveyed route endpoints receive seams; free ends remain listed in the
receipt. A global delta alone is never treated as complete repair.

Then derive visible feature text from route order:

```bash
ds pls deviation-labels \
  --workspace ./reconciled \
  --points ./points.json \
  --routes ./routes.geojson \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out ./labelled \
  --dry-run --output json

ds pls deviation-labels \
  --workspace ./reconciled \
  --points ./points.json \
  --routes ./routes.geojson \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out ./labelled \
  --yes --output json
```

The label writer replaces only selected feature-code byte ranges. XYZ, flags,
descriptions, unrelated rows, reserved bytes, headers, and inactive content
remain unchanged. These labels are terrain text, not alignment PIs.

Finish with the one-receipt native readback, using the untouched baseline and
the exact point batch again:

```bash
ds pls delivery-verify \
  --baseline ./baseline \
  --workspace ./labelled \
  --points ./points.json \
  --output json
```

The verifier requires the delivered count to equal baseline plus supplied
points, preserves the complete baseline terrain prefix and supplied XY/flags/
descriptions, reports the elevation-delta range and median, proves exact NUM
alignment and DON structure bytes, runs attachment closure, and re-reads every
phase/OPGW section support chain. `verified: false` is a real receipt when a
native model has sections but no complete support-chain surface; it is not an
instruction to invent one. Solver completion and engineering approval remain
outside this command.

## `section-orientation` takes a document, not flags

Its request needs the alignment's ordered structure numbers and the boundary
kind at each end — a nested object. Growing a flag per nested field would
produce exactly the "enormous collection of ambiguous flags" a typed request
document exists to avoid.

So it takes `--request <path>`, and publishes the contract:

```bash
ds pls section-orientation --schema --output json
```

That schema is the task's own, so it cannot drift from what the task accepts.

## PLS-CADD on its desktop: `ds pls desktop`

Eight verbs run PLS-CADD 16.81 itself: native Restore, the two-restore
qualification, the whole deliver chain, AutoSag, the deliverable reports and
the plan & profile PDF. The owner of that work is the set of PowerShell
drivers proven on the Nyamagabe delivery (ds-work `ea67e9b`, the deliver chain
proven end to end on the v19 cap6 export). `ds` carries them rather than
re-implementing a click:

- **Embedded byte for byte.** `crates/ds-cli-pls/desktop/` holds the 26 driver
  files and the ds entry scripts. Each file's sha256 is pinned in
  `src/desktop/bundle.rs`, with its origin: vendored unchanged, vendored and
  modified (only `pls-deliver-autosag.ps1`, which gained `-NoSheets`), or owned
  by `ds`. `.gitattributes` exempts the folder from line-ending conversion, so
  every checkout embeds the same bytes.
- **Extracted per call.** A run writes the bundle into a new private folder
  under `%TEMP%`, reads every file back against its pin, runs one
  `ds-desktop-<verb>.ps1` with `powershell.exe -NoLogo -NoProfile
  -NonInteractive -ExecutionPolicy Bypass -File …`, and removes the folder.
  Windows PowerShell 5.1 is found under `%SystemRoot%`, never on `PATH`.
- **One result document.** The entry writes `status: ok` with its receipt's
  path, or `status: failed` with the driver's own message and the PLS-CADD
  processes still running. `ds` reads the receipt the drivers wrote
  (`deliver.json`, `restore-open.json`, `manifest.json`, …) and never parses
  console text. Every result carries `receipt` and `drivers`, the bundle
  digest that names the exact scripts.

Every verb except `dialogs` refuses `windows_only` off Windows,
`pls_cadd_not_found` when `C:\Program Files\PLS\pls_cadd\pls_cadd64.exe` is
absent, and `powershell_not_found` without Windows PowerShell 5.1. Every folder
a verb creates must not exist yet, must have an existing parent, and must not
be on `C:` (`system_drive_refused`): project work lives on the project Drive,
the rule the deliver chain and the backup driver already enforce. A run
refuses `pls_cadd_running` when PLS-CADD is already open. Run them from a
terminal in the signed-in Windows session: the drivers need the desktop, so a
remote shell without one cannot drive PLS-CADD.

### The dialog catalogue rule

An unknown dialog stops the run. Record it in the catalogue with its
decision; never click through it blind.

`pls-dialog-catalog.psd1` lists every modal the drivers have met: when it
fires, how it is recognised, and the decision — `wait` (a progress box, never
clicked), `click` a named button, `click_any_ok`, `options` (press only the
visible OK of a tabbed dialog), `flow` (a dialog a driver fills in), `ignore`,
or `stop`. The watcher acts on it and journals every event. A dialog that
matches nothing is `unknown`: its control tree is journaled and the run stops
with `unknown_dialog`, carrying the dialog's title and text in `detail.dialog`.
A catalogued `stop` or an out-of-flow dialog stops with `dialog_stop`. PLS-CADD
is left open so the operator can see it.

```bash
ds pls desktop dialogs --output json                 # every decision
ds pls desktop dialogs --action stop --output json   # what stops a run
ds pls desktop dialogs --name save_changes --output json
```

`dialogs` reads the catalogue embedded in this `ds`, the one its drivers use,
so it answers on any host. To add an entry, reproduce the dialog on the pinned
PLS-CADD version, record its title, text, controls and safe outcome in the
catalogue, pin the new digest in `bundle.rs` and the entry count in
`catalog.rs`, and ship `ds`.

### `check`

Reads, changes nothing: PLS-CADD at its pinned path, its sha256 and version
against the pinned 16.81 profile (the same test the restore drivers apply),
running PLS-CADD processes, Word registered for report PDFs, and the
PowerShell version. `ready` is false with named `blockers` otherwise. The
Classic interface and the Project Wizard switched off have no characterised
setting key yet, so they are listed under `operator_confirms` with any
`PLS_CADD.INI` lines that mention them, never guessed.

### `restore`

```bash
ds pls desktop restore --bak <file.bak> --into <new folder> [--evidence <new folder>] [--sha256 sha256:<hex>] [--source-root <C:\dir>] [--project-file <name.xyz>]
```

`interim/pls-restore-open-interim.ps1` then `interim/pls-close-interim.ps1`:
a fresh Restore through PLS-CADD's own dialogs, every file restored and none
skipped, only catalogued open prompts answered, the project opened, exit
without saving, and the restored tree checked against the backup's protected
members. `--into` keeps the restored workspace; the journals go to
`--evidence`, `<into>-evidence` by default. Without `--sha256` the digest is
computed and reported; either way the driver re-checks it before PLS-CADD sees
the file. `--source-root` maps a backup spanning several roots.

### `qualify`

```bash
ds pls desktop qualify --bak <file.bak> --out <new folder>
```

`pls-backup-restore-qualify.ps1`, the native acceptance `backup-create`
cannot give itself: Restore and open (`r1`), PLS File > Backup of the
untouched project (`fresh-pls-backup.bak`), close, Full and Protected checks;
Restore that fresh backup (`r2`), close, checks against both backups. No save
is authorised. Its `evidence/manifest.json` is the receipt, including the
`saps_unlicensed_pls_16_81` caveat: this proves Restore integrity, not
engineering. On an unexpected dialog the qualifier leaves PLS-CADD open, and
the refusal says so in `detail.process_left_for_operator`.

### `deliver`

```bash
ds pls desktop deliver --bak <file.bak> --out <new folder> [--label <name>] [--paging-gap <m>] [--no-sheets] [--report-timeout <s>]
```

`pls-deliver-autosag.ps1`, the proven chain, unattended:

1. working session: fresh Restore (`r1`), AutoSag of every section through the
   Section Table, paging settings (new sheet per alignment, `--paging-gap`,
   default 100 m, page starts not rounded), Save, the Section Usage gate, PLS
   File > Backup to `backup\<label>.bak`, Exit;
2. from a fresh Restore of that backup (`r2`) — what a reviewer opening it
   sees: the six reports as RTF with their verdict lines (Section Usage,
   Structure Usage, Terrain Clearances for every feature code, Wind & Weight
   Span, Summary, Sag-Tension), every plan & profile sheet to
   `pdf\Plan and Profile.pdf`, Exit without saving;
3. the RTFs to A3 landscape PDFs with Word.

`--no-sheets` skips the sheet PDF only (the owner no longer prints PLS plan &
profile); the receipt's `sheets` is then null. `--label` names the delivered
backup and defaults to the `--bak` name with any character outside
`A-Z a-z 0-9 . _ -` replaced by `_`. `--paging-gap` takes at most two
decimals, because the driver types the gap with two and refuses a readback
that differs. Word is checked before PLS-CADD starts. The result is
`deliver.json` (`ds.pls.deliver_autosag.v4`), regrouped.

### `autosag`, `reports`, `sheets-pdf`

```bash
ds pls desktop autosag    --project <project.xyz> --out <new folder>
ds pls desktop reports    --project <project.xyz> --out <new folder>
ds pls desktop sheets-pdf --project <project.xyz> --out <new folder>
```

One step of the deliver chain each, on a saved project. PLS-CADD opens a
project by its `.xyz` entry point (`pls-launch-project.ps1`, pinned
executable digest); the catalogued watcher settles the startup and open-time
prompts. They reuse the chain's own `Watch`, `Save`, `ExitPls` and `Verdict`,
loaded verbatim from its script, and its report list and Sheets View step,
which a test holds to the chain's text.

- `autosag` saves the project **in place**: AutoSag through the Section Table
  (never menu command 40337, which crashes 16.81), Save, the Section Usage
  gate, Exit. Receipt `autosag.json`.
- `reports` writes the six RTFs and their A3 PDFs and saves nothing. Receipt
  `reports.json`.
- `sheets-pdf` writes `pdf\Plan and Profile.pdf` with PLS-CADD's own exporter,
  each sheet at its page size, and saves nothing. Receipt `sheets.json`.

These three compose proven steps in an order the deliver chain already runs;
the compositions themselves have not yet run on the desktop. `deliver`,
`restore` and `qualify` run the drivers' proven sequences as they are.

### Refusals from a run

The drivers already refuse precisely; `ds` names those refusals. The cause
is the driver's own message: a phrase the embedded drivers throw, each held to
the scripts by a test, or the watcher outcome it quotes (`did not return to
ready: unknown`). The journals only enrich a dialog refusal with the latest
matching event's title and text; they never decide the cause, because a step
can journal a dialog event and still succeed:

| Code | From |
|---|---|
| `unknown_dialog` | a watcher `unknown` outcome, or an unexpected window in the restore/backup/exit drivers |
| `dialog_stop` | a catalogued `stop` or out-of-flow dialog, or a project that needs repair |
| `pls_cadd_timeout` | PLS-CADD did not reach a state a driver waited for |
| `pls_cadd_running`, `pls_cadd_mismatch` | PLS-CADD already open; not the pinned 16.81 build |
| `backup_digest_mismatch`, `backup_invalid` | the pin moved; not a readable one-project backup |
| `restored_tree_mismatch` | restored files differ from the backup's members |
| `word_not_found` | no Word for report PDFs |
| `driver_failed` | any other driver refusal, with its message and script |
| `desktop_run_timed_out` | the whole run exceeded ds's bound; PLS-CADD may still be open |
| `driver_bundle_failed`, `driver_result_unreadable` | extraction, or a run that left no readable document |

Every refusal from a run carries `detail.message`, `detail.script` and
`detail.pls_cadd_running`. A code a verb does not document is reported as
`driver_failed`.

## What is not here yet

`ds-grid-cli` carries more PLS surface than this — structure ingest, emit,
roundtrip, method audit, the Structure Locations and Usage table, the
Available Structure List projection, the Oracle submit/status/result loop, and
PLS post-processing. Those sit behind `ds-grid-exchange`'s PLS adapter and the
Oracle spool rather than behind `ds-grid-tasks`, so each needs its own request
translation rather than a typed task to call.

Commands are exposed only through a linked owner with a narrow contract;
there is no generic native-patch adapter or argument-vector escape hatch.

## Ownership

Every command calls one function in `ds-grid-tasks`:

| Command | Task |
|---|---|
| `backup-create` | `ds_io::pls_cadd_write_workspace_backup_container` |
| `pole-capacity read` | `describe_pole_capacity` |
| `reference-closure` | `inspect_pls_reference_closure` |
| `section-orientation` | `diagnose_pls_section_orientation` |
| `compare-don` | `compare_don_assignment` |
| `shading-variants` | `create_pls_shading_variants` |
| `terrain-reconcile` | `reconcile_pls_terrain` |
| `deviation-labels` | `label_pls_deviations` |
| `delivery-verify` | `verify_pls_delivery` |

The `desktop` verbs call no task: each runs one ds entry of the embedded
PLS-CADD driver bundle, as above.

| Command | Driver |
|---|---|
| `desktop check` | `ds-desktop-check.ps1` (reads only) |
| `desktop dialogs` | the embedded `pls-dialog-catalog.psd1`, read in `ds` |
| `desktop restore` | `interim/pls-restore-open-interim.ps1`, `interim/pls-close-interim.ps1` |
| `desktop qualify` | `pls-backup-restore-qualify.ps1` |
| `desktop deliver` | `pls-deliver-autosag.ps1` |
| `desktop autosag` | `pls-launch-project.ps1`, `pls-section-table-autosag.ps1`, `pls-report-any.ps1` |
| `desktop reports` | `pls-launch-project.ps1`, `pls-report-any.ps1`, `pls-rtf-to-pdf.ps1` |
| `desktop sheets-pdf` | `pls-launch-project.ps1`, `pls-save-sheets-pdf.ps1` |
