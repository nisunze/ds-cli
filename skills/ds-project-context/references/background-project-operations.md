# Background project operations

Use this workflow when the user wants project-wide work done without a map,
a room or the desktop application: the compounded report deliverable, or the
reversible retirement of transformers. Every command here declares
`authority: headless_project`; the paired application's visible project is
irrelevant to it and must not be switched to make it succeed.

1. Establish the CLI-selected project: `ds auth project status --output json`.
   If it is not the project the user named, list with `ds auth project list`
   and select the exact id with `ds auth project use --project <id>`; then
   read status again and require the exact resulting id. A project id alone
   is never authority.
2. Inspect before you write. `ds design transformer inventory --output json`
   lists every transformer document with `state` (`active`, `retired`,
   `deleted`, `missing`) and the retirement record. With `--transformer`
   names it answers exactly those names; that receipt is the plan.
3. Retire only with the user's authority and a reason they gave:
   `ds design transformer retire --transformer <name> … --reason "<why>"
   --yes`. Retirement is reversible and non-destructive — nothing is erased,
   and `ds design transformer restore --transformer <name> --yes` brings it
   back. Never use `map design delete` for a reversible intent. Read every
   per-name result; a `refusal` (`not_owner`, `governance_locked`,
   `special_document`, …) is the service's decision, not a retry prompt.
4. Plan the deliverable: `ds report project scope --output json` shows the
   exact participating set and every excluded name with its state. Report
   `compounded_ready` and the exclusions to the user before generating.
5. Publish: `ds report project compounded [--transformer …] --file-level
   <transformer|sector|district|root> [--combine-per-district] [--force]
   --yes --output json`. The call blocks until the service answers (up to
   ten minutes). Return `status`, `prefix`, the archive locators, individual
   coverage, the missing individuals with their causes, and
   `registry_write_failed`. A `partial` status is a delivery with named gaps,
   not a failure to hide.
6. Hand over: `ds report project archives --output json` lists the registry
   newest first; `download_url` is a short-lived signed link when present.

Do not loop single-transformer report commands and then request a compounded
archive; the service reuses fresh individual artifacts itself. Do not mix the
paired and headless contexts in one delivery: the paired
`map design batch report` and the headless `report project compounded` produce
the same deliverable from their own project context.
