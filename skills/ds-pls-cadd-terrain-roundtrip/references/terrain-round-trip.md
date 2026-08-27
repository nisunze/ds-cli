# PLS-CADD terrain and deviation round trip

Use this reference only when the task extends beyond the skill's terrain and
label fast path into native import, operator PI movement, or return analysis.
The live command descriptor owns every flag and refusal.

## Establish evidence

Record the exact backup/workspace, point batch and route digests; the selected
project member; horizontal CRS; vertical datum; route authority; and required
PLS-CADD version. Do not infer authority from recency or a filename.

Keep these identities separate:

- alignment PI: an ordered route vertex that changes topology/stationing;
- terrain observation: surveyed/acquired/derived XYZ plus classification;
- structure placement: independent XY, station/offset, orientation and exact
  native library identity.

A visible terrain feature code is not an alignment PI.

## Import and inspect

Discover the live contracts for `dsgrid-exchange.inspect`,
`dsgrid-exchange.plan`, `dsgrid-exchange.convert`, and `dsgrid.validate`.
Import an actual `.bak` when backup Restore behavior matters; do not substitute
a hand-unpacked folder. Require explicit CRS/location evidence, review every
loss, write a new package, then validate it.

Stop for ambiguous project selection, changed source digests, unresolved
native references, a failed coordinate envelope, or an invalid canonical
model. DS readability is not native Restore acceptance.

## Alignment authority

When the operator reserves existing alignment adjustment, submit no existing
route move/replace/restation operation. Prove the original alignment and
structure prefixes unchanged. A separately authorized new branch does not
grant authority to alter the old network.

For DS-owned route changes, discover the exact engine command descriptor, run
one revision-pinned dry run, then write one new `.dsgrid` revision. Never
confuse the package revision with the authored model revision.

## Terrain repair and labels

Use the skill's exact `pls.terrain-reconcile` and `pls.deviation-labels`
commands. Their receipts own statistics, seams, unresolved ends, label counts,
changed fields and digests. The reproduced failure and acceptance evidence is
in `ds-network/docs/contracts/pls-cadd-native-failure-modes.md`; do not copy it
into agent context unless diagnosing that failure.

Raw point evidence stays external and immutable. Corrected points form a new
batch. Free ends without surveyed authority remain unseamed. Existing
non-angle endpoint rows remain untouched; coincident batch markers carry
visible start/end labels.

## Native assets and workspace delivery

Actionable differential state may cross to PLS-CADD: alignment geometry,
terrain, placement/orientation, typed structure assignments and characterized
stringing. Resolve every native structure, cable and criteria member from one
pinned library version by exact typed leaf, kind and digest. Never emit or
regenerate a PLS-CADD asset from DS Grid bytes.

Write a new self-contained workspace. Run `ds pls reference-closure`, then the
skill's exact `ds pls delivery-verify` invocation, and keep
route/terrain, placement/stringing, attachment, method/criteria and report
gates independent. A copied DXF is not attached until the typed native
attachment record and closure both verify.

## Operator/native loop

Use PLS-CADD only for operator-owned PI movement, native solver work or an
explicit visual acceptance request. Do not edit files while PLS-CADD has the
workspace open. Launch-only means one guarded PowerShell launch after CLI
verification, not coordinate clicks or menu automation.

Treat every native save/backup as a new authority candidate. Re-import it and
compare:

- route vertices, direction, length and junctions;
- terrain XYZ and feature codes;
- structure XY, station/offset, elevation, orientation and type;
- exact-leaf library preservation and reference closure;
- section/support-chain and attachment readback;
- native analysis/check status and report digests.

Report deterministic container/readback evidence, native acceptance and
engineering approval as separate gates. No lower gate implies a higher one.
