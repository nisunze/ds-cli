# `ds design migrate`, `ds solar migrate` and `ds survey migrate` — one vocabulary

Tier-4 reference for the migration verbs. Each command's `--help` is its
contract; this document is what is true of all of them.

They are verbs in their own domains on their own endpoints (ds-brain `POST
/api/v1/data action=migrate_design`, `POST /api/v1/solar
action=migrate_plan|migrate_apply`, and `POST /api/v1/pipeline
action=migrate` for survey data). Kinds stay inside their own domain —
`transformer|dsgrid` for design, `city|portfolio` for Solar, none for survey
data, which copies everything — and there is no cross-domain migration
registry. What they share is how they **speak**, so an operator or agent who
learned one drives the others (feedback fb90a0a5).

## Arguments

| Concept | Name, every verb | Design | Solar | Survey |
|---|---|---|---|---|
| where the objects come from | `--source-project <project-id>` | required | required | required |
| where they go | `--project <project-id>` | required | required | required |
| which object type | `--kind` | required, `transformer` or `dsgrid`, no default | required, `city` or `portfolio`, no default | — (every entry) |
| which objects | `--item <name>`, repeated, one object each | 1–200, required | 0–64; omitted means every source object of the kind | — (every entry) |
| may an existing target change | `--overwrite` | switch | switch | — (never) |
| deployment lane | `--lane` | `stable` default | `stable` default | `stable` default |

An `--item` is never split, so a name holding a comma is still one name. The
kind decides what an item names: a transformer name or DS Grid model id; a
Solar city id or portfolio name.

Migration is stateless: `--source-project` INTO `--project`, both named on every
call, never the saved selection (owner ruling 2026-09-23). Solar's apply also
takes the plan's `--migrate-digest` (see "Deliberately different" below).

## The receipt

Plan and apply return one receipt shape, and both verbs carry these fields
under these names:

| Field | Meaning |
|---|---|
| `lane`, `project.ds_project`, `source_project`, `kind`, `mode` | what was asked, `mode` being `plan` or `apply` |
| `items[]` | one row per object: `name`, `kind`, `status` (the outcome word below), `reason` |
| `requested`, `moving`, `identical`, `blocked`, `failed` | the kernel's fold of `items[].status` (`design_migration::totals`) |
| `empty_state`, `empty_breakdown` | why nothing moved; `null` whenever something moves |
| `warnings[]` | the owner service's own run-level statements, always an array |

Domain evidence follows the shared fields. Design adds `collision_policy`,
`bytes` and per-item `bytes`, `target_exists`, `rewritten`, `dropped`,
`revision_id`, `model_digest`. Solar adds `migrate_digest`, and on apply
`documents_written`, `idempotent` and `computation_results_migrated` (always
`false`); ds-brain's plan travels verbatim under `plan`, because that is the
document the digest binds.

### Survey data

Survey migration names no objects, so its receipt carries the "what was asked"
fields (`lane`, `project.ds_project`, `source_project`, `mode`) and then the
service's own totals, shaped once by the kernel
(`ds_command_kernel::survey::migration_receipt`):

| Field | Meaning |
|---|---|
| `total_matched`, `total_migrated`, `total_skipped` | entries found, copied (or copyable, in a plan), skipped |
| `skip_reasons` | why entries were skipped; `existing` is an entry id the destination already holds |
| `total_target_written`, `total_source_deleted` | what the apply wrote; deletion is never requested |
| `per_form` | entries per form |
| `more` | rows beyond the first 100 of `per_form` or `skip_reasons`, counted |
| `source_preserved` | read from the service's deletion count, not asserted |
| `overwrite_existing` | always `false` |

The request is exactly `{"action":"migrate","eds_project_id":<source>,
"target_project_id":<destination>,"dry_run":<plan>}`: the service's deletion,
overwrite and filter keys are never sent. A zero is explained by the counts —
nothing matched, or every entry was skipped for the reasons listed.

### Outcome words

One vocabulary, the kernel's `design_migration::Outcome`, in plan and apply
tense. The Solar service plans with its seeding words; `ds` states them in
these words and keeps the original under `plan`.

| `status` (plan → apply) | Means | Solar service row action |
|---|---|---|
| `would_copy` → `copied` | created in the target | `create`, listed in `applied` |
| `would_replace` → `replaced` | the target's head is replaced (`--overwrite`) | `replace`, listed in `applied` |
| `would_revise` → `revised` | a DS Grid model gains a migrated revision | — (design only) |
| `identical` | the target already holds exactly this; nothing is written | `skip` |
| `target_exists` | the target holds a different one and nothing asked to change it — or, on apply, it appeared since the plan | `changed`; a `create`/`replace` listed in `skipped` |
| `missing_source` | the source project does not have it | `missing` |
| `refused` | the source cannot be migrated as it stands; `reason` says why | `blocked` (a portfolio whose member cities are absent) |
| `conflict` | the target moved while the migration ran | — |
| `error` | something failed, or a word this vocabulary does not know | a planned move neither applied nor skipped |

Totals: `moving` counts the first three rows, `identical` the fourth,
`failed` counts `conflict` and `error`, and `blocked` everything else.

## Why nothing moved — one refusal shape

A run that moves nothing is never a bare zero. Both verbs say why the same way:

* **`empty_state`** — one key: `mig_empty_nothing_requested`,
  `mig_empty_all_identical`, `mig_empty_all_blocked` or `mig_empty_all_failed`,
  with `empty_breakdown` counting each outcome.
* **`items[].reason`** — why one object did not move (design: the service's
  sentence; Solar: a stable token such as `destination_differs` or
  `member_cities_missing_in_destination`).
* **`warnings[]`** — the service's run-level statements: design's "nothing
  moved: …" sentence, Solar's `computation_results_are_not_migrated` or
  `nothing_was_selected_to_migrate`. There is no second, single-string
  `reason` field.

A request `ds` refuses before it is sent (bad kind, bad selection, bound
exceeded, source equals target) is an ordinary error envelope with a declared
code and remedy, as for every command.

## The per-request bound

One basis for both: **the bound is the per-request limit of the owner's write
path that the apply goes through.** ds-brain declares it once, the kernel
mirrors it, and `ds` refuses above it locally, naming the number in the remedy,
before any credential is touched.

| Verb | Bound | Derived from |
|---|---|---|
| design | 200 | ds-brain `transformerCopyReadBatchSize`: one Firestore transaction commits a batch of 200 transformer copies plus the target's pipeline fence (Firestore allows 500 writes). The DS Grid kind shares it so one verb has one bound. ds-brain `MaxDesignMigrationItems`, kernel `design_migration::MAX_ITEMS`. |
| Solar | 64 | ds-brain `solarSeedMaxCities`: a migration apply writes every city through the seed writer (`writeSolarSeedCity`, one transaction of up to 500 writes per city), so it takes the seed writer's per-request bound. ds-brain `SolarMigrationMaxSelection`, kernel `solar_migration::MAX_SELECTION`. |

The numbers differ because the write paths differ. Changing either number
changes what one request can do, which is an owner decision rather than a
vocabulary one.

Survey data has no selection to bound: one request copies the whole source,
and the pipeline route's 600-second deadline is its limit.

## Deliberately different (owner rulings)

* Solar's apply is fenced by the plan's `migrate_digest` and refuses drift with
  `409`; design and survey have no plan→apply digest fence.
* Design refuses a **plan** on an archived target; Solar treats a plan as a
  read.
* Each domain keeps its own kinds. Survey data has none: it copies every
  entry, never deletes the source and never overwrites a destination entry;
  a filter, a move or an overwrite would be a new reviewed contract, not a
  flag.
