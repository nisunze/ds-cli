# Canvas2D graph geometry authority

**Status:** active authority — `ds-canvas-graph` owns the geometry; the matching
ds-web Architecture consumer must ship with the same pinned WASM build
**Crate:** `crates/ds-canvas-graph`
**WASM shell:** `crates/ds-interactions-wasm/src/canvas_graph.rs`
**Wire schema:** `canvas-graph-scene-v1` (packed `f64`) + `canvasGraphAbi()`

Rust owns every position a graph canvas draws. `ds-canvas-graph` computes node
rectangles, named port anchors, edge routes and their control points,
containment tethers, scene bounds, the world-to-window projection, what is under
the pointer, and where a dragged node lands. A host supplies window facts and
user intent and renders the answer; it does not recompute, correct or persist
geometry.

This is the boundary the DS Grid Profile surface already proves — Rust/WASM
computes, Canvas2D presents. The principle that makes it the rule for every
canvas surface is
[ds-web `docs/specs/RUST_WASM_CANVAS_GEOMETRY_MIGRATION_CONTRACT.md`]; the
consumer-side reuse boundary it is meant to replace is described in that repo's
`DYNAMIC_ARCHITECTURE_PLANNING_CONTRACT.md`.

## Ownership split

| Rust owns | Host owns |
|---|---|
| node/container/port/label/control-point positions | canvas width, height, device-pixel ratio |
| line, quadratic and cubic route geometry | pointer, wheel and keyboard events |
| scene bounds and the visible extent | resolved theme, colours, fonts, stroke widths |
| the world-to-window projection | which graph and which operation to run |
| hit regions and pointer resolution | painting the returned commands |
| the authoritative cell a drag resolves to | authored `column`/`row` and node identity |

Canonical positions are finite `f64` world coordinates. Screen pixels, window
size and zoom are projections only and are never canonical graph state.

## Domain neutrality

These are transient UI algorithms owned by `ds-interactions`. CLI and Server
dependency closures exclude them; headless Rust tests exercise UI behavior.
See [the interaction boundary](interaction-boundary.md).

The crate has no lane, kind, confidence, evidence, delivery state, task or
milestone. Two surfaces sharing this authority share geometry, not each other's
business objects. A consumer keeps its own model and speaks in identities,
authored cells and named ports.

## Authored placement and orientation

`column` is the position along the flow axis; `row` is the position across it.
Orientation decides which screen axis each becomes:

| orientation | flow axis (`column`) | cross axis (`row`) |
|---|---|---|
| `horizontal` | +x | +y |
| `vertical` | +y | +x |

One authored graph therefore draws both ways without re-authoring, and
`columnGap` always means "between successive flow steps". Placement is authored,
never solved: a solved layout looks different after every edit, so a reviewer
cannot tell a content change from a jitter change.

Default metrics: `nodeWidth` 212, `nodeHeight` 64, label anchors at normalized
ratios `9/212`, `23/64`, and `45/64`, `columnGap` 92, `rowGap` 26,
`scenePadding` 40, `stub` 14, `tetherRailGap` 12, `portHitRadiusPx` 7,
`edgeHitTolerancePx` 6, `curveFlattenSteps` 16, `maxColumn`/`maxRow` 4096,
`maxDragWorld` 1e6. A host may override any of them; they are geometry inputs,
not measurements taken at draw time.

## Route shapes

Four orthogonal cases, selected by the resolved anchor faces and the authored
cells:

1. **facing across the flow gap** — a Z elbow turning at the mid-gap;
2. **same flow step, cross-axis faces** — a sidestep to a rail beyond both
   boxes, so a route between two nodes in one column never lies over the nodes
   between them;
3. **mixed axes** — an L with a stub at each end;
4. **facing the same way or away** — around an outward rail with stubs.

`shape` selects what the route is drawn as. All four produce the same `Curve`
type and the same endpoints; they differ only in segments and control points:

- `orthogonal` — the routed polyline as `line` segments;
- `straight` — one `line` from anchor to anchor;
- `quadratic` — one `quadratic` whose control is the elbow the orthogonal route
  turns at, so the two agree about which way the edge bends;
- `cubic` — one `cubic` whose handles reach along each anchor's own normal.

Every route also carries a flattened `polyline` used for bounds, hit testing and
hosts that do not want bezier support. A label anchor is the arc-length midpoint
of that polyline.

Scene bounds cover node rectangles **and** route geometry, then pad by
`scenePadding`: a sidestep or tether rail sits outside every box, and a bound
that ignored it would clip the drawing the first time a viewport fitted it. An
empty graph reports `(0, 0) → (1, 1)`, which is non-degenerate on purpose.

### Free placed cards and shared connector presentation

