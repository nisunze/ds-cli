# Microsoft Word adapter

Third party: Microsoft Word, reached through COM (`Word.Application`).

`pls-rtf-to-pdf.ps1` opens each PLS-CADD report RTF read-only, optionally sets
every section to an explicit A4 or A3 landscape page box, exports a PDF beside
it and always quits Word. It never changes the RTF and refuses an existing PDF
unless told to overwrite.

It may not reformat report content, choose a paper size on its own, or fall
back to another converter: without Word the `ds` verbs refuse `word_not_found`
before PLS-CADD starts, and `ds pls desktop reports --rtf-only` skips Word.

Callers: `../pls-cadd/ds-desktop-reports.ps1` and
`../pls-cadd/pls-deliver-autosag.ps1`, through `..\word\pls-rtf-to-pdf.ps1`.

Report PDFs prefer registered Microsoft Word and otherwise use LibreOffice at
its fixed supported Windows installation path. LibreOffice runs with a private
profile and a 120-second bound, preserves the source RTF, and refuses existing
PDFs unless overwrite is explicit. Both converters retain A4/A3 landscape
overrides. Each report records the converter used. Verify this adapter locally
with scripts/test-pls-report-pdf.ps1; the test copies native RTFs and never
opens or changes a PLS-CADD model.
