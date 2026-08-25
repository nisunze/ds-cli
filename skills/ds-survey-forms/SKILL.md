---
name: ds-survey-forms
description: Inspect and change Data Solutions survey forms and survey data through the deployed `ds` CLI — list a project's forms, read one form's schema, add/change/remove a field with the version guard, export survey data to GPKG/SHP/CSV/…, and read the desktop's Working Area cache. Use for any task about survey forms, fields, choices, exports or survey scope; never for editing survey entries.
---

# Survey forms and data through `ds`

Use the deployed CLI as a declarative contract. Assume the `ds` skill: discover
live, invoke exactly as declared, `--output json` for agent calls.

1. Orient with `ds survey form list --output json`. Every later `--form` is a
   `slug` from this list; `permissions.can_edit` says whether writes can
   succeed at all, `version` is the guard every write is checked against.
2. Read before changing: `ds survey form read --form <slug> --output json`.
   Take field keys from `fields[].key`, never from a label. `options_total`
   above the returned `options` length means the list was cut.
3. Change one field per command, and only what was asked:
   - `ds survey form field set --form <slug> --field <key> [--label] [--required true|false] [--option …] [--help-text] [--placeholder] --yes`
   - `ds survey form field add --form <slug> --key <key> --type <type> [--label] [--required] [--option …] --yes`
   - `ds survey form field remove --form <slug> --field <key> --yes`
   `--option` replaces the whole choice list in the order given, so pass every
   choice that should remain. Keys and types cannot be changed in place; remove
   and add instead, and say so to the user because it is a new field.
4. On `survey_version_conflict`, re-read the form and issue the same command
   again once. On `survey_not_permitted`, stop and report; do not retry.
5. Export with `ds survey export --format <gpkg|shp|kmz|geojson|xlsx|csv> [--form …] [--from yyyy-mm-dd] [--to …] [--surveyor <email>…] [--bbox w,s,e,n] --yes`.
   Report `blob_path` as the durable reference; `download_url` expires.
6. Before survey-dependent processing, check `ds survey working-area read`.
   If `cached_total` is 0 or the scope is wrong, materialize with
   `ds map survey download --entire-project` and re-read.

Do not describe a schema change as affecting stored entries; it changes what
new entries and the editor show. Do not attempt to read, edit or delete entries
through `ds` — no command does, by design. Copying survey data between projects
is `ds map survey migrate plan | apply`, with its fixed policy.
