# Governed design repair

`ds design repair describe --output json` returns generated native request and
result schemas for audit, compare, propose and apply-selected. Each accepts
`--input request.json --out new-result.json`. The last command projects selected
cells and returns `persisted:false`; it does not save or process a transformer.

Audit names geometry, identity, engineering-field and reference findings;
connectivity requires explicitly complete source/root evidence. Compare joins
exact positions with an explicit CRS/dimensional policy, or independent typed
keys. Ambiguous matches refuse; unmatched records remain unchanged. Proposal
pins source content and target snapshot, carries exact before/after cells and a
closed field allowlist. Selection re-derives the proposal, requires its digest,
explicit scope and confirmed stable change IDs. Missing and null remain distinct.

`ds design project repair --workspace DIR --input repair.json` commits the same
selected projection through SQLite CAS and an operation replay receipt, retaining
history and the durable pending outbox. It performs no processing or publication.

`ds design repair publish --lane canary --input publication.json --yes` restores
the matching native identity and explicit project in the proposal. It requires
the saved server version and layers-content digest, submits only selected
allowlisted cells, and verifies the returned receipt and exact saved layers.
If evidence declares a raw file SHA-256, supply its exact `--evidence-file`;
changed bytes or decoded layers refuse. Publication makes one exact saved-room readback
and one property-only write. Locks, project lifecycle, permissions, stale heads,
altered preimages, operation payload conflicts and unverified readbacks refuse.
It never processes, resizes, rebuilds or demotes approved data. Its one renumber is
the explicit `renumber` operation: one `lv_poles` feature of a repeated identity,
pinned by its reviewed feature digest, gets an unused id, and nothing else moves.

`ds design repair reseal --project P --transformer T --lane stable --dry-run`
reports a saved head whose stored content digest disagrees with the digest of
its own stored layers (publication refuses such a head with
`transformer_repair_source_moved`). With `--expect-version` and
`--expect-stored-digest` from that plan and `--yes`, it stores the recomputed
digest without sending or changing any layer, version or approval, verifies the
receipt and a stored-row re-read, and refuses when the digests already agree.

External analysis uses the executor's own commands and file formats. Its evidence
returns through these direct native controls; persistence remains an explicit
separate command consuming the reviewed proposal and selection. CLI and MCP use
the same descriptors and refusals; no skill can waive them. See the kernel
governed-design-repair contract for the closed property catalogue and source fences.
