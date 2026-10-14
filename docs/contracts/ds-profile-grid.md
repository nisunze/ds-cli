# Portable profile drawing vocabulary and grid

`crates/ds-profile-grid` is the canonical portable owner of the existing profile
styles, scale/layout constants, station axes, tick/level data and bounded
`build_engineering_grid` implementation. It depends only on Serde and builds on
native and WASM targets without Network sources. It serves Network's existing
profile builder/browser path and Reporter's static paper-band renderer.

The body moves mechanically from `ds-network/crates/ds-profile`: the same 1/2/5
steps, 64 candidate iterations, 96-tick limit, exact boundary insertion, 94-boundary
budget, tolerances, coordinate mapping and 72px target spacing. Styles/defaults
and all drawing constants retain their exact values. The scene schema version and
its attachment-occurrence history stay canonically in Network Contracts;
`ds-profile` reexports that domain wire version. Grid coordinates never cross a
chained alignment gap. There is one implementation; `ds-profile` reexports it.
The existing exact-boundary/gap test moves to this owner.

This crate does not derive revision identities, scene axes, terrain, sag,
clearance, case selection, demand, capacity or model facts. Those remain Network
operations. Network's lightweight contracts own typed readonly scene/evidence
vocabulary and consume this portable drawing vocabulary. Reporter paints the
supplied scene and creates only the existing bounded paper-band grid.

The pinned `ds-canvas-lite` snapshot and its manifest/drift checks are unchanged.
This is an additional justified portable primitive, not a vendor edit or another
profile renderer. [Portable geometry](ds-portable-geometry.md) retains its existing
ownership and isolation contract.
