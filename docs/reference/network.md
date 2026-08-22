# `ds network` — reference

Tier-4 reference. `ds network <command> --help` is the contract.
`network inspect` has its own page: [`network.inspect.md`](network.inspect.md).

## Why this domain links instead of calling

`ds-grid-model`, `ds-grid-engine` and `ds-grid-exchange` are pure libraries
with a clean boundary — no ambient state, no process contract, no documented
reason to stay separate. `ds-web/src-tauri` links them; so does `ds`.

The consequence is worth stating because it is visible to a caller: **this
domain has no external dependency at all.** `ds doctor` reports every
`network` command available on a machine with no sidecar installed and an
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
ds network describe                       # the operation index
ds network describe --kind commands       # journaled mutations only
ds network describe --id create_alignment # one full descriptor
```

Two small translations, both deliberate. The engine spells the effect field
`effect_class`; `ds` reports it as `effect`, because `ds` uses that word for
the same idea everywhere else and a caller should not learn a second one at a
single command. And the three catalogs do not agree on how to spell an id
(`operation_id`, `command_id`, `projection_id`), so `ds` normalizes to `id`.

## `convert inspect` before converting

This answers the first question anyone has about a pile of engineering files:
*what is this, and what can I do with it?* Until it existed, the only way to
find out was to attempt a conversion and read the failure.

```bash
ds network convert inspect --source ./workspace
```

A directory is read as **one folder source**, recursively, in sorted order.
Sorting is not cosmetic: the engine digests the member list, so unsorted
directory iteration would make the same tree produce different digests on
different machines. `network_convert_inspect_is_deterministic_over_a_directory`
holds that line.

The result carries, per source: the engine's classification, the exact
`sha256:` digest, the member count, and whatever version and units evidence
the engine recovered — for a PLS-CADD workspace that is its declared version
and unit system, read out of the files rather than guessed.

Then the **capability matrix**: every conversion the engine offers from this
set, with its state and reason. Only `ready` and `unverified` capabilities are
offered by default; `--blocked` adds the rest with the engine's own
explanation of why each is unavailable.

`unverified` is included on purpose — a path that exists but has not been
verified for these inputs is something a caller may reasonably attempt, and
its reason says so.

Nothing is converted and nothing is written.

### Bounds

512 MiB and 4 096 files across all sources. A mistyped path at a large tree
fails in a moment with `source_too_large`, not after reading it.

## What is not here yet

`convert plan` and `convert run` are not implemented, and the reason is
specific rather than a shrug.

`ConversionRequest` carries a `SourceSet` of raw **bytes**, plus batch mode,
target format, PLS version intent and container, combine options, a declared
CRS, optional expected-location evidence, an XY-swap flag, and opaque
selection tokens. A JSON request document for that would name paths where the
struct holds bytes — which means `ds` would own a hand-authored adapter shape
that the engine never sees and therefore cannot check.

That is exactly the hand copy this repository requires to be *checked* against
its owner (see
[`../contracts/process-boundary-contract.md`](../contracts/process-boundary-contract.md)),
and the check does not exist yet. Shipping the adapter without it would be
shipping a schema that drifts silently. `convert inspect` needs no such
adapter, which is why it ships now.

## Ownership

`ds` computes none of this. It reads bytes and calls:

| Command | Owner |
|---|---|
| `inspect` | `ds_grid_exchange::dsgrid::inspect`, `package::unpack`, `ds_grid_model::GridModelSummary` |
| `validate` | `ds_grid_exchange::package::unpack`, `ds_grid_model::validate_snapshot` |
| `convert inspect` | `ds_grid_exchange::conversion::{inspect_sources, conversion_capabilities}` |
| `describe` | `ds_grid_engine::{describe_commands, describe_operations, describe_projections}` |

There is no second implementation of the `.dsgrid` format, of model
validation, or of source classification in this repository, and there must not
be one: two readers with two tolerances disagree silently, and the caller
receives a different answer rather than a disagreement.
