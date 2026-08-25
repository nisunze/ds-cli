# `ds survey` — reference

Tier-4 reference. `ds survey <command> --help` is the contract; this document
is the part that is true of every command in the domain.

## Where forms and survey data are

Not on disk, and not reachable with a credential this process holds.

A survey form is a Form Factory document (`eds_forms/{slug}`) governed by
ds-brain, which validates field types, resolves who may edit, and refuses an
update authored against a version that has moved. Survey entries are field
data behind the same gate. So every command here is one named semantic
operation the *paired application* performs under the session it already
holds — `ds` sends a request and receives an outcome; it never receives a
credential and never runs code inside the application. There is no
`--project` flag: the active project is the one the application has open.

## The shape of a session

```bash
ds survey form list --output json                       # slugs, versions, permissions
ds survey form read --form edcl_customers_survey        # keys, types, choices, rules
ds survey form field set --form edcl_customers_survey \
  --field meter_number --required true --yes            # one change, one version guard
ds survey export --format gpkg --form edcl_customers_survey --yes
ds survey working-area read                             # what the desktop has cached
```

## Writes are the editor's own save

`field add`, `field set` and `field remove` do exactly what the Form Factory
editor's Save does: re-read the form, apply one change to the whole field
list, save it with the version that was read. Two consequences are the whole
safety story:

- **A form saved by someone else in between is refused, not merged**
  (`survey_version_conflict`). Re-read and issue the command again.
- **Keys and types never change through this door.** Renaming a key would
  orphan every entry already written under it; changing a type is a new field.
  Remove and add instead, knowing stored values are untouched by either.

Every write is `global_write` and is stopped by dispatch unless `--yes` is
present. Permission is the backend's decision: `survey_not_permitted` is the
form's own `can_edit` answer, reflected — never computed here.

## Bounds

- Choice lists: at most 200 options per field, 160 characters each; `form
  read` returns at most 50 per field and reports `options_total`.
- Export selectors: at most 50 forms and 50 surveyors per export.
- No command returns a survey row. Reads return schema and counts; export
  returns an artifact path and a signed link that expires.

## What stays elsewhere

- Materializing survey data into the desktop cache: `ds map survey download
  --entire-project` (this domain's `working-area read` only reports it).
- Copying survey data between projects: `ds map survey migrate plan | apply`,
  with its fixed preserve-source, no-overwrite policy.
- Creating, duplicating, archiving or publishing a whole form, per-project
  form settings, and entry mutations: not exposed. Each is a separate
  reviewed contract, not a flag on these commands.
