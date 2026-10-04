# Authored style purposes

`style_purpose` indexes declared editing and print purposes over the existing
`style_resolution::Snapshot`. The native CLI captures the authorized table
under the explicit project and invokes this pure kernel function. This read
does not create styles, seed a project, adopt documents or produce a preview.

The declaration lives in an immutable governed style document at
`metadata.style_purposes`, with schema `ds.style-purposes/v1`. The source
document is admitted by the existing exact tuple resolver before the
declaration is read. A document bound to several tuples is read once for its
exact reference, scope, revision and content digest. Distinct declarations
of the same purpose for the selected target and ink are ambiguous.

Every group explicitly authors its id, label, kind, target, ink, ordered
required role ids, role declarations, required printing page roles and layout
references. Kinds are `style_editing` and `print_composition`. A print
composition requires at least one printing page role; a style editing group
has no layout requirements. No A0 roster, adjacent-circuit style, label,
source alias or template id is built into the index.

Each role has an id, label and exact existing `style_resolution::Key`.
Required ids and declared role ids must match. Resolution uses the held
table unchanged; class, source kind, target, role and ink never fall back to
a neighbouring tuple. The key target and ink must equal the group's own.
The returned binding retains its immutable reference, revision, digest,
scope, document and presentation tier.

Every line role also authors `line_type`, the exact array value or expression
expected in its held document's `paint.line-dasharray`. Absence of either the
constraint or the property refuses; a different expression refuses even if
both documents contain a dash property. Explicit solid values remain data.
Non-line roles cannot declare this line constraint. The style resolver still
owns structural style admission and content-digest checks; this module does
not implement MapLibre evaluation or pick a dash pattern.

## Separate printing layout authority

Each required layout reference names `template_id`, the existing `PageRole`,
`paper`, `revision_id` and `content_sha256`. The required page-role roster and
declared references must match exactly. These closed references cannot carry
layout bodies or masquerade as style keys.

The index has no `Templates` capture. Each required reference therefore
returns `style_purpose_layout_capture_required` in the group's blockers, with
`composition_ready: false`. A successful style index is not permission to
preview, render or deliver that composition.

The index marks admitted `style_roles_ready: true`; `composition_ready` stays
false for this read, including editing-only groups, because it admits no print
composition. Editing-only groups need no layout capture blockers.

The print authority remains
`printing::standards::{Templates, TemplateDocument}`. The existing generic
accessor `Templates::document_by_id` admits the exact template, page role,
paper and project, validates the captured body's content digest and revision,
and checks layout and semantic-layer contracts. `printing::Command::StandardLayout`
already delegates the standard page consumer to that authority. A later
purpose preview must obtain that separate authorized capture and compare
both admitted document pins to the authored required reference before
delegating to the existing composition consumer. The style index does not
clone those layout decisions or claim a reference proves its body.

`style_purpose::plan` performs this separate admission for one named print
purpose. It reuses the index and `Templates::document_by_id`, compares both
document pins with the authored reference, and requires every template layer
tuple to be one of the purpose's admitted roles. Every required role must
occur on a captured page. Existing `style_resolution::prepare_print` supplies
the exact captures consumed by `printing::resolved_styles`; no alternate
resolver, linetype choice or layout body is generated.

The native `style.purpose.plan` command captures Styles and the server-sealed
configuration's `printing_standard` under the same explicit project, native
account, credential audience and lane. Identity changes between these calls
refuse. Its `ds.style-purpose-plan/v1` result contains the admitted page bodies,
source table/Templates revisions, and resolver captures. `composition_ready`
means those presentation inputs are admitted. It does not claim geographic
coverage, saved engineering inputs, a preview or publication: geometry and
artifacts are explicitly absent. Report acquisition and rendering retain their
own existing guards. The style index alone remains capture-required.

## Bounds and proof

The captured table and serialized result each have a 32 MiB bound. Across
distinct declaration sources there are at most 64 groups, with at most 64
style roles and 16 layout references per group. Unknown declaration fields,
duplicate roles and invalid identities or pins refuse. Authored declaration
and required-role order survives; the UI has no group-selection policy to
reimplement.

The in-module fixtures explicitly author a synthetic transformer-on-A0
purpose and exact held line documents. They prove preservation and controls
for missing declarations, missing roles and linetypes, expression mismatch,
project changes, class/source/role changes, screen/print and ink separation,
tampered held digests, duplicate purposes and missing layout references.
They do not assert any production document has adopted these declarations.
Live production reads may correctly return `style_purpose_missing` until
the document owner authors and reviews a version through the governed
document workflow. Seed planning and adoption remain separate explicit
operations; this implementation does not perform them.
