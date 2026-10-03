# Background project operations

Use this workflow when the user wants project-wide work done without opening a
map or entering a transformer room: the Combined Report, or
reversible transformer retirement. Every command here declares
`authority: headless_project` and names its project with a required
`--project`; no saved selection is read. Never change the project merely to
make a refusal disappear. (There is no cache to
prepare: the native report path reads rooms from the service, and the former
`ds design transformer download` was retired on 2026-09-20.)

For “status of project X”, resolve the exact id with `ds auth project list`
and read `ds design dashboard --project <exact-id> --output json`; this is a
headless Server read, independent of the paired Desktop project. For a report,
choose the computation path deliberately: `ds report project compute
--project <exact-id> --yes --output json` starts individual cloud report
generation; claim publication only from its returned task/result receipt.
`ds report project export --project <exact-id> --out-dir
<directory> --output json` creates local outputs. Drain queued local
publications with `ds report outbox drain --project <exact-id> --yes --output
json` before requesting an archive. Preview its scope with `ds report project
scope --project <exact-id> --output json`, publish with `ds report project
compounded --project <exact-id> --file-level <transformer|sector|district|root>
--yes --output json`, then verify the registry with `ds report project
archives --project <exact-id> --output json`. A Compounded Report is a ZIP
snapshot containing individual reports and a combined data set.

1. Read the chosen command's descriptor. Resolve the exact project id the
   user named with `ds auth project list` and pass it as `--project <id>`.
   A project id alone is never authority: the gateway rechecks membership on
   every call.
2. Nothing here navigates, processes, stages, saves or publishes a room.
3. Inspect before you write. `ds design transformer inventory --project <exact-id>
   --output json`
   lists every transformer document with `state` (`active`, `retired`,
   `deleted`, `missing`) and the retirement record. With `--transformer`
   names it answers exactly those names; that receipt is the plan.
4. Retire only with the user's authority and a reason they gave:
   `ds design transformer retire --project <exact-id> --transformer <name> …
   --reason "<why>"
   --yes`. Retirement is reversible and non-destructive — nothing is erased,
   and `ds design transformer restore --project <exact-id> --transformer <name>
   --yes` brings it
   back. Never use `map design delete` for a reversible intent. Read every
   per-name result; a `refusal` (`not_owner`, `governance_locked`,
   `special_document`, …) is the service's decision, not a retry prompt.
5. Plan the deliverable: `ds report project scope --project <exact-id> --output json` shows the
   exact participating set and every excluded name with its state. Report
   `compounded_ready` and the exclusions to the user before generating.
6. Publish: `ds report project compounded --project <exact-id> [--transformer …] --file-level
   <transformer|sector|district|root> [--combine-per-group] [--force]
   --yes --output json`. The call blocks until the service answers (up to
   ten minutes). Return `status`, `prefix`, the archive locators, individual
   coverage, the missing individuals with their causes, and
   `registry_write_failed`. A `partial` status is a delivery with named gaps,
   not a failure to hide. For one archive per city, tag or administrative
   level, pass `--group-by <definition-id>` (repeat to nest) and optional
   `--where <id>=<value>` instead of `--transformer`; `.data.groups` then
   holds one receipt per archive.
7. Hand over: `ds report project archives --project <exact-id> --output json` lists the registry
   newest first; `download_url` is a short-lived signed link when present.

Request the ZIP archive through `report project compounded`; the service reuses
fresh individual artifacts itself. Verify coverage and publication in the
returned receipt and `report project archives`. The deprecated `report project
combined` alias requests the same ZIP; no paired command composes this archive.
