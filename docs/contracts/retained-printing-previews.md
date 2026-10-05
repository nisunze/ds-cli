# Retained printing previews

A preview is an actual renderer PDF published by the existing `network_reporter`
compute-artifact owner. It belongs to one project-owned template and saved
transformer. Opening reads the current retained PDF. Only explicit Refresh
captures inputs, renders and publishes a replacement. Earlier immutable cloud
objects and compute runs remain retained; this contract has no TTL or deletion.

The first consumer is the governed `voltage-drop-a4-v1` document. Its head uses
`network_reporter / export-<canonical-transformer> /
printing-preview-voltage-drop-a4-v1`, with exactly one `voltage_drop_pdf` output,
`application/pdf`, paper `A4`. No calculation happens during preview capture.

The closed `printing_preview` binding has schema `ds.printing-preview/v1` and
the exact template revision and HTML digest, saved analysis digest, server
project receipt sheets digest and existing v2 source fingerprint (country/reference included), governed renderer style reference/revision/content
digest, saved transformer revision and layer content digest. Brain reconstructs
these inputs and compares at open and in the transaction that advances the head.
A moved source stores stale work without replacing the current preview.

The source fingerprint is SHA-256 of the UTF-8 preamble
`ds.printing-preview/input-base/v1\0`, followed by unsigned 64-bit big-endian
byte-length-framed UTF-8 values in this order: project, canonical transformer,
template_id, template_revision_id, template_html_sha256, analysis_sha256,
sheets_sha256, report_input_fingerprint, renderer_style_ref, renderer_revision_id,
renderer_content_sha256, decimal transformer_revision, content_digest.

The host verifies the real A4 helper receipt, PDF hash/size/page count and
unchanged renderer build before sealing. `seal_printing_preview` uses the same
durable report artifact producer and sync queue as report publication. The
committed producer receipt transports the binding to compute open. A debug
renderer remains local only. No synthetic `report-run.json`, new database,
alternate upload route or unauthenticated cache is admitted.

Every intentional Refresh has a fresh render run and publication identity,
including when unchanged inputs produce the same PDF bytes. Retrying that run
is idempotent; changing its bytes or captured inputs refuses. The existing sync
owner uploads a newer preview even when the PDF digest matches the head. The
schema-2 sync store retains prior publication rows while projecting the latest
row of each identity to the sync planner. Its v1 migration preserves all rows
and receipts in one transaction; unknown schema-2 layouts refuse untouched.

Remote PDFs use the existing immutable `compute-artifacts/v1/projects/` object
prefix. The owner has no expiry or purge operation. Deployment must preserve a
bucket lifecycle with no automatic deletion covering this prefix. A signed
download URL expiring is a transport lease, not expiry of the retained PDF;
local byte eviction reads that cloud object again without rendering.

Project-owned forms, surveys, designs and printing documents use the same
project semantics for ordinary and template lifecycle states. Future template
families can extend this typed preview binding at their own proven boundary;
they do not justify global samples, global libraries or template-specific data
ownership.
