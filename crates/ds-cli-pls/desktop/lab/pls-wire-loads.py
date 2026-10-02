"""Wire loads (.lic) for a structure family from the canonical cables and the project criteria - what PLS-CADD/Lite
would hand PLS-POLE ('Have program calculate maximum permissible tension based on limits in the criteria library').

Lite quits on the PLS host (see the pls-native-lab skill), so this reproduces its load step:
  - cable properties read from the cable files themselves (SI cable files, CABLE FILE 14/15);
  - stringing tension by AutoSag at the ruling span (largest tension meeting every catenary limit), then the
    tension at each structure load case for its cable condition (I initial, C after creep; L after load = initial
    for these linear-elastic cables, as PLS-CADD's own report shows) - pls-sagtension.py, validated against
    PLS-CADD 16.81's Ruling Span Sag Tension Report to 0.3 %;
  - unit weight, unit wind = pressure x diameter, insulator weight split half back / half ahead;
  - written with pls-lic-write.py.

usage: pls-wire-loads.py <loads-basis.json> <family-loads.json> <cables-dir> <out.lic>
       (also writes <out.lic>.json: every computed tension and the governing AutoSag limit per cable)
"""
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def _load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, file))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


st = _load("sagtension", "pls-sagtension.py")
lic = _load("licwrite", "pls-lic-write.py")


def read_cable(path):
    """Structural properties of an SI PLS cable file (the fields PLS-CADD's Cable Data Report prints)."""
    raw = open(path, "rb").read().decode("latin-1").split("\r\n")
    head = raw[0]
    if "TYPE='CABLE FILE'" not in head or "UNITS='SI'" not in head:
        raise SystemExit(f"{path}: expected an SI PLS cable file")
    area, dia, wt, rts = (float(raw[i]) for i in (6, 7, 8, 9))
    E = float(raw[13])
    alpha = float(raw[14]) / 100.0
    k = next(i for i, l in enumerate(raw) if l.strip().endswith("; cable_file_type"))
    model = int(float(raw[k + 1].split()[8]))
    shift = float(raw[k + 3])
    if model != 1:
        raise SystemExit(f"{path}: cable model {model}; this tool models 'linear elastic + creep temperature increase' (1)")
    return {"file": os.path.basename(path), "description": raw[1].strip(), "area_mm2": area, "diameter_mm": dia,
            "weight_N_m": wt * 10.0, "rts_N": rts * 10.0, "E_GPa": E, "alpha_per_C": alpha, "creep_shift_C": shift}


def main(basis_path, family_path, cables_dir, out):
    basis = json.load(open(basis_path, encoding="utf-8"))
    fam = json.load(open(family_path, encoding="utf-8"))
    S = basis["ruling_span_m"]
    weather = basis["weather"]
    record = {"ruling_span_m": S, "cables": {}}
    tens = {}
    for name in sorted({p["cable"] for p in fam["points"]}):
        props = read_cable(os.path.join(cables_dir, name))
        cab = st.Cable(props)
        L0, gov = st.autosag(cab, S, weather, basis["autosag"][name])
        t = st.table(cab, S, L0, 25.0, weather)
        tens[name] = (cab, t)
        record["cables"][name] = {"properties": props, "governing_autosag_limit": gov,
                                  "horizontal_tension_N": {w: {c: round(v["H_N"], 2) for c, v in d.items()} for w, d in t.items()}}
    cases = []
    for lc in basis["load_cases"]:
        cond = "I" if lc["condition"] in ("I", "L") else "C"
        pts = []
        sign = lc.get("wind_sign", 1)   # -1: explicit negative-wind case (PLS-POLE recommends these over a copy)
        for p in fam["points"]:
            cab, t = tens[p["cable"]]
            s = t[lc["weather"]][cond]
            ud = sign * s["wind_N_m"] * lc.get("wire_wind_factor", 1.0)
            label = f"{p['cable']} {p['set']}:{p['phase']}"
            # side: 'both' = a suspension or clamp point carrying the back and the ahead span (two rows, half the
            # insulator on each); 'back' / 'ahead' = one strain insulator of a dead-end set, carrying one span with
            # its whole insulator weight and wind (one row, TenL for back, TenR for ahead), as PLS-CADD/Lite writes
            # dead-end points (IBC 70_lic/j-w-toff.lic).
            side = p.get("side", "both")
            wind_on_insulator = sign * lc.get("structure_wind_Pa", 0.0) * p.get("insulator_wind_area_m2", 0.0)
            if side == "both":
                half = p.get("insulator_weight_N", 0.0) / 2.0
                pts.append([p["label"], cab.w, half, ud, wind_on_insulator / 2.0, s["H_N"], 0.0, label])      # back
                pts.append([p["label"], cab.w, half, ud, wind_on_insulator / 2.0, 0.0, s["H_N"], p["cable"]])  # ahead
            elif side in ("back", "ahead"):
                full = p.get("insulator_weight_N", 0.0)
                ten = (s["H_N"], 0.0) if side == "back" else (0.0, s["H_N"])
                pts.append([p["label"], cab.w, full, ud, wind_on_insulator, ten[0], ten[1], label])
            else:
                raise SystemExit(f"point {p['label']}: side must be both, back or ahead, not {side!r}")
        cases.append({"name": lc["name"], "neg_wind": lc.get("neg_wind", 0), "add_insulator": 0,
                      "strength_factors": basis["strength_factors"], "structure_weight_factor": 1.0,
                      "wind_pressure_pa": [sign * lc.get("structure_wind_Pa", 0.0), 0.0], "points": pts})
    spec = {"description": f"{fam['family']} wire loads: {basis['name']} (ruling span {S:g} m)",
            "structure_height_m": fam["structure_height_m"], "line_angles_deg": fam["line_angles_deg"],
            "span_ratio": 1.0, "check_neg_wind": 0, "min_weight_span_m": fam["min_weight_span_m"],
            "wind_span_search_limit_m": fam["wind_span_search_limit_m"], "conditions": basis["conditions"],
            "load_cases": cases}
    lic.write(spec, out)
    record["lic_spec"] = spec
    json.dump(record, open(out + ".json", "w", encoding="utf-8"), indent=1)
    for name, r in record["cables"].items():
        g = r["governing_autosag_limit"]
        print(f"{name}: AutoSag governed by {g['weather']} {g['condition']} <= {g['max_catenary_m']} m; "
              f"EDT initial H {r['horizontal_tension_N']['EDT']['I']:.0f} N")
    print(f"wrote {out}")


if __name__ == "__main__":
    main(*sys.argv[1:5])
