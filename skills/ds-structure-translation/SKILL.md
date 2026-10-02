---
name: ds-structure-translation
description: Translate a local designer's PLS-CADD structure models (EDCL-style names such as S190_1p_strain_12, HS255T-OFFHor_12, C 1000_2p_tfo_ 12) into the canonical library, one local model to one canonical member, with the design's staking table as the key. You decide the map from the evidence, ask the owner where it stops, apply it, and accept it when DS's blind staking table carries the same bill of quantities.
metadata:
  ds-chapters: pls-cadd, grid-model
  ds-mcp-profile: pls
---

# Translate local structure models into the canonical library

Local designers deliver the models EDCL uses most, under names only they can
read. A translation names each local model into one canonical member: it never
renames a file, never swaps bytes, and never borrows another member for a gap.
`ds pls structure-translate` does the mechanics (conversion, evidence, member
import, wire re-binding, blind staking, comparison). The decision is yours: the
names disagree with themselves in ways no fixed rule reads well. Read the
command's `--help` first; it is the contract.

## 1. Read the evidence

Dry run with the design's `.bak`, its staking table and the canonical library
directories (first holding a name wins):

```
ds pls structure-translate --backup <design.bak> --crs rwanda-tm \
  --staking <staking.xlsx> --library <canonical dir> --dry-run --output json
```

Without a staking table the baseline is the table DS reads from the design as
it stands; say so when you report. Per local model the receipt gives:

- `signatures`: the bill-of-quantities marks its staking rows carry, with
  counts: pole column and count, assembly, `H-Poles` or `1P +Xarm`, existing,
  transformer, stays, foundation;
- `staking_descriptions`: how the table spells it (it may not match the file);
- `attachment_sets`: strain or suspension, and branch sets;
- `line_angle`: the placements' deviation;
- `library_members`: the names the canonical libraries hold.

Check `staking_join` first: unmatched or ambiguous rows mean the wrong table,
sheet or CRS.

## 2. Decide the map

Write each member in the canonical grammar
`<type>-<material>[-<variant>]-<class>[-<stay>].<length>` (REG Version VI Annex 3
letters; ds-work `standards/POLE-NAMING-CONVENTION.md`):

| Evidence | Canonical member |
|---|---|
| 1 pole, `Susp Str`, suspension sets | `a-<m>-<class>` |
| 1 pole, `1P +Xarm`, strain sets | `l-<m>-<class>` |
| 1 pole, `1P +Xarm`, suspension sets (posts) | `l-<m>-susp-<class>` |
| 2 poles, `H-Poles` | `j-<m>-<class>` |
| `T-off str-str`, 2 poles (1 pole) | `j-<m>-toff-<class>` (`l-<m>-toff-<class>`) |
| transformer marks, 2 concrete poles | `m-c-<class>-2stay` |
| existing (tap overhead) | `ex-<m>-<class>…`: the table has no class; ask |
| no form: `1°-9°dev`, `Inline str`, `10-60° dev`, `60-90° dev`, `Term Str` | `b`, `d`, `f`, `e`, `c` |

Pole columns give material, class and length: `12 S140`…`12 S255` wood `.012`,
`14 S325` wood `.014`, `12 S800`/`14 S800` steel `S800`, `12 C850`/`C1000`/
`C1250` concrete `NPD850d`/`NPD1000`/`NPD1250`.

One local model, one member. What varies between its placements is a site
fact, not a second name:

- stays: take the stay token only when every placement carries the same stays
  and the family has stayed members; never on H-poles, whose head variants
  imply theirs;
- angles: H-pole angle variants (`60d`, `120d`) and REG angle findings are a
  design step after the translation (`ds dsgrid structure retype`);
- spellings: the signature decides; `S90_2p_12` in the table is the
  `S190_2p_12.str` model when its rows read two S190 poles.

A member no library holds stays `missing_member`: it is authored natively into
the canonical library, never mapped onto another class or family. Where the
evidence cannot decide (an existing pole's material and class, a model whose
rows split between two readings), put the candidates and their evidence to the
owner and wait.

Start from the dry run's `mapping_template`, fill each `canonical`, put the
signature you read in `note` and who decided in `decided_by`, and pass it back
with `--mapping`; `--map local=canonical` adds one decision. Keep the reviewed
map: the same designer's names come back, but check it against each new
staking table rather than trusting it blind.

## 3. Accept

Re-run the dry run with the map until nothing is `invalid_name` or `conflict`,
`held_back` is empty (or explained) and `refusal_on_write` says what is left.
Then read `acceptance`, DS's staking table written blind from the translated
model against the design's:

- `same_structure_bom` covers what names decide: poles, assemblies, form,
  existing, transformer and foundation;
- a structure pattern concentrated on one local model is a wrong decision:
  change the map;
- a pattern across one whole family can be a BOQ vocabulary difference. For
  example, a local BOQ counts `Term Str 70kN` on every transformer structure
  where ours counts the transformer structure. Report it as an exception with
  its count and let the owner accept it;
- stays are site quantities: report them, never rename to match them.

Write only when the owner accepts what is left, with the pinned digest the dry
run reports: `--source-sha256 <digest> --out <new dir> --yes`. Add
`--allow-partial` only for undecided or missing models the owner agreed to
leave. The output root holds the translated `.dsgrid`, the blind staking
table, `translation-map.json` and `translation-receipt.json`.

## 4. After

Validate the model (`ds dsgrid validate`), read REG findings
(`ds dsgrid report structures`), export to PLS-CADD (`ds dsgrid-exchange
convert --target pls-bak`) and let PLS-CADD verify it natively. A product gap
goes to `ds feedback submit` with the command, its inputs and digests, and
the receipt.

Stops at: the owner's decisions (undecided models, members to author natively,
BOQ exceptions to accept) and PLS-CADD's native verification of the exported
design, on the Windows host that runs it.
