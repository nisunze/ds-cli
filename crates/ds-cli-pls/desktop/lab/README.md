# PLS lab tools (not adapters)

These Python scripts write PLS-POLE and PLS-CADD input files from declarative
specs. They start no program, but unlike an adapter they carry engineering
logic, so they are kept apart from `../adapters/`:

| File | Engineering it carries |
|---|---|
| `pls-sagtension.py` | ruling-span sag-tension with creep as a temperature shift |
| `pls-wire-loads.py` | wire loads per structure family from cables and criteria |
| `pls-lic-write.py` | the PLS-POLE wire-loads (`.lic`) file writer |
| `pls-pole-author.py`, `pls-pole-family.py` | PLS-POLE `.POL` models, solver settings and inset views per family |
| `pls-components-author.py` | wood pole, material and insulator libraries |
| `pls-library-manifest.py` | `LIBRARY.json` for a canonical PLS library folder |
| `pls-make-fictional-sheet-assets.py` | fictional logos and an A3 border DXF for sheet tests |

Their owner should be a Rust crate in `ds-network`, reached through `ds`. No
`ds` verb runs them; they ship in `ds pls desktop toolkit` so nothing proven is
lost, and they go when the Rust owner lands.
