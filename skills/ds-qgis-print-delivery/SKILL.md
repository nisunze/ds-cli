---
name: ds-qgis-print-delivery
description: Run operator-approved multi-layout QGIS/PyQGIS printing from governed DS report data, attach PDF/PNG/JPEG pages, and package report deliveries. Not report calculation.
---

# Deliver print reports through QGIS

Use `ds` first. QGIS is the cartographic authority; `ds` remains the only DS
data, project and artifact interface. Never call a DS API, open application
storage, upload directly to cloud storage, or recreate a retired print service.

## Establish the live surface

Run `ds --version`, `ds doctor --output json`, then discover the narrow live
contracts:

```text
ds capabilities report --output json
ds capabilities map.design.report --output json
ds capabilities map.design.attach-print --output json
ds capabilities map.design.batch.report --output json
```

Trust those descriptors over examples here. If attach-print is not available
in the installed build, discover and submit one bounded `feedback.submit`
report; do not bypass it with an API or storage client.

## Confirm the project print setup

Treat the QGIS setup as a human-in-the-loop project asset. Ask the operator to
confirm the saved QGIS project and required layout set when that evidence is
not already provided. A project may require several layouts and paper sizes —
A0/A1/A2/A3/A4, portrait/landscape, or custom dimensions. Never collapse them
to one “project paper size.”

For every layout record:

- map family: `lv-atlas`, `mv-map`, or `custom-map`;
- exact QGIS layout name;
- paper size and orientation;
- page role: `sheet`, `atlas`, or `joined`;
- transformer or combined scope;
- expected output: PDF by default, optionally PNG/JPEG;
- whether the title block, scale/extent rule and atlas coverage are approved.

If setup or approval is missing, prepare governed DS inputs and stop before
claiming a deliverable is final. A selected QGIS MCP may operate QGIS, but it
does not authorize DS project writes.

## Export, render, attach

1. Export the project data using the live project-backed report command, or
   `ds report export` when the user supplied local typed inputs. Preserve the
   artifact receipts and SHA-256 values.
2. Load the approved QGIS project. Use its existing layouts and PyQGIS atlas
   configuration; do not generate a DS-side title block or page template.
3. Render PDF as the primary artifact. Render PNG/JPEG only when requested.
   A joined or concatenated PDF is produced and ordered by QGIS/PyQGIS, not DS.
4. Review the rendered files with the operator, then attach each one with the
   live `map.design.attach-print` contract. Repeat per file, layout and paper
   size. Pass the source receipt digest when one is available.

Representative invocation only (discover before using):

```text
ds map design attach-print --path <pdf> --transformer <name> \
  --map-family lv-atlas|mv-map|custom-map \
  --layout <qgis-layout> --paper-size <size> \
  --orientation portrait|landscape --page-role sheet \
  --source-receipt-sha256 <digest> --yes --output json
```

For a project atlas or joined combined PDF, use `--scope combined` and
`--page-role atlas|joined`; omit `--transformer`.

Use `lv-atlas` for LV atlas pages. Use `mv-map` for both single-sheet and
multi-page MV map deliveries: attach separate sheets with `page-role=sheet`,
or an ordered QGIS atlas/joined PDF with `page-role=atlas|joined`. Use
`custom-map` for all other operator-defined map layouts.

## Package the delivery

Individual delivery keeps its QGIS pages beside XLSX and SHP, plus KMZ and any
optional GPKG/GeoJSON declared by the live task. Combined delivery keeps the
combined XLSX/SHP/GeoJSONSeq set beside its QGIS pages.

For a compounded delivery, attach all individual and combined pages first,
then run the live `map.design.batch.report` command for the exact transformer
scope and folder level. The server includes individual pages in each
transformer directory and combined `atlas`/`joined` files at archive root.
Do not mutate an existing immutable archive; generate a new one after adding
or changing pages.

When operating only on local files, `ds report bundle` can seal digest-pinned
report and pre-rendered QGIS artifacts into a ZIP. It does not render,
concatenate or approve them.

## Return evidence

Report the project, target, source export receipt, QGIS project/layout, paper
size, orientation, page role, output format, attached SHA-256 and resulting
archive receipt. Clearly separate operator approval from successful upload.