`connector::route_buffer` and the numeric WASM `canvasGraphRoute` export route
free placed rectangles through the same `Curve`, anchor normals and shape
construction as authored-cell scenes. They do not arrange or move cards.
Plan keeps its authored positions and hierarchy; workflows keep their typed
port references; Architecture keeps its chapter identities. The shared owner
chooses every attachment point, control point, label midpoint and tangent.

The input contains 16 finite numbers:

```text
sourceX, sourceY, sourceWidth, sourceHeight,
targetX, targetY, targetWidth, targetHeight,
shape, stableLane, sourceSide, targetSide, sourceOffset, targetOffset,
orientation, clearance
```

Shapes are `0` orthogonal, `1` straight, `2` quadratic and `3` cubic. Sides use
the existing packed port codes; `-1` requests automatic attachment. Offsets
must lie in `0..1`; lanes are integers in `0..4096`. Auto attachment keeps
stacked cards on an external side lane and forward/backward flow on facing
sides. A lane is derived from stable edge identity, never array order.

Two optional operands select a container boundary (`0` none, `1` target,
`2` source) and its interior padding. A boundary stub preserves the authored
dependency direction. A stub longer than its child card's width returns an
empty buffer instead of running across the container. Three further operands
optionally project the answer with `panX, panY, positiveZoom`; the host does
not recalculate screen positions.

The result is:

```text
startX, startY, endX, endY, startAngle, endAngle,
midX, midY, midAngle, segmentCount, ...segments
```

Segments use the same seven-value scene encoding. Start/end tangents use the
exact Bézier derivative, skipping repeated controls; middle markers follow
the flattened arc-length midpoint. `canvasGraphCurveMarkers` supplies these
nine marker values for an existing scene curve. `canvasGraphCurveDistance`
uses the same flattened painted path for hit distance, so choosing angled or
curved presentation cannot leave the old shape as an invisible hit target.
Both accept `startX, startY, segmentCount, ...segments`; distance additionally
takes the pointer's x/y. Invalid lengths, opcodes, dimensions, offsets and
non-finite operands are refused using the existing typed geometry envelope.

Hosts may choose colours, stroke widths and end/middle/dot/no marker style.
They paint the returned segment instructions and tangents and contain no
connector geometry fallback. A bounded cache may retain immutable native
answers across paint, badge and pointer passes.

### Shared explicit arrangement

`arrange::arrange` and WASM `canvasGraphArrange` accept one closed graph intent
with stable node ids, variable width/height, dependencies, optional minimum
flow coordinates and optional calendar-day lower bounds. `gapX`, `gapY` and
orientation are explicit inputs. The packed answer is
`width, height, ...[x, y, rank]` in the supplied node order.

The solver uses iterative cycle-safe longest-path layering, predecessor-median
ordering and collision compaction. Stable ids break ties, so reordering input
arrays or repeating an edge does not rearrange a graph. Calendar lower bounds
may only advance successors; native geometry never edits authored dates or
decides workflow admission. Display cycles discard only deterministic DFS back
edges; the owning workflow/schedule validator still refuses illegal edits.

Collision queries use a compressed horizontal interval tree whose vertical
occupancy intervals merge. A dense source column therefore remains a bounded
indexed query rather than a scan over every previously placed card. Native
tests lock array-order stability, variable footprints, calendar alignment,
orientation, invalid inputs, ten-thousand-node chains, and the growth shape of
two-thousand versus eight-thousand dense cards.

Arrange is an explicit intent or initial model assembly, never a pointer or
animation-loop operation. It has no viewport input, so resizing cannot change
world positions. Existing authored positions stay in place until the user
requests Arrange. Plan invokes the same solver within each containment sibling
set; canonical hierarchy and calendar facts stay in the Plan adapter, while native composition owns their spatial interpretation. Workflow invokes
it on its typed dependency graph. Architecture may retain authored chapter
cells or explicitly arrange its current chapter through this same owner.

The retained `GeometrySession::arrange_cells` / WASM session `arrangeGraph`
method resolves the shared proposal onto that session's authored-cell grid.
It atomically rebuilds the scene, keeps node/edge identity tables unchanged,
and increments the revision only when positions change. A refused arrangement
retains the prior scene and revision. This changes the current diagram session;
it does not write the architecture manifest. Hosts re-read the scene buffer and
may explicitly fit the camera afterwards. Expanded card footprints belong in
native `LayoutMetrics.nodeWidth/nodeHeight`, so adjacent cells retain clearance.

