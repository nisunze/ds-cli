---
name: ds-solar-final-authoring
description: Fill one Solar city or portfolio prompting draft, lint and import its reviewed Markdown, then optionally finish DOCX/PDF with installed document tools. Not report calculation or in-system LLM interpretation.
metadata:
  ds-chapters: project, solar
---

# Author and finish a reviewed Solar report

Use `ds` as the only DS interface. The Solar engine ends at deterministic,
fact-complete Markdown; interpretation happens in the current external LLM
session. Pandoc, LibreOffice, Microsoft Office, and bridge scripts are
post-authoring document tools, never Solar calculation or factual authorities.

## Obtain the prompting package

On Server, discover `solar.application` and the native application schema's
`report_bundle` operation. Select one city report or imported reviewed final,
pin its exact calculation batch identity, and provide a new private ZIP destination.
For a portfolio, discover `portfolio_report_bundle` and pin both its aggregate
batch and exact city-source batch. Do not substitute a current or same-name run.
On a paired Desktop the live `solar.report.bundle` compatibility contract also
exports one run, city and intent. The ZIP contains:

- the exact canonical `*-draft-<language>.md` source;
- a preview copy whose image links point into `media/`;
- every referenced verified image (export refuses if one is unavailable);
- `media-manifest.json` and a boundary README.

Edit only the canonical source. Read the whole document once for its
`FACT_LEDGER`, `CONVENTIONS`, `CHAPTER_MAP`, and `MEDIA_MANIFEST`, then fill
each `LLM_INTERPRETATION_BLOCK` in document order.

Each block is independently complete. Use only its own `CONTEXT_DATA`, which
repeats the relevant facts, table rows, and trend series even when they also
appear in a visible table. Professionally synthesize that packet; do not copy
the raw table line by line and do not retrieve facts from another block or
section. The conclusion uses its own project-wide packet in the same way.

Never inspect or interpret image pixels. Images are presentation only; their
underlying data is already present in the relevant block. Copy supplied number
display strings without recalculation. Preserve pending authored-fact markers,
use only headings in `CHAPTER_MAP` for cross-references, and change no byte
outside the named `LLM_EDIT_START/END` regions.

## Lint and import the Markdown authority

Import applies the native `ds_solar_report::lint_final` contract and refuses
changed immutable bytes, unfilled blocks, unsupported numbers, dangling
cross-references, empty headings, or invalid units. Fix the narration, not the
tables or comments. Then import and submit through the live contracts:

```text
ds solar final import --run-id <run> --city <context> --file <final.md> --yes
ds solar final submit --run-id <run> --city <context> --yes
```

Those commands are paired compatibility routes. For a Server portfolio,
discover `portfolio_final_import` and `portfolio_final_inspect`, pin the source
portfolio batch and choose its declared source draft. Each reviewed final keeps
an independent immutable closure. On Server discover the native
`final_import` and `final_inspect` schemas through `solar.application`, supply an
explicit source path, and inspect the immutable final receipt. A native
`report_bundle` of that imported final produces its governed rendering copy
with the actual reviewed narration. Do not count local import or export as online
publication; discover a supported publication contract before claiming delivery.

Import is local review state; submit is the separate publication action. The
exact reviewed Markdown remains the report authority.

## Optional document finishing

On Server, prefer the discovered native `report_finish` operation after final
import. It pins the final's complete image package and original canonical
bytes, removes the authoring guide and hidden comments only in a presentation
copy, and invokes fixed installed Pandoc/LibreOffice effects headlessly. Select
DOCX or PDF and a new private destination. Keep its source pins, content digest
and `publication: not_requested` with the exported file; local conversion is
not online delivery. When that installed operation is absent, follow the
external finishing procedure below.

Only after Markdown passes import lint, discover the governed workstation
surface with `ds capabilities workstation.status`, then run the returned
read-only status command. It currently reports LibreOffice. For a finishing
tool that status does not catalogue, use only a bounded local identity probe
such as `pandoc --version`; on Windows, treat `Get-Command WINWORD.EXE` as an
availability check, not permission to automate Office. Do not install a
component unless the operator requests it. Prefer:

1. Pandoc for Markdown → DOCX or HTML.
2. LibreOffice headless conversion for DOCX → PDF and a reopen check.
3. Microsoft Office, when installed, for operator visual review or a proven
   local bridge—not as an assumed unattended API.

Run Pandoc from the extracted bundle root so the preview copy's `media/` links
remain resolvable. Keep the reviewed canonical Markdown unchanged; the preview
copy is the rendering input. Representative commands:

```text
pandoc <bundle>/<preview.md> --from=gfm --resource-path=<bundle> \
  --standalone --output <bundle>/final.docx
libreoffice --headless --convert-to pdf --outdir <bundle> <bundle>/final.docx
```

If final narration exists only in the canonical source and the live `ds`
surface exposes no governed rendering-copy operation, stop and report that
confirmed gap. A skill must not ship or run a private executable that silently
creates a second report transformation contract.

Verify each produced file is non-empty and opens before calling it finished.
Do not import the rendering copy as the final. Upload DOCX/PDF or another
finished interpretation only through a live, explicitly discovered `ds`
report-attachment/upload command. If the installed CLI exposes none for this
Solar scope, keep the artifact local and report that confirmed gap; never call
an API or cloud-storage client directly.

Return the Markdown lint result, pending-fact list, import/publication receipt,
tool versions used, and SHA-256 for every converted or uploaded deliverable.

Stops at: the document renderer and the reviewer — installed tools typeset the
DOCX/PDF, and a human approves the text `ds` linted.
