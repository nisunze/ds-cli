# Shared Canvas2D primitives

This crate owns the camera and spatial primitives extracted from DS Grid
Profile. It has no graph, project, printing, alignment or electrical model.
UI hosts link this core; ds-interactions-wasm exports the numeric interface.
CLI and Server do not depend on these UI algorithms. Rectangular input admission
uses the neutral ds-geo-lite value shared with the existing Profile wire contract.

- Camera: Profile's 24 CSS pixel fit padding, 0.2–1,000,000 zoom range,
  finite-pan sanitization, cursor anchored zoom and centering.
- Selection: rectangle/lasso containment, crossing and touching segments,
  point-to-segment distance and drag threshold.
- Measured paths: an immutable R-tree for station/value picking and ordinate
  snapping. Separate segments preserve coverage gaps. Ties use input order.
- Free layout: bounded rectangle translation/resizing, selected handles,
  page/peer edge and center guides, optional grid snapping, minimum size.

World positions are f64. Device pixel ratio belongs to the drawing surface;
it does not scale CSS pointer coordinates. Input groups are numeric ordinals;
hosts map results to their own immutable identities.

The browser mounts the Interactions runtime before canvas editors. Profile supplies one
packed measured-path model per scene object/revision and only numeric pointer
coordinates afterwards. TypeScript does not reproduce these algorithms.
Architecture and PM retain their own business models; migrating their existing
geometry implementations is a separate consumer change.

Run `cargo test -p ds-canvas` and `cargo clippy -p ds-canvas --all-targets -- -D warnings`.
