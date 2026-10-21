# Design version and attachment authority

`ds-brain` owns immutable governed `vN` history. Native Server and Desktop
clients use the same Rust workflow with an explicitly captured project,
principal, credential audience and lane. Web authentication supplies transport
for the shared `designVersionAuthorityCommand` plan/accept facade. Creation
snapshots the governed saved server state and never captures unsaved browser
content. Device-local rooms are retained drafts, not governance versions or
pending publication work.

Objects are `lv_transformer` or `mv_model`. LV comparisons use retained
snapshots; MV comparisons name exact content revision/digest/manifest lineage
changes, without claiming geometry comparison. Restore remains LV-only and
pins the current exact source revision before the fenced server transaction.
Responses must match project, kind, object and the numeric ordinal encoded by
`vN`. Idempotency keys bind the deliberate object/reason request; hosts cannot
change accepted context or source fences while retrying.

Native model publication validates the complete `.dsgrid` with Network and
extracts `manifest_model_id` and `manifest_model_revision` from its decoded
manifest. Zero is valid present lineage. Project-head expectations and exact
native identity/revision are independent fences. The commit and exact readback
must preserve both. Missing lineage on a legacy project model is unbound;
clients must follow the server remedy rather than inventing metadata.

Design attachments are opaque immutable bytes. Their native client uses one
closed profile route for listing, upload grant, native verified transfer,
finalize, generation-pinned download authorization and fenced soft retirement.
Each operation receives an explicit project; no selected project or Desktop
state is consulted. LV attachment pins are governed `vN`; MV attachment pins
are exact content revision IDs, never governance ordinals. Existing attachment
pointer versions are read once and reused for the write, with no changed-head
retry. Partial or rejected transfers do not finalize.

Profile schema v27 includes the closed design-attachment route/actions and
pins them into the complete profile digest. Credential audience identity is
unchanged by route expansion. Native client and command Kernel release pins
must name the actual source that implements this profile.
