# 01 — Server-required: land every design operation headless on the kernel

**Priority 1 of the program.** Nothing else can be exercised on a real workspace without it.

## Observed (ds 0.1.0 `3518728`, Windows Stable, 2026-09-20, `ds capabilities <id>` for every command)

| Domain | Commands | `requires: server` | `requires: window` |
|---|---|---|---|
| dsgrid | 15 | 14 | 1 — `dsgrid.model.prepare-project` |
| dsgrid-exchange | 3 | 3 | 0 |
| pls | 10 | 10 | 0 |
| library | 16 | 16 | 0 |
| design | 101 | 73 | **28** — `design.tag.*` (6), `design.materials.*` (2), `design.known-columns.*` (2), `design.group.*` (5), `design.consumer-grouping.*` (4), `design.comment.*` (5), `design.sync.*` (3), `design.transformer.download` |
| map | 59 | 20 | **39** — design rooms (`map.design.open/read/select/set/create/delete/geometry/save/process/…`), `map.ui.open`, `map.zoom`, `map.draw`, `map.view`, local layer visibility |
| pm | 9 | 1 | 8 |
| assets | 14 | 5 | 9 |
| data | 16 | 12 | 4 |
| desktop | 29 | 3 | 26 (by nature: they drive the paired application) |

The DS Grid **engine** publishes 81 operations (`ds dsgrid describe`): 63 model mutations (`create_alignment`, `move_structure`, `retype_structure`, `author_structure_geometry`, `author_attachment_point`, `create_tension_section`, `author_feature_code`, `author_criterion_rule` …) reachable only as a raw JSON envelope through `dsgrid apply --envelope`, plus reads/solves/proposals (`project_plan/profile/table`, `feature_code_report`, `project_criteria_workbench`, `calculate_stringing_and_structures`, `screen_structure_usage`, `compute_support_demands`, `run_structure_analysis`, `plan_optimum_spotting`, `analyze_network_topology`, `terrain_anomaly_analysis`) reachable through `dsgrid run`. There is no typed command for any mutation.

## Required

