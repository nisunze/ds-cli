---
name: ds-grid-project-model
description: Get a DS Grid model into a project through `ds` — from scratch, from a verified .dsgrid, or from a PLS-CADD workspace — and know which half of that route works headlessly today.
metadata:
  ds-chapters: grid-model
  ds-mcp-profile: grid
---

# Put a DS Grid model into a project

A **project model** is a project's own copy of a network: a mutable head plus
a chain of immutable revisions, each an exact `.dsgrid` package stored by
digest. It is not the global engineering library. If the task is seeding or
resolving an immutable shared version, you are in `ds library`, not here.

`ds dsgrid model --help` is the canonical family, with three sources for one
act:

```
ds dsgrid model create   --name … --kind … --reason …          from nothing
ds dsgrid model import   --model <path>  --name … --kind … …   from a .dsgrid
ds dsgrid model convert  --source <path> --name … --kind … …   from PLS-CADD
```

`--kind` is closed: `general`, `lv_network`, `mv_line`. The parser refuses
anything else, so do not invent a fourth.

## Read this before you plan a route

All three verbs currently refuse with
`project_model_registration_unsupported`. That is a declared state, not a
fault — check it before planning, with

```
ds capabilities dsgrid.model.import --output json
```

and read `.data.command.unavailable`. If the code is gone, registration has an
owner and the verbs run; the declared inputs do not change either way, so a
plan written against this help stays correct.

## What works headlessly today

The preparation half, all of it, with no project and no principal:

```bash
ds dsgrid validate --model ./karongi.dsgrid --output json     # prove a package
ds dsgrid inspect  --model ./karongi.dsgrid --output json     # what is in it
ds dsgrid-exchange inspect --source ./Karongi --output json   # classify a workspace
ds dsgrid-exchange plan    --source ./Karongi --output json   # what conversion would do
ds dsgrid-exchange convert --source ./Karongi --out ./karongi.dsgrid
```

Always validate before proposing registration, and carry the validation
summary into your report — a package registered without it records nothing
about its own soundness.

Then either register the result in DS GridDesign, or, when the source is a
governed catalogue example rather than a file, use the one registration path
that is already wired end to end:

```bash
ds library global fork-example --payload '{"project_id":"…","fork":{…}}' --yes
```

Read `ds library global fork-example --help` for the payload; every field is
required by ds-brain, including `expected_head_revision_id`, which is what
stops a revision replacing a head you never saw.

## Rules

- Never present a refused registration as done. `ds` refuses closed on
  purpose, and there is no fallback: no direct API call, no service account,
  no reading the project's own storage. If a route seems to exist, it is one
  of those, and it is wrong.
- Registration is `global_write` and needs `--yes`. One confirmation
  authorizes one revision, never an approval as well — approval is a separate
  capability in ds-brain and never a side effect of registering.
- `--model-id` needs `--expected-head`. Adding a revision to an existing model
  without naming the head it follows is the write this family must never make.
- `--project` fences the paired session; pass it when the project matters,
  which is whenever an agent rather than a person chose it.
- `docs/contracts/project-grid-model-contract.md` enumerates the exact ds-web
  operations and ds-brain actions still required. Cite it when reporting the
  gap; do not re-derive it.
