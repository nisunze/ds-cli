# Continue Profile differential compute — fresh-window handover

The owner requested this handover after crashes/compaction. Read OPEN.md §14,
“Differential compute after an edit”, in full before coding, then PRINCIPLES §6
and the transactional Profile / alignment-terrain contracts in ds-network.
Delete this file only when that OPEN entry is gone. Do not repeat its baseline.

The objective remains unfinished: each edit lands whole, solves its affected
sections, reconciles at most affected alignments, retains unchanged terrain and
other work, equals a full held-axis calculation byte for byte, and leaves a
cheap next action. Server/kernel owns decisions shared by CLI, Web and Desktop.
Work directly on canonical run, prove/push one coherent slice before the next.
Claude concurrently commits Solar sync on run; preserve its edits/processes.

Current delivered slice:
- ds-network 48349c72: native Table copy-on-write band routes, ground, terrain,
  supports and nested samples; unchanged case/label arrays stay shared; component
  signing uses their storage identity. ProfileAuthoredView now rebases consumed
  temporary connections after a complete calculation.
- ds-web 8837fa79c: whole-edit renderer admission checks the old authored base
  and final native renderer revision/input/axis pins. Local solves may change
  elevation-axis digest; do not require the old digest after native calculation.
- ds-command-kernel f3290cf: ds-profile-runtime retains composed labels and
  rebases the authored comparison
  view to the completed scene. Its new regression admits three consecutive
  geometry edits against the previous completed receiver, with fresh byte parity.

Proof is retained in ds-network/out/profile-differential/ and the owning contract.
39 engine/WASM tests, eight runtime transfer tests, 60 frontend tests (one skip)
passed. Release parity sweeps at Nyamagabe indices 100/250/400 passed with no
pending facts, zero terrain geometry checks and 128/124/128 unchanged arrays
shared. Native updates were 630/666/630 ms; worker installs 758/781/768 ms.
Do not remeasure these unchanged baselines; reduce the remaining costs.

Matching optimized LOCAL DEVELOPMENT Desktop (debug assertions enabled) and
release WASM proved real handle drag → CLI retype → undo → next drag → human
picker retype. Completion samples: 5.26/5.18/5.40/4.74/4.28 s; 0.5 s polling.
Camera retained; authored/scene/computed heads equal; pending calculation empty.
Undo keeps the journal hash while moving its cursor: compare complete history,
not only history_pin. The original timing receipt falsely timed out; the derived
cow-next-undo-retype-corrected.json retains the exact first healed sample.

Prior demonstrated trap: calculation clears temporary straight connections but
runtime retained 2,564 of them. The following edit spliced into the worker's
empty array and refused CaseBaseUnavailable. Real CLI retype committed; its
checkpoint was preserved, never undone to hide the failure. Actual Nyamagabe
native repro failed before, passes after. DS_PROFILE_GEOMETRY_PACKAGE selects
that test package; use out/profile-differential/cow-retype-committed-checkpoint.dsgrid.
Retain its referenced assets in test emit inputs. Move horizontally; native
aligned moves derive Z from terrain, so a supplied Z-only move is a no-op.

Next slice: per-row signing/transport (first remaining OPEN bullet). Calculated
transport still signs span fields and equal splice prefixes/suffixes. Shared
nested sample arrays already prove unchanged bytes, including signed zero.
Keep fresh signature equality, canonical order, bounds and publication/history
refusals. Authored transport still regenerates all 2,564 temporary connections;
bound it to affected incidence. The other independent OPEN costs are pick/drag
preview scope, exact signed base across case selection, navigation scope and
observation/publication races. A CLI observation can arrive after the async
observer healed the head: its no-op work counts do not measure the winning solve.
Fix transaction-wide timing/work attribution alongside that race, without reruns.
The installed retype help still incorrectly says calculation is excluded; update
its owning CLI discovery/reference slice when proving that timing contract.

Live Desktop on display :10:
- PROTECTED original: instance 7a1e02bc7f4f771164199c31cc83b021, PID 3134699,
  model local-8e19c052cf574797, exact head
  rev:9287c4a95405b6c3159933756e2b41d2103bb0337d6c4cf1dadc5f9f1c999502,
  undo four, redo zero, history pin
  d71e054fd38e286f3618dc1010ff4e585fcc65bc938128df0675f9a07bf9e9d5.
  Never reload/restart it. Its checkpoint preserves bytes, not its journal.
