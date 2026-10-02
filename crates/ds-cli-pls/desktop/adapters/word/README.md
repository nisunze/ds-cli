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