1. **Window-free design family.** The 28 `design.*` window commands, the `map.design.*` room operations (open, read, select, set, create, delete, geometry, save, process, batch, report, pin, discard, layer-to-local, upload-to-local), `dsgrid.model.prepare-project`, the 8 `pm.*` and 9 `assets.*` window commands run with `headless_project` authority on the server, against the same governed objects the application uses, with the same refusals. The application keeps calling the same kernel commands. A `requires: window` descriptor remains only for `desktop.*` and the three pure-UI map commands (`map.ui.open`, `map.zoom`, `map.view`).
2. **Typed dsgrid commands over the engine** (all `authority none` on a package path, `headless_project` on a project head; every one with `--dry-run`, revision pin, `--yes`, receipt naming the engine operation id and the new revision):
   - `ds dsgrid alignment create|route|reverse|delete`, `ds dsgrid node create|move|delete`, `ds dsgrid edge create|delete`
   - `ds dsgrid structure create|move|retype|role|delete|describe` (description = the consultant's point 1)
   - `ds dsgrid structure-type author|geometry|attachment|list|show` (contract 05)
   - `ds dsgrid cable author|definition|curve`, `ds dsgrid section create|support|path|sag-mode|details|delete`
   - `ds dsgrid criteria weather|case|set|rule|binding|clearance` (contract 03)
   - `ds dsgrid feature-codes import|migrate|export|report` (contract 03)
   - `ds dsgrid terrain point|source|points|insert|elevation|delete|supersede`, `ds dsgrid policy author|delete`
   - `ds dsgrid analyse stringing|usage|demands|structure|topology|terrain-anomalies|clearance` (reads/solves; contract 06 attaches verification levels)
   - `ds dsgrid spotting plan|apply` (contract 04)
   - `ds dsgrid transaction apply` stays as the escape hatch for raw envelopes.
3. **Working copy as the default target.** Every typed command accepts `--model <local-id>` (the machine's working copy, journaled, becomes the next revision) or `--package <path> --out <path>` (immutable package → new package). `dsgrid model list` shows the head revision after each command.
4. **MCP parity**: the `ds_grid_model` chapter lists the same commands with the same descriptors.
5. **Receipts** name: engine operation id, descriptor digest, source revision → new revision, counts touched, warnings, and the PLS member versions affected when the working copy is linked to a workspace (contract 02).

## Refusals (named)

`requires_window_retired` (a window-only path was requested by an old client), `revision_conflict`, `operation_not_admitted` (journaled op through `run`), `envelope_invalid`, `structure_type_unknown`, `alignment_unknown`, `dry_run_only` (a mutation on a promoted project head without a working copy).

## Decisions taken while landing Required 1 (01-A, 2026-09-20)

Written here because the contract was silent; each is the smallest behaviour consistent with the existing `pm` / `assets` / `design` contracts.

1. **Retired transport, not retired codes.** A freed command keeps every refusal that names a *condition* (`work_not_permitted`, `work_revision_conflict`, `invalid_date`, `invalid_email`, `invalid_task_shape`, `invalid_assignment`, `too_many_assignees`, `nothing_to_update`, `confirmation_required`, `plan_unreadable`, `asset_*`) and drops the nine `desktop_*` pairing codes, which named the transport and cannot occur headless; documenting them would be a lie and `refusal_coverage` refuses undocumented-but-constructible codes either way. The headless set every freed command declares instead is `ds auth project status`'s (`headless_signed_out`, `headless_project_not_selected`, `project_context_stale`, `native_*`, `auth_*`), plus the route's own: `pm_refused` (ds-brain or the engine declined by name; `detail.service_message` carries the sentence), `task_not_found`, `record_not_found`, `project_not_visible` (404 = not a member).
2. **`requires_window_retired` is the parser's.** It fires for `--desktop-descriptor` on any `requires: server` command, documented once in `ds-cli/docs/contracts/cli-output-contract.md` like every parser code. There is no `--desktop-descriptor` "preview target" on `pm.*`/`assets.*`: nothing of theirs is previewed in a window. Map rooms (group d) are where an optional preview target has a meaning, and they decide it when they land.
3. **`pm.*` reads fold `get_graph` + `get_context` in the kernel; writes go through `commit_command` / `commit_batch`** — the four actions ds-brain's deployed canary (`00e3d50`) and prod already serve, so the freed commands work against today's deployment. The correspondence contract's server-side `record_list` / `record_read` / `task_read` actions (ds-brain `ffd73c5`, not yet deployed) EXTEND these reads when they ship; they do not replace them. `ds pm record list|read` therefore list records for the first time (the browser never fetched the context; `decodeProjectGraph` hard-coded `records: []`), bounded by ds-brain's 100 rows per context read and reported as `truncated`.
4. **Where the decisions live.** `ds_command_kernel::project_management::{reads, writes}` — the browser adapter's rules (`ds-web/src/lib/desktop/cli-pm.ts`) translated as they were, with the line they came from; the CLI is typed inputs and receipts. The TS adapter is left in place, unread by `ds` (TypeScript is read-only; removal is ledgered separately). `ds-cli-pm` no longer links the desktop bridge; the `tests/bridge_parity.rs` walks of `ds_cli_pm::BRIDGE_OPS` are removed with it. (That suite had been silently skipping on `run` since ds-web renamed `cli-work.ts` → `cli-pm.ts`; the stale fixture is fixed in the same commit.)
5. **Idempotency keys are the host's.** `command_id` is 128 bits of OS entropy minted in `ds-cli-auth` (`device::mint_command_id`); the kernel mints nothing. A `pm task create` without `--id` mints its task id the same way.
6. **Membership is the server's.** An elevated (platform-admin) credential reads every project but is a member of none by that fact; `pm task assign --request <me>` on a project the account is not a member of is refused by ds-brain (`pm_refused`, "every task assignee must be an active project member") and the receipt says so verbatim. Acceptance of the assign loop therefore names a member of the testing project.
7. **Ratchets.** `WINDOW_BACKLOG_TOTAL` 131 → 123 (`pm` 8 → 0); `ds-cli-pm` leaves the `WINDOW_COMMANDS` and `INVENTORY` ledgers. Context budgets untouched. No `CLIENT_PROFILE_SCHEMA` bump: `/api/v1/pm` is hard-coded in the transport like `/api/v1/assets`.

## Acceptance

```
ds dsgrid model list --account <uid>                                  # local-e9b0ccbf92d7447b active (Nyamagabe from PLS)
ds dsgrid structure describe --model local-e9b0… --structure str-…-230 --text "W045S-A0101-11 MV H-Poles Assembly, 12 m wooden, 10–60°, 2 stays" --yes
ds dsgrid structure retype   --model local-e9b0… --structure str-…-230 --type j-w-60d-S325.014 --dry-run
ds dsgrid structure retype   --model local-e9b0… --structure str-…-230 --type <h-pole type> --yes
ds dsgrid analyse usage      --model local-e9b0… --output json         # receipt with verification level (06)
ds capabilities design --output json | jq '[.data[] | select(.requires=="window")] | length'   # 0
ds capabilities map    --output json | jq '[.data[] | select(.requires=="window")] | length'   # 3
```
All on ds-server, no desktop paired, `ds doctor` green.
