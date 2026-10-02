"""Write PLS-POLE component libraries (wood poles .wpp, materials .mat, insulators .inl) from a declarative JSON spec.

The canonical library's parts are data, not copies: this regenerates them byte-for-byte from `components.json`
(schema ds.lab.pls_components.v1). Grammar: the versions PLS-POLE 16.81 reads (WPOLE DIMS 9, MAT 10, Insulator
Library 11), learned from the component files PLS-POLE writes; units INTERNAL/SI (m, cm for pole diameters, N, MPa).

usage: pls-components-author.py <components.json> <out-dir>
"""
import json
import os
import sys

CRLF = "\r\n"


def q(s):
    return "'" + s.replace("'", "") + "'"


def head(kind, version, units, path):
    return (f"TYPE='{kind}' VERSION='{version}' UNITS='{units}' SOURCE='PLS-POLE Version 16.81' "
            f"USER='DS canonical library' FILENAME='{path}'")


def write(path, lines):
    with open(path, "w", newline="", encoding="latin-1") as f:
        f.write(CRLF.join(lines) + CRLF)


def wood_poles(spec, path):
    L = [head("WPOLE DIMS FILE", 9, "SI", path), str(len(spec["poles"])), "1.000000"]
    for p in spec["poles"]:
        # label, stock, species, class, length m, tip circumference, circumference at distance from butt,
        # tip diameter cm, base diameter cm, default embedment m, drag coefficient, strength check (2 calculated),
        # distance from tip, ultimate load kN
        L.append(f"{q(p['label'])} {q(p['stock'])} {q(p['species'])} {q(p['class'])} {p['length_m']:g} 0 0 "
                 f"{p['tip_diameter_cm']:g} {p['base_diameter_cm']:g} {p['embedment_m']:g} {p['drag_coefficient']:g} 2 0 "
                 f"{p['ultimate_load_kN']:g}")
    write(path, L)


def materials(spec, path):
    L = [head("MAT FILE", 10, "SI", path), f"{len(spec['materials'])} ; label, elasticity, yield stress, weight density"]
    for m in spec["materials"]:
        # label, E MPa, design stress (MOR) MPa, weight density N/m3, ANSI O5.1 included, allowable shear,
        # allowable compression, through-boring reduction %, distance above / below ground
        L.append(f"{q(m['label'])} {m['elasticity_MPa']:g} {m['design_stress_MPa']:g} {m['weight_density_N_m3']:g} "
                 "1 0 0 0.000000000000 0.000000000000 0.000000000000")
    write(path, L)


def insulators(spec, path):
    ins = spec["insulators"]
    L = [head("Insulator Library", 11, "INTERNAL", path), "CClampProperties",
         f"{len(ins.get('clamps', []))} ; label, stock #, holding cap"]
    for c in ins.get("clamps", []):
        L += [f"{q(c['label'])} {q(c['stock'])} {c['holding_capacity_N']:.8f}", "'' 0 0.00000000"]
    L += ["CStrainProperties", f"{len(ins.get('strains', []))} ; label, stock #, tension cap, length, wind area, weight, E L, E R"]
    for s in ins.get("strains", []):
        L += [f"{q(s['label'])} {q(s['stock'])} {s['tension_capacity_N']:.8f} {s['length_m']:.8f} {s['wind_area_m2']:.8f} "
              f"{s['weight_N']:.8f} 0.00000000 0.00000000", f"'' 0 {s.get('hardware_capacity_N', 0):.8f}"]
    L += ["CSuspensionProperties", "0 ; label, stock #, tension cap, length, wind area, weight, Top w h, Bot w h, Vert w h",
          "C2PartProperties",
          "0 ; label, stock #, left,right length, left,right wind area, left,right, weight, left,right,comp cap, left,right tens cap",
          "CPostProperties",
          f"{len(ins.get('posts', []))} ; label, stock #, has brace, horz dist, vert dist, vert up cap, vert down cap, "
          "axial comp cap, axial ten cap, long cap, weight, long, vert stiff"]
    for p in ins.get("posts", []):
        # label, stock, has brace, horizontal and vertical projection m, weight N, cantilever / tension / compression
        # capacities (0 when the interaction table is used), longitudinal and vertical stiffness
        L.append(f"{q(p['label'])} {q(p['stock'])} 0 {p['horizontal_m']:.8f} {p['vertical_m']:.8f} {p['weight_N']:.8f} "
                 "0.00000000 0.00000000 0.00000000 0.00000000 0.00000000")
        pts = p["interaction_capacity_N"]  # rows of (longitudinal, transverse, vertical)
        L.append(f"{len(pts)} ; # Capacities")
        L += [" ".join(f"{v:.12f}" for v in row) for row in pts]
        L.append("'' 0 0.00000000")
    L += ["CGuyStrainProperties", f"{len(ins.get('guy_strains', []))} ; label, stock #, Tension cap"]
    for g in ins.get("guy_strains", []):
        L += [f"{q(g['label'])} {q(g['stock'])} {g['tension_capacity_N']:.8f}", "'' 0 0.00000000"]
    write(path, L)


