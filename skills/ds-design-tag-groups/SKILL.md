---
name: ds-design-tag-groups
description: Assign, group and govern project tags through `ds` — batch edits, governed administrative location enrichment, explicit ordered grouping plans, and the digest-pinned projection a report consumer pins.
metadata:
  ds-chapters: design
---

# Batch-edit project tags

`ds design group` discovers active, single-valued choice definitions that apply
to LV transformers. Definition ids are project-authored metadata; do not infer
special behavior from names such as `city` or `phasing`. Single-object edits may
also use the ordinary `ds design tag set` path.

1. Read the vocabulary: `ds design group list --transformers a,b --output json`.
   Per group: `defined`, `allowed`, `needsModel`, and what each transformer
   carries today.
2. Select the exact `.data.groups[].group` definition id, then plan: `ds design
   group preview --group <definition-id> --transformers a,b --value <allowed-value>
   --output json`. Read `.data.outcomes` back to the user — one row per
   transformer — and keep `.data.digest`.
3. Commit only with the user's intent: `ds design group apply … --digest
   <digest> --yes`, or `ds design group unassign … --digest <digest> --yes`
   for a clearing (previewed with no `--value`).

**Match the value, never repair it.** Take values from `allowed`. `Phase 1` and
`phase 1` are different values and the server refuses the one the project did
not define; do not lowercase, title-case or pluralise on the user's behalf.

If a plan carries model evidence, report its returned model state and
outstanding rows exactly. Never infer a model requirement from the definition
id or fabricate a receipt.

**A stale digest is not a retry.** `design_plan_stale` means the project moved;
preview again and show the new plan before applying.

## Exporting for a report

A consumer chooses grouping with an explicit ordered list of definition ids.
`ds design group export --transformers a,b --definition-ids region,wave
--output json` returns that projection plus the `sha256` over its exact bytes.
Use `--definition-ids` with the ids approved for that report; omit it only when
the intended result is one explicit untagged group.

Save it verbatim, for example
`ds design group export --transformers a,b --output json | jq -r .data.document > tags.json`.
Never parse and re-serialize it: the digest is over those bytes and the report
pins it.

Report a missing, archived, inapplicable, or ambiguous selected definition; do
not substitute a similarly named definition or fall back to a filename,
coordinate, or consumer document. `.data.excluded` names values the projection
could not carry, with the reason.

## Governed administrative location

Administrative location is not something an operator retypes and not something
you infer. When the governed location path has resolved a transformer, one
reusable operation materializes that evidence as **system-managed tags**:

```
ds design tag enrich-preview --transformers a,b --output json
ds design tag enrich-apply   --transformers a,b --digest <plan-digest> --yes
```

Read `.data.counts` first — it is the answer to "how many change?" — then
`.data.outcomes`, one row per transformer and level. Every action is a state to
report, not a step to retry:

| Action | What it means |
|---|---|
| `assign` / `reassign` | a value will be written |
| `unchanged` | the stored value already IS the evidence |
| `unassign` | the object resolves, but this level no longer has a value |
| `not_located` | the location path resolves nothing here; stored values are KEPT and reported |
| `unsupported_jurisdiction` | no governed source for this country — a complete answer, not a failure |
| `refused` | unknown transformer, or a value the vocabulary's token rules refuse |

**`unsupported_jurisdiction` is not an error to work around.** A project in Chad
groups by its own authored `city` or `region` definition instead; do not invent
administrative values, and do not create empty levels to make a schema look
complete.

**One value per level is where the TRANSFORMER is.** A transformer sited in one
sector often serves customers across several; these tags do not say that and
must not be read as saying it. Grouping by an administrative level groups
objects by their own location. If the user asks which bounds a transformer's
network covers, that is a spatial question about the entities and their
geometry — answer it there, never by inventing a second administrative value.

**Re-running is the repair path.** A second apply of an applied plan is all
`unchanged` and writes nothing.

**System-managed values are read-only through every ordinary door.**
`ds design tag set`, `ds design tag define` and `ds design group preview|apply`
all refuse them with `TAG_SYSTEM_MANAGED`. That is not a permissions problem to
escalate: the remedy is to re-run enrichment. `ds design tag list` reports
`management` and `writable` per definition — read them before offering an edit.

## Hierarchy is metadata, never a name

`ds design tag list --output json` gives, per definition: `management`,
`semanticNamespace`, `semanticKey` (`country`, `admin_level_1` … a
jurisdiction-neutral ROLE), `parentDefinition`, `jurisdiction`, `allowed` and
`allowedIds`.

- **Select definitions by id, in the caller's order.** Never search for a
  definition whose NAME is `city`, `district` or `phase`. Rwanda displays
  "District" and Chad may display nothing at that level; the id and the semantic
  key are what mean something.
- **Never split a label or a value to get a hierarchy.** `parentDefinition` and
  the values' `parent_value_id` are explicit. A jurisdiction with two levels is
  a complete hierarchy, not a broken five-level one.
- A project may author its OWN hierarchy:
  `ds design tag define --definition city --name City --values Kyabe
  --parent-definition country --jurisdiction TD --semantic-namespace
  administrative --semantic-key admin_level_1 --yes`.

## Grouping plans

A report or an archive groups by an APPLIED, digest-pinned plan — never by an
administrative column and never by a definition name:

```
ds design consumer-grouping preview --purpose report_archive \
  --transformers a,b --definition-ids loc_admin_level_2,loc_admin_level_3 --output json
ds design consumer-grouping apply   --purpose report_archive … --digest <plan-digest> --yes
ds design consumer-grouping read    --purpose report_archive --output json
ds design consumer-grouping archive --purpose report_archive --yes
```

- **`--purpose` is a closed set.** `solar_report` binds each group to a governed
  Solar city id; `report_archive` binds nothing and is what a compounded
  archive's folders follow. Nothing else is a purpose.
- **Order is identity.** `--definition-ids city,phase` and `phase,city` are
  different plans with different digests. Pass the order the user asked for.
- **Omitting `--definition-ids` means one untagged group.** It never means
  "find the city tag".
- `read` shows the stored plan without re-planning, so a stale plan is visible
  rather than refused. A consumer about to publish gets the refusal instead.
- Report `member_count`, `unassigned_count` and the group keys as returned. The
  UI, the CLI, the report receipt and the archive manifest all state the same
  numbers because exactly one authority decides them.