- OWN proof: instance d1df7164943ffd04cd0779f73b9c4016, PID 3775228,
  model local-feae6e3ba260438e, exact head
  rev:ee2fbd3209fced5fcaafe9bfab4c3d218660c2c0bdaf28e5c88df79a4b69551f,
  undo three, redo zero. Checkpoint cow-slice-completed-checkpoint.dsgrid,
  SHA256 f34f3f3e77f8ba505c9104924a3e04a91a9e518a6dc1005a4ec0baf84d319bd3.
  It preserves all owned test edits, not the journal. Keep the proof window for
  continuation; checkpoint its new head before rebuilding it.

Use ds skills and declared installed CLI leaves, never bridge/store/API bypass.
ds executable: ds-web/src-tauri/binaries/ds-x86_64-unknown-linux-gnu.
Set DS_DESKTOP_DESCRIPTOR to ~/.local/share/rw.datasolutions.desktop.local-dev/
cli-bridge.d/<exact-instance>.json. Do not use the launcher; it can close the
protected stack. Catalog account ARiIF6tYgUTGHN4rZMm1Gez7f3t2, lane stable;
Desktop lane local and ready project wnmajona_edcl_rural_electrification_template.
Imported proof copies are inactive; global catalog active model stayed unchanged.

Build with two workers and one own heavy job at a time. Check RAM/processes
before each. Disk TMPDIR: ds-network/out/profile-differential/tmp. Cargo target:
~/.cache/ds-griddesign/desktop-local-dev/cargo-target. Use opt-level=3, debug=0,
incremental=false for local dev. Local lane deliberately refuses release-profile
Desktop; do not bypass that fence or change identity. Build logs: cow-next-base-desktop-build.log and cow-next-base-wasm-build.log.
Source ds-web/scripts/desktop/solar-build-inputs.sh and call
ds_prepare_solar_build_inputs /home/magese/data-solutions/ds-command-kernel;
DS_SOLAR_CARGO_LOCK_PATH is ds-web/src-tauri/Cargo.lock. Export DISPLAY=:10,
GDK_BACKEND=x11, LIBGL_ALWAYS_SOFTWARE=1, WEBKIT_DISABLE_DMABUF_RENDERER=1,
DS_DESKTOP=1, DS_DESKTOP_LANE=local, VITE_DESKTOP_LOCAL_DEV=1, DS_DEV_WATCH=0.
Run ds-web/node_modules/.bin/tauri dev from ds-web, with --no-watch,
src-tauri/tauri.desktop-runtime.conf.json plus out/profile-differential/
tauri-proof.conf.json. Staged Profile WASM SHA256 is
8512c03d3ee3d28977a912a1fc3fcffa31b273eb1efcaac0757f27a79f910c03.

Vite PID 3134576, port 5173, has source watching disabled. New-window stale
transforms were safely invalidated without HMR or old-window reload:
kill -USR1 3134576; node out/profile-differential/vite-cache-inspect.mjs --invalidate
(run node in ds-network or use the full script path). It closes Node's inspector.
Use DISPLAY=:10 ~/.cache/ds-xdrive/xd for real gestures/screenshots. Bare g opens
support navigation; type 127, wait for its result, then choose it. Pencil at
289,587; type picker at 440,587; undo 319,587. Reinspect geometry before dragging.
The helper desktop-gesture-proof.py requires exact DS_DESKTOP_DESCRIPTOR.

Cleanup is FINISHED: owner said stop when temporary builds are done. About
532 GiB of inactive compiled output was removed after process/open-file checks.
Fresh builds leave disk about 29% used / 624 GiB available. No project data,
reports, sources, parked patches or sessions were deleted. Do not archive project
deliverables or keep deleting to chase 75% free. Never SIGKILL another process.
Tree registry: zero live trees, cap four, zero unlanded tree commits.
Keep out/parked-numbering-retype-20261008-1119; its duty-retype work is separate.
