# Native PLS-CADD failure modes retained from the retired MCP surface

These rules were reproduced against PLS-CADD 16.81 and the DS Grid exchange
boundary. Use them as classifiers and stopping conditions, not as permission
to repair unknown native bytes by analogy.

## Container, project and coordinate identity

### Import the `.bak`, not a hand-unpacked lookalike

Strict ingest resolves the member/reference structure of the actual container.
A manually unpacked folder can leave absolute historical references that only
native Restore knows how to rewrite. Basename fallback is unsafe when two
members share a leaf.

Some historical DS candidates were ZIP wrappers whose inner member began with
`TYPE='***PLSBACKUPFILE***'`. DS exchange could read through that wrapper, but
native Restore could not. Classify the outer container before native handoff;
never assume “DS can read it” means “PLS can restore it.”

### Never guess CRS or vertical datum

Native PLS-CADD does not carry a reliable project CRS. Coordinate magnitude or
map location is not an EPSG proof. Rwandan Custom/EDCL projects have used:

```text
+proj=tmerc +lat_0=0 +lon_0=30 +k=0.9999 +x_0=500000 +y_0=5000000 +ellps=GRS80 +units=m +no_defs
```

Use it only when the project authority declares Custom/EDCL. Expected WGS84
location is independent audit evidence, not a substitute for the source CRS.

### Project movement is a lifecycle operation

Hand-changing the main project `FILENAME` to a relative path produced the
PLS-CADD “project moved” warning. The accepted sequence is: localize exact
dependencies, bind current absolute runtime paths, native Save, native Backup,
Restore elsewhere, then re-resolve. Canonical package paths and active native
runtime paths are both required and serve different purposes.

## PI, terrain and stationing

### PI is topology, not a label

PLS-CADD alignment PIs become ordered route nodes and edges in DS Grid. A
terrain row with feature text `PI` does not become an alignment vertex. If the
operator must move it with PLS-CADD's alignment tools, it must be emitted in
the native PI run.

The active project may contain stale earlier PI/staking blocks. Select the
last non-zero structurally valid active block; whole-file scans double-count
and ordinal joins across blocks corrupt identity.

### Route edits and structure stations move together

Deleting or moving a PI changes chainage. A route-only patch can strand
structures outside the new station range or silently alter their meaning.
Use the engine operation that restations placements while preserving absolute
XY when that is the intended invariant. Never rebuild the station cascade by
hand.

### PLS line-angle reports are incomplete evidence

On a reproduced 1,129-structure project, PLS-CADD reported line angle only at
52 structures coincident with an alignment PI and printed `0.00` elsewhere,
including large bends and taps. Structure loads still reflected real span
geometry. Do not overwrite authoritative angle data or infer route topology
from that report column alone.

### Terrain interpolation requires a trustworthy surface

The engine refuses insertion where effective ground coverage is absent. Keep
that refusal. On a real corridor, external DEM residuals swung about 22.5 m
and changed sign; subtracting one mean offset would have invented a deep valley
under the line. Validate against surveyed ties and distinguish datum error,
surface mismatch and isolated outliers before correction.

## Native file and library integrity

### A DS Grid round trip is not native validation

A historical pole-capacity writer inserted a fourth header comment where
PLS-CADD expected exactly three lines followed by a record count. DS Grid read
its own result successfully; PLS-CADD dropped the structure for that session
and continued producing reports. Native reopen plus expected structure counts
is mandatory after any emitted candidate.

Never edit into a workspace PLS-CADD has open. It caches structure resources
at project open, so later reports can remain byte-identical after a disk edit.

### Exact leaves and opaque resources are identity

Structure, cable, criteria and parts filenames—including case, `.012`, `.014`,
or no extension—are adopted identities. Never rename a structure to encode a
site fact, infer an extension, or regenerate approved native resource bytes
from DS Grid's reduced representation. A deliberate library change is a new
reviewed release with digest evidence and native reopen.

### Display, shading and strength are separate gates

Appending graphical-looking subdocuments to an analytical shell did not make
a valid shaded model; PLS-CADD refused at `0 ; number dxf/shp files attached`.
Likewise, a `.cri` file's presence is not proof that required strength cases
cover every active placement. Report native open, graphical envelope,
material identity, analytical method, criteria coverage, native check and
engineering approval independently.

### Fresh Restore is the portability test

Backup walks referenced files and can reveal dangling paths not visible during
ordinary open. Restore can also flatten or collide native member paths. A
handover is not accepted merely because a `.bak` exists: restore into a fresh
directory, reopen, and repeat reference/digest checks against restored bytes.

## Automation and reporting traps

- A long native report can outlive a short automation timeout. Do not let a
  timeout kill PLS-CADD and leave concurrent locked instances; start and
  collect as distinct operations when automation is explicitly authorized.
- Each report opens a new MDI pane while older panes remain. Identify the new
  or changed pane; “largest pane” can return a stale prior report.
- PLS-CADD 16.81 has native DXF, XML, KMZ and PFL export routes, but no native
  SHP export. Generate SHP through a separate verified DS/report boundary.
- Never use an arbitrary coordinate click to activate PLS-CADD. One reproduced
  click launched a full Structure Check and hundreds of criteria prompts.

## Acceptance statement

A final receipt keeps these dimensions independent:

1. deterministic DS container read/write;
2. native Restore and reopen;
3. reference closure and exact-leaf identity;
4. route/PI and terrain fidelity;
5. structure placement and stringing consistency;
6. material/method/criteria coverage;
7. native analysis/check results;
8. report artifact completeness and digests;
9. engineering approval and remaining unjudged scope.

No lower gate implies a higher one.
