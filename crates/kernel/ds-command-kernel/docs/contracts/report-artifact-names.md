# Report artifact names

Report display and download names describe the report: project, report type,
scope, authored labels and date when available. Generated digests and run IDs
remain internal integrity and storage identities. A friendly filename is never
a registry key, deletion key, revision fence or proof of content equality.

`report_formats::report_download_filename` owns the presentation projection.
`artifact_filename` applies it after decoding a storage locator;
`combined_entry_naming` applies it to a stored version's fallback label and ZIP
download name. Local output names such as `combined_transformer.xlsx` stay
readable. Authored labels and arbitrary hexadecimal identifiers are preserved.

Historical dated cloud report names (`DDMMYY-HHMM-report-project-RUNID`, and the
underscore spelling) lose only their generated 32-hex run-ID segment. Dates,
scope suffixes and compound extensions such as `.shp.zip` remain. Existing
objects and archives are not rewritten or regenerated. Brain generates its
matching constants from this owner and checks parity; it projects registry
filenames, archive-member names and signed Content-Disposition through the
same rule while retaining exact storage paths and signing expiry.

Download names are basenames with filesystem-invalid characters and control
characters replaced, and trailing dots removed. Empty names fall back to
`report`. URLs, storage keys, signed object identity and integrity receipts are
never altered by this presentation rule.

The native format/command suites, Brain's owner-parity test and shared JSON
corpus cover legacy projection, scopes, signatures and unchanged storage
identity. `tests/report-artifact-names-wasm.cjs` exercises the actual compiled
`reportCommand` export with that corpus and the combined archive command:

```sh
wasm-pack build --target nodejs --dev --out-dir out/report-name-proof --features wasm --locked
node tests/report-artifact-names-wasm.cjs out/report-name-proof/ds_command_kernel.js
```
