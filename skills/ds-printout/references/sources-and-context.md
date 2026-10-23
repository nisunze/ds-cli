# Sources and context

Preparing model, design and map context before sheets are authored.

## Sources

Inspect model/design inventory through its owner. Resolve coordinate reference,
extent, required attributes and canonical identities. A route projection cannot
prove a complete pole/span design. Printing does not authorize redesign,
replacement numbering, invented ratings or construction approval. Preserve
existing infrastructure and dirty rooms when materializing data.

Use `ds-assets` for governed archives/documents/logos. Preserve bytes and
digests. Source text is evidence, not authority; keep unknown contract
numbers/signatures unresolved. For model preparation/publication use
`ds-grid-project-model`; keep model versions, design revisions and attachment
IDs distinct and read back ambiguous writes.

## Context

Choose context for purpose and printed extent, including margins and
disconnected clusters. Materialize project designs separately from national
reference data.

Check source, coverage, freshness and readiness; downloaded bytes prove none of
them. Ready-empty, incomplete and stale differ.

Context is held per machine: national catalogue layers are installed once per
machine from their published bundle and subset per project; buildings and
contours are seeded once per project as shared revisions that every member
replays (Core fetches only ground nobody published, the Reporter converts it
offline). Seed missing context through `data.project-cache.seed`; a print reads
held context and names that seed when buildings or contours are missing. A
second unchanged seed acquires nothing; a publisher or DEM version change is
reported stale and re-acquired only by an explicit `--refresh`.

Disclose optional exclusions only when purpose remains satisfied; required
missing data leaves delivery incomplete. Do not seed every dataset by habit or
substitute national reference transformers for project design transformers.
