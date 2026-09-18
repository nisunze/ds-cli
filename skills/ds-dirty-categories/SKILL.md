---
name: ds-dirty-categories
description: Diagnose DS Dirty Categories from the workbook and live project seeds; distinguish missing categories or aliases from code defects, apply authorized seed corrections, and verify regenerated reports.
metadata:
  ds-chapters: design, reports
---

# Diagnose Dirty Categories from the seed

Use `ds` first and follow each discovered command contract. Read delivered
workbooks with the report-consumption workflow. A dirty category is evidence
to investigate, not proof of a missing algorithm.

## Start with the exact diagnostic

Record the workbook and digest, transformer, Source, Description, Unit and
Quantity from Dirty Categories. Check the corresponding InfoTable and LV
Summary: invalid values may have been excluded or counted under a fallback.
Do not silently treat a fallback quantity as correctly classified.

## Where to look

Read fresh configuration for the same project used by the report. Discover
`design.categories.read` for customer and meter catalogs, and
`design.feeder-limits.read` for the transformer cable catalog, bounds and
optional complete configuration artifact. Inspect the live declarations;
do not infer flags from this skill.

Read the `hazards` the catalog read returns before reading its rows. They name
what the rows cannot: which category unrecognised values are counted as and
whether anything chose it (`decided_by: catalog_order` means nothing did but
the seeding order), which source labels two canonical categories both claim,
how many rows carry no canonical name, and which client/country vocabularies
the project was seeded with. A contested label resolves to neither owner: its
customers land in the fallback, so its quantity is missing from one section
total and inflating another.

| Diagnostic Source | Configuration owner and fields |
|---|---|
| Customers Categories | Data Cleaning / Cust Category: `cust_category`, canonical `clean_name`, `misspelled_names`, demand metadata; project `default_category` |
| Phase Types | Data Cleaning / Cust Meter Type: `cust_meter_type`, canonical names and aliases |
| Feeders | Engineering Standards / Transfo Sizes: transformer kVA → `lv_cable_sizes_bundles` → `standard_cables_size_transfo_to_db`; project LV and feeder bounds |
| LV Lines / Service Cables | Data Cleaning cable catalogs and their canonical names and aliases |
| Assembly / Pole Types | Assembly and structure catalogs; project structure-size selection policy |

Use `ds` source-feature inspection only where needed to distinguish the raw
value from the report projection. A stale workbook is not evidence that the
current seed is wrong. Preserve the user's newer catalog edits.

## Decide what needs changing

- **Missing canonical row or new category:** confirm its business meaning,
  then seed it as a distinct category. Do not collapse it into an existing
  category just because that removes the diagnostic.
- **Recognized concept with a different source label:** seed an approved
  alias to the right canonical category. Preserve that category's demand
  and engineering metadata. Conflicting aliases need resolution, not a guess.
- **Blank source value:** distinguish missing data from an unknown nonblank
  category; inspect the governed default before proposing a repair.
- **Valid seed ignored:** this is a code defect. Reproduce it with a small
  configuration-driven fixture and fix generic catalog consumption when code
  work is authorized. Never add runtime branches for particular category names.
- **Wrong project, stale inputs or stale engine:** correct the report context
  before changing a valid seed.

Readyboard is a distinct meter category alongside Single Phase and Three
Phase. Productive can have an owner-approved Commercial default alias while
retaining its source label. These are seed examples, not global runtime rules.
An unfamiliar category never authorizes inventing its demand or cable rating.

## Apply and verify

Within the user's authorized scope, discover the configuration owner.
`design.meter-types.ensure` adds a distinct meter type and
`design.customer-categories.alias` assigns an alias to an existing customer
category — those two are the only verbs that create anything. Housekeeping on a
catalog that is already wrong has its own verbs, each of which orders, renames
or removes what is already there: `design.customer-categories.retire`,
`design.customer-categories.retire-unnamed`,
`design.customer-categories.rename`, `design.customer-categories.unbind` and
`design.meter-types.default`. Resolve a contested label by unbinding it from
the category that should not own it, then aliasing it to the one that should.
Before retiring the category the hazards name as the `catalog_order` fallback,
store the choice as the project's `default_category`, or retiring it moves every
unrecognised customer somewhere nobody asked for. Any further catalog repair
requires its own supported owner; report a confirmed missing capability through
`ds feedback` instead of bypassing it.

Verify a fresh saved catalog, then regenerate with the same intended scope
through `ds report`. Do not edit or split workbook cells to hide diagnostics.
Check that the targeted dirty rows disappeared for the right reason, distinct
categories remain visible, quantities are conserved, and no new dirty rows
appeared. For grouped deliveries, verify that disjoint scope totals add to
the combined report and shapefile membership matches each scope.

Return the seed correction or proven code defect, affected quantities,
verified configuration/report receipts, and any unresolved diagnostic.

Stops at: the repository — a proven code defect is fixed by a developer, not by
another seed correction.
