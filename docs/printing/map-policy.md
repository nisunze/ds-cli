# Governed map printing

Every map consumes the held project's `project_settings`, independently of its
paper, template, language or print purpose. The published defaults are:

| Parameter | Default | Authored value |
| --- | --- | --- |
| `keep_all_labels_at_high_density` | false | Boolean |
| `print_metric_grid_enabled` | true | Boolean |
| `print_metric_grid_interval_m` | automatic | Positive metres, or null |

False density preserves collision culling. True density retains every requested
label; when no position fits inside the frame the job refuses explicitly instead
of silently dropping that label. Label leaders and offsets are presentation.

A metric grid takes its CRS from the normalized authenticated project's
`ProjectParams.crs`, captured once alongside the print inputs. This is not a
second project setting. A held `ds.print-project-crs/v1` capsule binds exact
project parameters and their SHA256 to project, lane, principal and credential
audience. The host validates that identity on replay. Reporter admits supported
projected metre CRS through the geometry owner; missing or unsupported authority
refuses an enabled grid. A disabled grid needs no CRS. Neither this capsule nor
local metadata changes a server-issued sheets digest; the complete print-context
bytes, including the capsule, have their own source digest.

Automatic spacing uses the actual frame's transformed metric extent and a round
1, 2, 2.5 or 5 times a power of ten, targeting four to eight crossings. At ordinary
LV scales these include 100, 200, 250 and 500 metres; larger district frames and
small detail frames require other powers of ten. A finite positive explicit
interval overrides the automatic choice. Grid pen and typography use the shared
quiet graticule defaults, below map labels.

CLI settings edits use the existing project-governed design configuration
boundary. Only these named published parameters may create a previously absent
row; unknown settings remain refused. Source-only model edits do not mutate a
project. Production writes still require the operator's authorization.
