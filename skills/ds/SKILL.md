---
name: ds
description: Use the deployed `ds` CLI as the sole Data Solutions interface — discover one live command and its contract, invoke it as declared, report a feedback gap only when live discovery confirms one. Use before any DS task; other ds-* skills assume it.
---

# Work through `ds`

Humans and agents use the same executable contract. Everything you learn about
the stack, and everything you do to it, passes through `ds`. Never substitute
an API, desktop bridge, store, file parser, implementation repository, or
skill-local program. If live CLI discovery proves `ds` cannot do the task,
report the observation through `ds` itself.

Use `--output json` for agent calls.

## Establish the installed surface

```
ds --version
ds doctor --output json
```

Treat these live results as the identity and availability of this installation.
A remembered catalog or a skill written against another build is not evidence
that a command exists here.

## Find one command

Walk the tiers; each is small and names the next:

```
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Search is lexical, not semantic. A zero-match search proves only that those
words did not match. Inspect the most likely live domain, try the product's own
vocabulary, and read summaries before concluding that the capability is
absent. Broad or noisy matches also require narrowing; never choose a command
from rank alone.

Compact discovery is not a replacement for help. `ds --help`,
`ds <domain> --help`, and `ds <domain> <command> --help` remain the complete
human-readable tiers. Use them whenever the user asks for help or the readable
contract; use `capabilities` when bounded machine-readable selection is useful.

## Read, then invoke, the live contract

```
ds capabilities <command-id> --output json
```

The descriptor is authoritative. Use only its declared inputs and example
shape, inspect its current availability, authority, effect, confirmation, and
refusal information, then invoke the narrowest command. Pass `--yes` only when
the user's actual intent authorizes that exact effect and scope.

Interpret the returned JSON according to the live result itself. Follow its
remedy and next action when it refuses. When Stable, Canary and a dev build
run together, a paired-desktop command refuses as ambiguous and lists the
candidate descriptors; pass the one whose profile matches the `ds` install
you established, and keep using it for the rest of the session. Do not repeat a non-retryable call
unchanged, switch identity or project merely to force success, or reconstruct
the answer through another surface. Return only the bounded evidence the CLI
actually supplied.

## When `ds` cannot

Stop after checking the likely live domain and alternate product vocabulary.
Discover the current feedback contract rather than relying on this prose:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Use `ds feedback submit` to send one bounded, non-secret agent sighting to the
same shared backlog as DS GridDesign's `fb` shortcut. Include what was
expected, what live discovery or invocation showed, the impact, and observable
acceptance behavior. Never create a gap Markdown file, call the feedback API
directly, or use a workaround that bypasses `ds`.

## Route to a narrower skill when one fits

- `ds-project-context` — establish or switch the active project; the state
  boundary between CLI, desktop and project.
- `ds-map-local-data` — temporary map layers, focus and restore.
- `ds-lv-design-revision` — revise one transformer's LV design safely.
- `ds-pls-cadd-terrain-roundtrip` — revise PLS-CADD route PIs and terrain through a canonical `.dsgrid` round trip.
- `ds-style-composite` — style a layer by two fields: colour plus a halo,
  opacity or size dimension.
- `ds-report-consumption` — obtain a delivered report workbook and read it
  with your own tools; consuming a document is not a `ds` gap.
- `ds-boq-staking-table` — LV `poles` sheet and MV structure names as
  staking tables against a BOQ.
- `ds-boq-combined-report` — the combined workbook against a project BOQ.

Those skills assume this one. Do not load them for ordinary discovery.