def tubular_xarms(spec, path):
    """TUBULAR XARM PROPERTIES FILE 11 (field order as PLS-POLE writes it, IBC components/default.xtm): label, stock,
    2, wall thickness m, outside diameter m, 1, yield Pa, length m, three capacities N, stiffness term, 0, 2, shape."""
    L = [head("TUBULAR XARM PROPERTIES FILE", 11, "INTERNAL", path), str(len(spec["xarms_tubular"]))]
    for x in spec["xarms_tubular"]:
        c1, c2, c3 = x["capacities_N"]
        L += [f"{q(x['label'])} {q(x['stock'])} 2 {x['thickness_m']:12g} {x['diameter_m']:12g} 1 {x['yield_Pa']:12g} "
              f"{x['length_m']:12g} {c1:12g} {c2:12g} {c3:12g} {x['stiffness_term']:12g} 0 2 'R'", "0"]
    write(path, L)


def braces(spec, path):
    """BRACE PROPERTIES FILE 11 (IBC components/default.brc): label, stock, 2, area m2, 0, depth m, 0, weight N/m,
    E Pa, 1, 0, 0, net area m2, yield Pa, I minor m4, I major m4, 1, 1, 0, 0, 2."""
    L = [head("BRACE PROPERTIES FILE", 11, "INTERNAL", path), str(len(spec["braces"]))]
    for b in spec["braces"]:
        L.append(f"{q(b['label'])} {q(b['stock'])} 2 {b['area_m2']:12g} 0 {b['depth_m']:12g} 0 {b['weight_N_m']:12g} "
                 f"{b['E_Pa']:12g} 1 0 0 {b['area_m2']:12g} {b['yield_Pa']:12g} {b['I_minor_m4']:12g} {b['I_major_m4']:12g} 1 1 0 0 2")
    write(path, L)


def guy_cables(spec, path):
    """SAPS CABLE PROPERTIES FILE 9 (IBC components/default.cab): label, stock, area m2, E Pa, weight N/m,
    diameter m, 1, thermal expansion 1/C, then the two limit percentages PLS writes (90 90)."""
    L = [head("SAPS CABLE PROPERTIES FILE", 9, "SI", path), str(len(spec["guy_cables"]))]
    for g in spec["guy_cables"]:
        L.append(f"{q(g['label'])} {q(g['stock'])} {g['area_m2']:g} {g['E_Pa']:g} {g['weight_N_m']:g} {g['diameter_m']:g} 1 "
                 f"{g['alpha_per_C']:g} {g.get('limit_pct', [90, 90])[0]:g} {g.get('limit_pct', [90, 90])[1]:g}")
    write(path, L)


def steel_poles(spec, path):
    """STEEL POLE PROPERTIES FILE 10 (IBC components/default.spp): label, stock, '0', '0', embedment m, base and tip
    diameter m, 0, 1, 0, 0, sides code, 0.2, tip load term N, 0, 0; then one tube: length m, wall m, 0, yield Pa;
    then the steel density line and no bolts."""
    L = [head("STEEL POLE PROPERTIES FILE", 10, "INTERNAL", path), str(len(spec["steel_poles"]))]
    for p in spec["steel_poles"]:
        L += [f"{q(p['label'])} {q(p['stock'])} '0' '0' {p['embedment_m']:12g} {p['base_diameter_m']:12g} "
              f"{p['tip_diameter_m']:12g} 0 1 0 0 {p.get('sides_code', 4)} 0.2 {p.get('tip_load_term_N', 40000):12g} 0 0",
              "1 ; # tubes",
              f"{p['length_m']:.12f} {p['thickness_m']:.12f} 0.000000000000 {p['yield_Pa']:.12f} 0.000000000000 0.000000000000 0.000000000000",
              f"'0' '' 0.000000 0.000000 0.000000 {p.get('density_N_m3', 76972.8378024):g}            0            0 0.000000 0.000000",
              "0 ; # bolts"]
    write(path, L)


def concrete_poles(spec, path):
    """CPOLE DIMS FILE 9 (IBC components/default.cpp): label, stock, description, 1, 4, length m, embedment m, base and
    tip diameter m, 0, two cover terms m, 1, two moduli Pa, density kg/m3, 0.25, 0, 0, a load term N, 0, 0, 0; then
    the rated capacity rows (none when only a nominal tip load is known)."""
    L = [head("CPOLE DIMS FILE", 9, "INTERNAL", path), str(len(spec["concrete_poles"]))]
    for p in spec["concrete_poles"]:
        L += [f"{q(p['label'])} {q(p['stock'])} {q(p['description'])} 1 4 {p['length_m']:12g} {p['embedment_m']:12g} "
              f"{p['base_diameter_m']:12g} {p['tip_diameter_m']:12g} 0 {p.get('cover_m', 0.08):12g} {p.get('cover_m', 0.08):12g} 1 "
              f"{p['E1_Pa']:12g} {p['E2_Pa']:12g} {p.get('density_kg_m3', 2500):12g} 0.25 0 0 {p.get('load_term_N', 20000):12g} 0 0 0",
              f"{len(p.get('capacities', []))} ; # Capacities"]
        L += [" ".join(f"{v:g}" for v in row) for row in p.get("capacities", [])]
    write(path, L)


WRITERS = {"wpp": wood_poles, "mat": materials, "inl": insulators, "xtm": tubular_xarms, "brc": braces,
           "cab": guy_cables, "spp": steel_poles, "cpp": concrete_poles}


if __name__ == "__main__":
    spec_path, out = sys.argv[1:3]
    spec = json.load(open(spec_path, encoding="utf-8"))
    files = spec["files"]
    for kind, fn in WRITERS.items():
        if kind in files:
            fn(spec, os.path.join(out, files[kind]))
            print("wrote", os.path.join(out, files[kind]))
