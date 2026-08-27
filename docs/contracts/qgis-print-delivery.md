# QGIS print delivery contract

**Ruling:** QGIS/PyQGIS is the only cartographic print authority. DS has no
print service, embedded renderer, HTML print shell, or production print queue.

DS owns governed data export, project-scoped artifact attachment, discovery,
and report packaging. QGIS owns paper, layouts, title blocks, legends, map
frames, atlases and rendered page bytes. The selected QGIS MCP may drive those
QGIS operations after it is approved, but it never becomes a second DS data or
artifact authority.

## Project setup is human-approved

Each DS project has one operator-owned QGIS project setup. “One setup” does not
mean one page size: the project may carry any approved set of layouts, for
example A0 overview, A1 construction atlas, A3 transformer sheet and A4 field
copy, in portrait or landscape. Custom dimensions are valid too.

Every layout belongs to one cartographic family:

- `lv-atlas` for the LV atlas;
- `mv-map` for MV maps, whether delivered as one sheet, several separate
  sheets, an atlas, or one joined multipage PDF;
- `custom-map` for project-specific maps outside the LV/MV families.

The one-time loop is:

1. Export governed DS report/GIS inputs.
2. Open or create the project QGIS file in an operator-approved workspace.
3. Configure every required layout, paper size, orientation, title block,
   legend, map frame, scale/extent rule and atlas coverage.
4. Render proofs and obtain operator approval.
5. Save the QGIS project and the exact layout names used for repeat runs.

Changing a title block, layout geometry, scale rule, paper definition or atlas
coverage reopens that approval loop. Routine PyQGIS execution may be automated
only after approval and must load the saved project rather than recreate its
cartography from a DS-side template.

## Delivery workflow

1. Discover the live `ds` command contracts. The normal sources are
   `map.design.report` / `map.design.batch.report` for project-backed work and
   `report.export` for local typed inputs.
2. Render from the approved QGIS project. PDF is the primary/default print
   deliverable. PNG and JPEG are optional previews or field-use variants.
3. Attach every approved variant with `map.design.attach-print`, declaring the
   exact map family, QGIS layout name, paper size, orientation and page role.
   Multiple paper sizes and map families are separate immutable artifacts; one
   does not replace another.
4. Generate the compounded report only after attachments are complete. The
   archive includes individual pages beside each transformer's report/GIS
   files and combined atlas/joined pages at the archive root.

The CLI uploads bytes through the signed-in desktop, records their SHA-256 and
QGIS page metadata, and never receives a project credential. An archived
project remains downloadable but accepts no new attachment.

## Artifact and archive shape

| Scope | Report-engine defaults | Optional data | QGIS pages |
|---|---|---|---|
| Individual transformer | XLSX, SHP ZIP, KMZ | GPKG, GeoJSON where the live task offers them | PDF default; optional PNG/JPEG; any number of approved layouts/sizes |
| Combined report | XLSX, SHP ZIP, GeoJSONSeq ZIP | Only formats declared by the live engine | PDF default; optional PNG/JPEG; atlas or joined document |
| Compounded archive | All selected individual and combined artifacts | Chosen folder level and district combined sets | Individual pages beside transformer files; combined/joined PDFs at top level |

A top-level concatenated PDF must already be a QGIS-produced, operator-reviewed
artifact (`page-role=joined`). DS packages it byte-for-byte; DS does not merge
PDFs or invent page order. Atlas output follows the same rule.

## Naming and freshness

Use filenames that distinguish target, layout and paper size, for example
`tx-1042-lv-a3-landscape.pdf` or `project-construction-atlas-a1.pdf`. Attachment
identity is the byte digest plus its project/target metadata, not the filename
alone.

Where available, bind the attachment to the exact DS source receipt SHA-256.
Report regeneration does not delete an approved page, but a changed source
receipt means it is no longer evidence of the current export and should be
re-rendered before the next delivery.

## Forbidden lanes

- No backend or desktop print renderer.
- No retired print repository, proxy URL, route, role grant or launcher stub.
- No QGIS project/template generated silently from report code.
- No direct Firestore/GCS write from a skill, PyQGIS script or MCP.
- No DS-side PDF concatenation or inferred page order.
- No claim that one paper size is the project standard unless the operator
  explicitly approved that restriction.
