# Background project operations

Use this workflow when the user wants project-wide work done without opening a
map or entering a transformer room: preparing the paired application's local
room cache, the compounded report deliverable, or reversible transformer
retirement. Cache preparation declares `authority: project` because IndexedDB
belongs to the paired application's visible project. The other commands
declare `authority: headless_project` and use the CLI-selected project. Never
switch either context merely to make a refusal disappear.

1. Read the chosen command's descriptor, then establish its matching context.
   For `design.transformer.download`, run `ds desktop status --output json` and
   require the intended visible project. For headless commands, run
   `ds auth project status --output json`.
   If it is not the project the user named, list with `ds auth project list`
   and select the exact id with `ds auth project use --project <id>`; then
   read status again and require the exact resulting id. A project id alone
   is never authority.
2. To prepare local background reporting, run
   `ds design transformer download --transformer <name> … --output json`, or
   omit all names for every active ordinary transformer. Read the receipt's
   downloaded, already-local, dirty-preserved, failed and cancelled sets.
   `--force` may refresh clean rooms but never overwrites a dirty room. This
   operation does not navigate, process, stage, save or publish.
3. Inspect before you write. `ds design transformer inventory --output json`
   lists every transformer document with `state` (`active`, `retired`,
   `deleted`, `missing`) and the retirement record. With `--transformer`
   names it answers exactly those names; that receipt is the plan.
4. Retire only with the user's authority and a reason they gave:
   `ds design transformer retire --transformer <name> … --reason "<why>"
   --yes`. Retirement is reversible and non-destructive — nothing is erased,
   and `ds design transformer restore --transformer <name> --yes` brings it
   back. Never use `map design delete` for a reversible intent. Read every
   per-name result; a `refusal` (`not_owner`, `governance_locked`,
   `special_document`, …) is the service's decision, not a retry prompt.
5. Plan the deliverable: `ds report project scope --output json` shows the
   exact participating set and every excluded name with its state. Report
   `compounded_ready` and the exclusions to the user before generating.
6. Publish: `ds report project compounded [--transformer …] --file-level
   <transformer|sector|district|root> [--combine-per-district] [--force]
   --yes --output json`. The call blocks until the service answers (up to
   ten minutes). Return `status`, `prefix`, the archive locators, individual
   coverage, the missing individuals with their causes, and
   `registry_write_failed`. A `partial` status is a delivery with named gaps,
   not a failure to hide.
7. Hand over: `ds report project archives --output json` lists the registry
   newest first; `download_url` is a short-lived signed link when present.

Do not loop single-transformer report commands and then request a compounded
archive; the service reuses fresh individual artifacts itself. Do not mix the
paired and headless contexts in one delivery: the paired
`map design batch report` and the headless `report project compounded` produce
the same deliverable from their own project context.
