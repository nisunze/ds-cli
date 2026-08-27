---
name: ds-pls-cadd-terrain-roundtrip
description: Repair PLS-CADD terrain waterfalls, derive visible deviation labels, preserve operator-owned alignments, verify closure, and hand off native solver decisions.
metadata:
  ds-chapters: grid-model, pls-cadd
---

# Revise PLS-CADD route and terrain without losing native identity

Use the `ds` skill first. The workspace, point batch, ordered routes,
horizontal CRS and vertical datum are evidence. Never infer one to make a
repair run. Read
[references/terrain-round-trip.md](references/terrain-round-trip.md) only for
the longer import/operator-return workflow.

## Five-minute fast path

Set these once to the actual closed workspace and evidence files:

```bash
BASELINE="$PWD/baseline-workspace"
POINTS="$PWD/incoming-points.json"
ROUTES="$PWD/ordered-routes.geojson"
RECONCILED="$PWD/reconciled-workspace"
LABELLED="$PWD/labelled-workspace"
HORIZONTAL_CRS="EDCL Rwanda TM"
VERTICAL_DATUM="project surveyed TIN"
```

Discover, dry-run, then repeat unchanged with `--yes`:

```bash
ds capabilities pls.terrain-reconcile --output json

ds pls terrain-reconcile \
  --workspace "$BASELINE" \
  --points "$POINTS" \
  --routes "$ROUTES" \
  --horizontal-crs "$HORIZONTAL_CRS" \
  --vertical-datum "$VERTICAL_DATUM" \
  --out "$RECONCILED" \
  --dry-run --output json

ds pls terrain-reconcile \
  --workspace "$BASELINE" \
  --points "$POINTS" \
  --routes "$ROUTES" \
  --horizontal-crs "$HORIZONTAL_CRS" \
  --vertical-datum "$VERTICAL_DATUM" \
  --out "$RECONCILED" \
  --yes --output json

ds capabilities pls.deviation-labels --output json

ds pls deviation-labels \
  --workspace "$RECONCILED" \
  --points "$POINTS" \
  --routes "$ROUTES" \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out "$LABELLED" \
  --dry-run --output json

ds pls deviation-labels \
  --workspace "$RECONCILED" \
  --points "$POINTS" \
  --routes "$ROUTES" \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out "$LABELLED" \
  --yes --output json

ds pls reference-closure \
  --workspace "$LABELLED" \
  --findings-only --output json

ds capabilities pls.delivery-verify --output json

ds pls delivery-verify \
  --baseline "$BASELINE" \
  --workspace "$LABELLED" \
  --points "$POINTS" \
  --output json
```

Require the terrain receipt to name the pair count/distribution, global delta,
every seam, zero XY changes, all raw/output digests and unresolved free ends.
Require the label receipt to name internal/start/end counts, preserved occupied
rows, added markers, `changed_fields: ["code"]`, unchanged XYZ/flags and
before/after digests. Require the delivery receipt to report `verified: true`,
unchanged alignment/structure prefixes, exact terrain counts/deltas, complete
attachment closure, and phase/OPGW support-chain readback. These are
deterministic evidence; they are not native solver or engineering approval.

## Typed refusals

- `workspace_open`: close PLS-CADD; never edit underneath it.
- `ground_evidence_insufficient`: obtain surveyed ground or correct route
  evidence; never force a median-only repair.
- `datum_authority_ambiguous`: obtain the authoritative CRS/datum.
- `unordered_route_ambiguity` or `unmatched_route_vertex`: supply one ordered
  LineString and one batch identity per vertex.
- `conflicting_start_end_identity`: split/reorder the evidence.
- `occupied_endpoint_overwrite`: use `--preserve-occupied-endpoints`; never
  replace a T-Off, tap, transformer or other non-angle survey code.
- `point_batch_not_reconciled_suffix`: label the output made from that exact
  batch, not a similar workspace.
- `delivery_verification_failed`: read `detail["task-code"]`; retain both
  immutable workspaces and repair from the baseline.

## Deterministic boundary and UI handoff

Terrain correction and visible labels complete in the CLI. A feature-code
label is not a native alignment PI. When the operator owns PI movement, do not
move, replace or restation existing alignments; provide the verified workspace
and review evidence. Native calculation, operator PI movement, visual
acceptance and engineering approval remain PLS-CADD/engineer decisions.

For an authorized launch-only handoff, after PLS-CADD is closed:

```powershell
powershell -ExecutionPolicy Bypass -File .\open-pls-workspace.ps1 `
  -Project ".\PLS-CADD WORKSPACE\A Project.don" `
  -Receipt ".\native-open-receipt.json"
```

The launcher must refuse an existing PLS-CADD process unless the operator
explicitly authorizes closing it. After any UI save, re-import the saved
workspace as a new authority candidate and compare it with `$LABELLED`.

Resolve every native resource through an exact library id, immutable version,
content-root digest, typed name, native kind and member digest. Never choose
latest/basename or generate PLS-CADD assets from DS Grid bytes.
