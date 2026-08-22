# `ds dsgrid` — reference

Tier-4 reference. `ds dsgrid <command> --help` is the contract.
`dsgrid inspect` has its own page: [`dsgrid.inspect.md`](dsgrid.inspect.md).

## Why this domain links instead of calling

`ds-grid-model`, `ds-grid-engine` and `ds-grid-exchange` are pure libraries
with a clean boundary — no ambient state, no process contract, no documented
reason to stay separate. `ds-web/src-tauri` links them; so does `ds`.

The consequence is worth stating because it is visible to a caller: **this
domain has no external dependency at all.** `ds doctor` reports every
`dsgrid` command available on a machine with no sidecar installed and an
empty `PATH`, and `domain_smoke.rs` asserts exactly that. Contrast `ds report`
and `ds solar`, which call binaries and report `unavailable` without them.

## `validate` answers two questions, not one

A `.dsgrid` can be a sound container holding an unsound model, and the two are
fixed in completely different ways. So they are reported apart:

| Field | Question | Failure means |
|---|---|---|
| `container.verified` | does every member match the manifest's byte length, digest, row count and schema fingerprint? | the file is damaged or was written by an incompatible release |
| `model.valid` | is the authored content sound by `ds-grid-model`'s own rules? | the file is intact and the content is wrong |

A damaged container is reported as a **result**, not a refusal — exit 0 with
`container.verified: false` and `model: null`. The caller asked whether the
package is sound; answering "no" is this command working. `model` is null
rather than absent because the model was not judged unsound either; it was not
judged at all.

## `describe` is the engine describing itself

`ds-grid-engine` publishes three catalogs: journaled `commands`, all
`operations`, and `projections`. Each entry carries its parameters, its effect
class, whether it is journaled, and its result type.

Nothing is copied into this repository. The descriptors come from the engine
compiled into this binary, so they cannot be stale relative to what it will
actually do.

The catalog is large, so it is tiered the same way `ds` tiers its own help:

```bash
ds dsgrid describe                       # the operation index
ds dsgrid describe --kind commands       # journaled mutations only
ds dsgrid describe --id create_alignment # one full descriptor
```

Two small translations, both deliberate. The engine spells the effect field
`effect_class`; `ds` reports it as `effect`, because `ds` uses that word for
the same idea everywhere else and a caller should not learn a second one at a
single command. And the three catalogs do not agree on how to spell an id
(`operation_id`, `command_id`, `projection_id`), so `ds` normalizes to `id`.

## Making a `.dsgrid`, and exporting one

Classification, planning and conversion are not in this domain. They are
`ds dsgrid-exchange` — see
[`dsgrid-exchange.md`](dsgrid-exchange.md).

The split is by effect, not by subject. Every command here is `discovery` or
`read_only` and answers a question about a model that already exists. The
exchange domain manufactures one, and holds the only file-writing command in
either. A reader who wants to know what a package *is* should not have to page
past the rules for producing one.

## Ownership

`ds` computes none of this. It reads bytes and calls:

| Command | Owner |
|---|---|
| `inspect` | `ds_grid_exchange::dsgrid::inspect`, `package::unpack`, `ds_grid_model::GridModelSummary` |
| `validate` | `ds_grid_exchange::package::unpack`, `ds_grid_model::validate_snapshot` |
| `describe` | `ds_grid_engine::{describe_commands, describe_operations, describe_projections}` |

There is no second implementation of the `.dsgrid` format, of model
validation, or of source classification in this repository, and there must not
be one: two readers with two tolerances disagree silently, and the caller
receives a different answer rather than a disagreement.
