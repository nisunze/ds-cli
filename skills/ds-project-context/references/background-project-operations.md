# Background project operations

Use this workflow when the user wants project-wide work done without opening a
map or entering a transformer room: the Combined Report, or
reversible transformer retirement. Every command here declares
`authority: headless_project` and uses the CLI-selected project. Never switch
that context merely to make a refusal disappear. (There is no cache to
prepare: the native report path reads rooms from the service, and the former
`ds design transformer download` was retired on 2026-09-20.)

1. Read the chosen command's descriptor, then establish its context with
   `ds auth project status --output json`.
   If it is not the project the user named, list with `ds auth project list`
   and select the exact id with `ds auth project use --project <id>`; then
   read status again and require the exact resulting id. A project id alone
   is never authority.
2. Nothing here navigates, processes, stages, saves or publishes a room.
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
6. Publish: `ds report project combined [--transformer …] --file-level
   <transformer|sector|district|root> [--combine-per-group] [--force]
   --yes --output json`. The call blocks until the service answers (up to
   ten minutes). Return `status`, `prefix`, the archive locators, individual
   coverage, the missing individuals with their causes, and
   `registry_write_failed`. A `partial` status is a delivery with named gaps,
   not a failure to hide.
7. Hand over: `ds report project archives --output json` lists the registry
   newest first; `download_url` is a short-lived signed link when present.

Do not loop single-transformer report commands and then request a Combined Report
archive; the service reuses fresh individual artifacts itself. Do not mix the
paired and headless contexts in one delivery: the paired
`map design batch report` and the headless `report project combined` produce
the same deliverable from their own project context.
