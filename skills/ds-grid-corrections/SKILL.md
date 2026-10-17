---
name: ds-grid-corrections
description: Record and verify project review evidence for Grid corrections through retained Project Work and Assets commands. Core package correction and planning commands are unavailable.
metadata:
  ds-chapters: project, assets
---

# Record a Grid correction review

Use the `ds` skill and live descriptors first. Keep the source comment, exact
model revision and digest, selected objects, engineering decision, and its
actual outcome separate. Core does not expose Grid correction planning or
package mutation. The independent engineering owner must supply a reviewed
result; Project Work records it without claiming to perform the correction.

## Project review record

For a requested note, use `ds-project-work` and live descriptors. Keep its
Markdown body short: source comment/finding ID, model digest, selected IDs,
actual decision, result and open decision. Keep the batch and report as assets.
Use one stable `--id`; on conflict, read that record before minting another.

To summarize recent work, read a dated, paged project-note list and open the
relevant IDs. Also inspect `ds pm plan` (`recent` task rows), selected
`ds pm task read --timeline`, and `ds assets list --order recent` for reports.
Report `truncated` or `bodyTruncated`; these reads are not a complete PM feed.
The deployed `ds` has no project-wide PM activity command; `server.activity`
shows Sync Center jobs only.

```bash
ds pm record list --project <project> --category project_note --since <day> --limit 50 --page 0 --output json
ds pm record read --project <project> --record <listed-note-id> --output json
```

For an authorized note and HTML report, read the minted IDs from each receipt.
Ingestion and linking are separate confirmed writes:

```bash
ds pm note create --project <project> --id <stable-id> --title "Pole modification review" --body-file /absolute/review.md --yes --output json
ds assets ingest --project <project> --path /absolute/review.html --yes --output json
ds assets attach --project <project> --asset <asset-id> --record <note-id> --yes --output json
ds pm record read --project <project> --record <note-id> --output json
```

Claim the HTML link only when the readback lists that asset ID. Skip ingestion
when the report already has an asset ID.

Stops at: the independent engineering owner for corrections and native PLS-CADD
for solver acceptance; hand over the exact reviewed evidence and project record.