The design review consulted GeoLibre's primary
[model graph editor](https://github.com/opengeos/GeoLibre/blob/main/apps/geolibre-desktop/src/lib/model-graph-edit.ts)
and [Model Builder guide](https://geolibre.app/user-guide/model-builder/).
Useful principles are explicit Arrange, preserving hand-placed positions,
iterative dependency traversal and keeping connector ports reachable after a
move. DS implements these in its existing native authority with variable card
footprints, calendar lower bounds and viewport-independent world coordinates.
No external layout runtime or copied GeoLibre implementation is required.

## Interaction ABI

### Free-card composition and hit sessions

`composition::compose` owns nested free-card measurement and placement,
container insets, hidden descendant identities, derived expansion displacement
and root stacking. `composition::auto_arrange` owns the existing root calendar
layout and undated parking. Consumers supply ordered identities, parent
references, optional numeric calendar days, disclosure intents and explicit
display metrics; no PM task, delivery state or authorization model enters this
crate. Unknown parents remain visible fallback roots. Containment cycles are
bounded and drawable. The native request admits at most 20,000 nodes and
60,000 edges and refuses non-finite coordinates and invalid metrics.

The WASM `CanvasFreeGraph` constructor parses those closed constraints once.
`composeBuffer` accepts only `[nodeIndex,x,y,...]` placement overrides and emits
`[count,...cards]`, with each card encoded as
`[index,x,y,width,height,depth,container,expanded,collapsed,childCount,hiddenCount,...hiddenIndices]`.
Indices address the constructor's ordered identities. A refused numeric update
retains the initialized scene; a later empty override restores its baseline.
`canvasGraphAutoArrange` is a one-shot JSON Arrange intent, never pointer or
animation traffic. Expansion displacement is derived, never saved over the
person's authored placements. Intentional baseline overlaps remain intentional.

`free_hit::FreeCards` and retained WASM `CanvasFreeCardGeometry` own inclusive
reverse-paint-order body hits, side/perimeter anchors, zoom-scaled port radii,
visibility, coarse-pointer port hits and cycle-safe descendant exclusion when
dropping a card. Rectangle/parent tables cross at scene initialization; pointer
queries carry numeric coordinates and return indices. The port painter and
hit test consume the same native radius and visibility answer, so an invisible
port cannot intercept a card drag. Native tests cover parity, malformed inputs,
hidden ports, orphan parents, cycles and deep hierarchy; Web tests exercise the
matching WASM plus retained-session reuse and refusal recovery.

The interaction loop must not stringify a JSON tree. Three things cross:

| | shape | when |
|---|---|---|
| scene | one `Float64Array` + id tables | once per `sceneRevision()` |
| projection | 7 `f64` | per frame |
| hit / drag | one `u32` / a packed patch | per pointer event |

Identity never travels in the buffer. Ids are strings, strings are expensive at
the boundary, and a drag changes geometry rather than membership — so
`sceneIndexJson()` is fetched once per revision and dense indices are used for
everything after that. `sceneRevision()` bumps only when scene geometry changes;
camera and window changes do not bump it.

### Scene buffer — `sceneBuffer()`

Header (11 values):

| index | meaning |
|---|---|
| 0 | buffer version (currently `1`) |
| 1 | revision |
| 2 | orientation — `0` horizontal, `1` vertical |
| 3 | node count |
| 4 | port count |
| 5 | edge count |
| 6 | tether count |
| 7–10 | bounds `minX, minY, maxX, maxY` |

Then, in order:

- **nodes**, stride 8: `x, y, width, height, primaryLabelX, primaryLabelY,
  secondaryLabelX, secondaryLabelY`
- **ports**, stride 4: `nodeIndex, side, x, y`
  (side: `0` left, `1` right, `2` top, `3` bottom)
- **edges**, 9 values then segments:
  `fromNodeIndex, toNodeIndex, fromPortIndex, toPortIndex, labelX, labelY,
  startX, startY, segmentCount` — a port index of `-1` (`noPort`) means geometry
  chose the face
- **tethers**, 5 values then segments:
  `childNodeIndex, parentNodeIndex, startX, startY, segmentCount`

Every segment is 7 values regardless of op, so a reader advances by a constant:

```text
op, controlAx, controlAy, controlBx, controlBy, x, y
op: 1 = lineTo, 2 = quadraticCurveTo (controlA), 3 = bezierCurveTo (controlA, controlB)
```

Unused control slots repeat the segment's own endpoint, so a consumer that
ignores `op` still reads coordinates inside the curve rather than a spurious
origin.

### Projection — `projectionBuffer()`

`[scale, offsetX, offsetY, devicePixelRatio, zoom, panX, panY]`. A renderer
applies it once:

```text
ctx.setTransform(scale, 0, 0, scale, offsetX, offsetY)
```

Zoom is relative to the fitted scale and is clamped to `0.2 … 1e6`; fit padding
is 24 px. `zoomViewportAt` keeps the world point under the cursor still.

### Hit testing — `hitTestScreen()` / `hitTestWorld()`

One packed `u32`: kind in the top four bits, dense index in the low 28.

```text
kind: 0 none, 1 node, 2 port, 3 edge, 4 tether
```

Priority is ports, then nodes, then edges, then tethers; within a kind the
later-authored entry wins, because it is the one drawn on top. Tolerances are
screen sizes divided by the current scale, so a target keeps the same physical
size at every zoom. `hitTargetJson(code)` resolves a code to its identity and is
called only when the hovered target changes.

### Drag — `previewDrag()` / `applyDrag()`

A host reports a pointer delta; it does not decide where the node lands. The
authority answers with the resolved cell and a patch of exactly what changed, so
a drag cannot drift: the result is recomputed from the authored grid rather than
accumulated from deltas. Preview and apply run the same pure planner — preview
discards the resulting scene, apply keeps it.

Patch header (14 values):

| index | meaning |
|---|---|
| 0 | buffer version |
| 1 | revision the patch describes |
| 2 | moved (`0`/`1`) |
| 3 | clamped (`0`/`1`) |
| 4 | dragged node index |
| 5–6 | resolved `column`, `row` |
| 7–9 | changed node / edge / tether counts |
| 10–13 | bounds `minX, minY, maxX, maxY` |

Then changed **nodes** (stride 9: `index, x, y, width, height, primaryLabelX,
primaryLabelY, secondaryLabelX, secondaryLabelY`), changed
**edges** (10 values then segments) and changed **tethers** (6 values then
segments), using the same segment encoding as the scene buffer. Bounds are
always present: any move can change the scene extent, and a stale extent
silently changes what "fit" means on the next frame.

`canvasGraphAbi()` publishes every offset, stride and opcode above. The native
test `the_published_descriptor_matches_what_the_encoder_writes` fails if the
descriptor and the encoder ever disagree, so a host derived from the descriptor
cannot drift silently.

## Refusals

Non-finite input is refused, never clamped: a clamped `NaN` is still a wrong
position, just one nobody notices. Every refusal carries a stable machine code
and its exact operands, and on WASM arrives as a JS `Error` whose message is the
JSON envelope `{ code, message, details }`.

```text
canvas_geometry_non_finite_input     canvas_geometry_invalid_metric
canvas_geometry_empty_identifier     canvas_geometry_duplicate_node
canvas_geometry_duplicate_edge       canvas_geometry_duplicate_port
canvas_geometry_unknown_node         canvas_geometry_unknown_port
canvas_geometry_cell_out_of_range    canvas_geometry_self_containment
canvas_geometry_containment_cycle    canvas_geometry_input_too_large
canvas_geometry_drag_too_large       canvas_geometry_invalid_json
canvas_geometry_invalid_hit_code     canvas_geometry_invalid_buffer
```

Authored-graph JSON refuses unknown fields: a misspelled key would otherwise
become a silently missing node or edge. A drag is bounded three ways — a
non-finite delta is refused, a delta beyond `maxDragWorld` is refused, and a
destination outside the authored grid is clamped back into it and reported as
`clamped`. Clamping is the authority's stated decision, not a correction a host
has to detect.

Zoom outside `0.2 … 1e6` is clamped rather than refused: it is a camera
preference and every value in between describes a drawable view. A *non-finite*
zoom is still refused.

## Native and WASM run the same core

`ds-canvas-graph` is pure computation with no wasm dependency. Native callers use
`GeometrySession` directly; `ds-interactions-wasm` wraps that same type and owns
transport only — no layout, no camera math, no hit rule. A second implementation
in the shell is precisely what this migration removes.

```rust
let mut session = GeometrySession::open_json(authored_graph_json)?;
session.set_host(width, height, device_pixel_ratio)?;
let scene = session.scene_buffer();
let hit = session.hit_test_screen(x, y)?;
```

## JSON

JSON crosses once when the graph is opened, and again only for durable or debug
reads (`sceneJson`, `sceneIndexJson`, `canvasGraphAbi`). It is deliberately
absent from the interaction path.

## Consumer boundary

The matching ds-web Architecture integration opens one session per chapter,
decodes the versioned buffers, forwards pointer/camera intent, and paints only
the returned coordinates. It contains no TypeScript layout or viewport
fallback. ds-interactions and ds-web must therefore be released from mutually pinned
commits; an older browser package refuses the canvas with a version error.

Also out of scope for this slice, and deliberately so: ellipse, hyperbola,
parabola, spline and sweep geometry (the `CurveSegment` enum is the extension
point); snapping; edge-to-edge intersection and clipping; marquee/multi-select
geometry; authored-graph port visibility thresholds (free-card thresholds are owned above); preview-versus-committed connector
semantics; and node creation, deletion or reparenting. Each is a position, so
each belongs here when it is needed — none may be added in a host.
