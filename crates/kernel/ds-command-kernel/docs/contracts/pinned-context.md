# Pinned design context

`pinned_context::plan` owns metadata-only read planning for the Working set and
headless `design pinned preview`. The default includes every saved design class,
including poles, diagnostics and custom classes. Explicit `require` narrows a
request; an anchor alone never proves completeness.

The shared [transformer acquisition contract](transformer-acquisition.md) owns
freshness and complete-room persistence. Pinning consumes that admitted room,
then derives a read-only renderer projection through `pinned_context::merge`.
Web/Desktop has no separate pinned preview download/cache. Project Control and pinning
share `pinned_context::room_is_fresh`; hosts do not duplicate that rule.

A genuinely absent class or a complete empty room does not cause endless reads.
A missing class/count, removed class or reduced count repairs the saved snapshot.
A newer known head invalidates all classes; an older observation cannot undo a
newer held room. Unknown revisions alone do not establish staleness. Dirty rooms
remain authoritative during ordinary refresh. Explicit discard is a separate
intent, admitted against the unchanged captured room before publication.

The focused transformer is excluded. Failures remain per transformer, and every
returned class participates in native merging and authored-style rendering.
Native proof lives in `src/pinned_context.rs`; ds-web's acquisition, pinning and
materialization tests exercise the real kernel WASM and complete room store.
