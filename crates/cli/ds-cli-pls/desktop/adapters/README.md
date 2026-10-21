# Third-party adapters

Everything below this folder faces a program `ds` does not own. Each sub-folder
names one third party:

| Folder | Third party | Version | Programs driven |
|---|---|---|---|
| `pls-cadd/` | Power Line Systems | PLS-CADD 16.81 x64, PLS-POLE 16.81 | `pls_cadd64.exe` GUI, PLS-POLE GUI, their INI, backup and report files |
| `word/` | Microsoft | Word (any build registering `Word.Application`) | Word through COM, RTF to PDF only |

An adapter is a thin, disposable layer. It may:

- start, attach to and close the program, post characterized menu command ids,
  fill and accept dialogs the catalogue (`pls-cadd/pls-dialog-catalog.psd1`)
  knows, and stop on any dialog it does not;
- save what the program produces (backups, reports, PDFs) into a fresh folder
  and return raw evidence: files, SHA-256 digests, journals, window captures,
  verdict lines exactly as the program printed them.

It may not:

- take an engineering decision: sag, tension, clearance, capacity, spotting,
  structure naming or criteria are decided by the program or by Rust, never
  by a script;
- click through an unknown dialog, write over an existing folder, or put
  project work on `C:`;
- keep state between runs. `ds` re-materializes these bytes, verified against
  the SHA-256 pins in `src/desktop/bundle.rs`, into a fresh folder for every
  run and deletes it afterwards. A copy written by `ds pls desktop toolkit` is
  for reading or hand use; no `ds` verb ever reads it back.

The whole layer can be deleted: build `ds-cli-pls` without its default
`desktop-adapters` feature and every `ds pls desktop` verb refuses with
`adapters_not_embedded` instead of failing to build or run.

`../lab/` is not an adapter: it holds authoring tools that still carry
engineering logic and await a Rust owner. See its README.
