---
name: ds-boq-staking-table
description: Reconcile LV pole sheets and PLS-CADD MV structure staking tables with an EPC BOQ.
metadata:
  ds-chapters: reports
  ds-mcp-profile: grid
---

# Staking tables against the BOQ

A staking table is the pole-by-pole list an EPC contract is built and paid
against: pole number, position, structure, assemblies, stays, earthing, span
and deviation angle. In Rwanda EPC deliverables (EDCL, and the same shape
elsewhere) the pole, stay, earthing and assembly lines of the BOQ are sums
over that table, so a BOQ is checked by re-deriving those lines from the
staking table and explaining every difference.

`ds-report-consumption` governs how you obtain and read the workbook. This
skill says what the rows mean and how to compare them. Read, join and sum
with your own spreadsheet tools; keep the arithmetic in a script or a sheet,
not in your head, and give the engineer the joined table.

## LV — the `poles` sheet is the staking table

One merged band per feeder line (`LV Line 01, Cable Size: 3x70+54.6`), then
one row per pole. The InfoTable pole sections are exactly these rows summed:

| BOQ line family | Derive from | Note |
|---|---|---|
| poles by type | `struct_type` (`S140`, `400daN`, …), new rows only | `Pole Types`; existing rows go to `Existing Pole Types` and are not supplied |
| pole material | `material`, or the fact that the column collapsed | an all-wooden network drops the column |
| assemblies | `assembly_type` split on `;`, repeats counted | `EAT 54-10; EAS 54-10` is one of each |
| stays, flying stays | `stay`, `flying_stay` counts | `flying_stay` is dropped when all zero |
| earthing | `earthing` count | |
| LV cable per size | not on `poles`: `lv_lines.length` by `cable_size` | `Lv Lines (m)` |
| service cables | `service_cables.length` by `cable_size` | `Service Cables (m)` |

`num_houses`, `dev_angle`, `back_span` and `from_tr_distance` explain why a
pole got its structure and stays — angle and span drive the rule table —
so quote them when a structure differs from the engineer's expectation. A
`struct_type` of `TAP` is a tapping point on an existing pole, never a pole
to supply. An existing-status pole carries only what the design adds to it.

## MV — PLS-CADD structure names carry the BOQ line

The reporter writes no MV sheets. MV staking tables come from the PLS-CADD
workspace the engineer holds (the structure list or staking report exported
from PLS-CADD); consume that document the same way. On EDCL projects the
structure **name** is the authority for what a structure is, and its grammar
selects the BOQ line directly:

```
<type>-<material>[-<modifier>]-<class>[.<height>]      l-w-stay-S190.012
```

| type | structure | BoQ line | poles |
|---|---|---|---|
| `a` | suspension (10 kN; a 70 kN variant exists) | F 1.1 | 1 |
| `b` | 1°–9° deviation | F 1.2 | 1 |
| `d` | inline strain | F 1.3 | 1 |
| `j` | H-pole | F 1.8 | 2 |
| `l` | single pole with cross arm | F 1.9 | 1 |
| `m` | transformer structure | I 1.1 / I 1.2 | 2 |
| `ex` | tapping on existing steelwork | not supplied | 0 |

Material is `w` wood, `s` steel, `c` concrete; `.012` and `.014` are pole
heights in metres; foundations equal the pole count; the modifier states the
designed stay count. Stays are the one quantity the name cannot be trusted
for: on as-builts roughly one row in eight diverges from the modifier, and
the divergence is recorded in the structure comment. Count stays from the
comment or a site column when one exists, never by editing a name — a
structure name is a library file's identity, and a renamed one references a
file that does not exist. When the assignment itself is in question, `ds pls
compare-don` reconciles a DON's structure assignment against an authority;
read its live contract first.

MV conductor lengths are not in the LV workbook either. Take them from the
PLS-CADD section report or the design document the engineer supplies, and
say which.

## Reconcile

1. Normalise pole numbers on both sides (case, zero padding, `LV01/P003`
   against `P3`) and join. Report unmatched poles on each side before any
   totals: a BOQ built on a different pole set cannot be reconciled by sums.
2. Per matched pole compare structure, assemblies, stays, earthing, and the
   coordinates within a stated tolerance. Group differences by cause: rule
   table (angle or span thresholds), existing against new, tapping, survey
   drift, transcription.
3. Re-derive each BOQ line from the joined table and set it beside the BOQ's
   figure and the InfoTable's figure. The three should agree; when the
   InfoTable and your sum differ by more than rounding, stop and report it
   rather than choosing one.
4. Map descriptions explicitly (`9 m wooden pole, 400 daN` ↔ `400daN`; BOQ
   item codes ↔ assembly codes) in a table the engineer confirms. Never
   guess a mapping silently, and never convert a unit without showing it.
5. Return: unmatched poles, per-line deltas with their cause, the confirmed
   mapping, and the rows you could not classify.
