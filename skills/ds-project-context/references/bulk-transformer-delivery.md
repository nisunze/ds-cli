# Bulk transformer delivery

Use this workflow only when the user supplies a bounded set of native design
archives to clean, save, and report as one project delivery.

1. Confirm the active project and explicit desktop descriptor with
   `ds desktop status --output json`.
2. Inventory the source archives without opening application storage. Derive a
   transformer name from a filename only when the user has established that
   naming contract; reject duplicates before staging. For the Huye backup
   delivery, strip a terminal Windows copy suffix ` (2)` from the filename
   stem before matching the transformer. Thus `agasharu.zip` and
   `agasharu (2).zip` address one transformer and require an explicit
   collision/revision decision; they must never create two transformer names.
   Do not strip `(2)` when it is not the terminal stem suffix.
3. Read the descriptors for `map.design.upload.inspect`,
   `map.design.upload.stage`, and `map.design.batch.save`. Work in bounded
   tranches. Treat zero mapped design layers, zero materialized features, any
   failed row, or a changed active project as a hard gate before save.
4. Save only the successfully staged explicit scope with `--yes`. Verify there
   are no locally dirty rooms. A bulk import does not mint recoverable
   transformer snapshots: existing user-visible versions remain unchanged and
   a transformer with no snapshots is v0. Keep the API's `metadata.version`
   save revision separate; it exists for optimistic concurrency and changes on
   each successful save. Feature `v_first` and `v_last` follow only the
   deliberate user-visible version; they never use the save revision. At v0,
   every baseline feature is honestly `0/0`. For an overwrite, audit those
   fields before save and inspect them again after save. Complete stamps alone
   are not proof of correct change capture: at v1 or later confirm at least one
   known unchanged, changed, and new row when those cases exist.
   Treat an unexplained all-current distribution as a delivery blocker and
   record the bounded change-capture gap instead of repairing row history in
   the skill or CLI.
5. Read the descriptor for `map.design.batch.report`. Submit the full explicit
   scope once with the requested `--file-level` and `--yes`.

The batch report command is declarative. A compounded delivery reuses fresh
individual artifacts, regenerates only missing or stale individuals, creates
the combined set for that exact scope, and packages them. Do not loop the
single-transformer report command and then request compounded output: that
duplicates work and loses one composition receipt.

Return the save counts and versions, then the report status, individual
artifact coverage, missing/error counts, archive prefix, layout, and cloud
archive locator. Sector and district placement comes from project metadata,
never from transformer filename suffixes.
