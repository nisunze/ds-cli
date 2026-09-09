# Engineering print composition

These are decision criteria, not universal fixed settings. Choose and persist
values in the live CLI recipe; consult its current schema for supported fields.

## Page and visual hierarchy

Start with the map's purpose. For an MV delivery, planned routes, transformers,
angle points and their canonical labels usually lead. Roads, hydrology, relief
and settlement names orient the reader. Hospitals, airports, universities and
schools can be essential landmarks; select their prominence by function and
scale rather than discarding every point-of-interest layer or naming special
cases in code. A district overview and an urban four-sector composition need
different density even on the same paper.

Give the map the largest useful frame. Put compact legends, tables, scale and
provenance at an edge; do not park a table over central geography just because
label placement found a gap. Judge both actual paper size and a page overview:
readable labels can still overwhelm the network when viewed together. Excess
blank furniture space and a crowded map are a layout defect, not a reason to
omit geographic context.

Use differences in line weight, saturation, opacity, symbol size and label
priority deliberately. Retain category interior colours where they carry
meaning; status halos are a separate visual channel. Group legend categories
beneath their owning layer and make the legend agree with rendered styles.

## Relief, water and landmarks

Hillshade is a subdued backdrop derived from a DEM, not arbitrary decoration.
Use measured terrain, a declared source/version and a reproducible illumination
recipe. Preserve vector network lines and text over the raster relief. Inspect
flat areas, steep terrain, tile seams and nodata edges. DEM resolution and print
DPI are different controls; higher DPI cannot create terrain detail absent in
the source. Never substitute a live-map hillshade screenshot for retained print
inputs.

Choose contour intervals for terrain and map scale. Wider intervals may be more
readable even on A0 when the geographic extent is large. Keep index contours
stronger than intermediate contours and subordinate to planned infrastructure.
Do not assume a larger sheet automatically needs more contour lines.

Show wetlands/marshland from an identified geographic source throughout the
actual map frame, including margins. A river line or valley-shaped hillshade is
not a wetland polygon. Inspect source coverage, classifications and attribution
before claiming complete marshland representation. Pale fills or sparse patterns
should make marshes distinguishable without masking routes and symbols.

Prioritize landmark names by their orienting value. Use collision-aware placement
and deliberate label hierarchy instead of uniformly shrinking all text. Preserve
important planned transformer and PI labels; reduce competing background labels
before making engineering text unreadable. If labels cannot fit, use an authored
inset, larger paper or a companion sheet where the current contract supports it.

## Network identity

A numbering hierarchy requires a known source/root, connectivity, junction roles
and a deterministic ordering policy. Detect cycles and disconnected components;
a traversal alone does not prove a radial design. NetworkX traversal and DAG
references help independent validation, but the production network owner remains
the authority. Reconcile canonical engineering numbers before printing; never
renumber to make labels look tidy, and never label opaque row IDs as PI numbers.

## Visual acceptance

Check a whole-page view, a dense urban crop, a rural crop and a route junction.
Can a reader follow the planned MV route immediately, distinguish transformers
from ordinary poles, read the important numbers, and locate named landmarks?
Does subdued context still explain terrain, access and wet ground? Are tables and
legends at the edges with enough map space? Verify multiple districts and random
transformer sheets, not only a repeatedly tuned favourite.

Keep local generation, preview, publication and cross-machine visibility as
separate observed outcomes. Recipe changes should reuse fresh held data and
replace only the selected canonical output.

## References

- [Ordnance Survey: cartographic purpose, hierarchy and composition](https://www.ordnancesurvey.co.uk/blog/what-is-cartography).
- [Esri: composing custom hillshade](https://learn.arcgis.com/en/projects/illuminate-terrain-with-a-custom-hillshade/).
- [Esri: hillshade calculation and terrain units](https://pro.arcgis.com/en/pro-app/3.3/tool-reference/spatial-analyst/how-hillshade-works.htm).
- [Rwanda Environment Management Authority: wetlands](https://www.rema.gov.rw/wetlands). Source discovery starts here; a descriptive page does not itself supply complete GIS coverage.
- [NetworkX traversal algorithms](https://networkx.org/documentation/stable/reference/algorithms/traversal.html) and [DAG algorithms](https://networkx.org/documentation/stable/reference/algorithms/dag.html).
