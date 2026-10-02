"""Author a PLS-POLE 16.81 input model (.POL) for any MV structure family from a declarative spec (v2).

Generalises pls-pole-author.py (single wood pole, family a) to every family of the canonical library: H-frames and
single poles of wood, steel or concrete; tubular cross-arms with named points bolted to pole joints; braces and straps;
guys (stays); clamp, post and strain insulators with their set/phase links; dead loads such as a transformer; and the
Method 1 / Method 2 run options. The grammar is the 20.3 input section PLS-POLE 16.81 reads, line for line as in the
16.81-readable references (line_optimization_lab_16.81/method4-16.81/_transcoded-v20.3); engineering values come from
the spec only (approved drawings + GTP parts, per the family dossiers), never from those references.

usage: pls-pole-family.py <spec.json> <components-dir> <out.POL> [--option N] [--lic <wire-loads.lic>]
Spec schema ds.lab.pls_pole_family.v2 (see specs/*.json in the line optimization lab):
  family, title, notes, components {kind: file}, analysis {type, max_iterations, points_on_cable, option, lic,
  interaction "show user neg min max inc", convergence {max_imbalance, ascorm, dasat, pwiter, min_stiffness}},
  poles [{label, material wood|steel|concrete, property, x_m, y_m,
  attachments {name: depth_below_top_m}}], selection {wood|steel|concrete: [[property, 0|1], ...]}, species (wood),
  xarms [{label, property, length_m, azimuth_deg? (90 = along the line), points [{name, at_m, pole_joint?}]}],
  braces [{label, from, to, property}],
  guys [{label, joint, anchor, property, azimuth_deg, slope_deg, tension_pct}], clamps / posts / strains
  [{label, joint, tip, property, azimuth_deg}], links [{insulator, tip, set_name, type, set, phase, dead_end}],
  dead_loads [{name, joint, load_N, tran_wind_area_m2, long_wind_area_m2}], parts_list [[stock, qty, auto]].
Insulator link types: 0 clamp, 1 strain, 4 post.
"""
import importlib.util
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("author_v1", os.path.join(HERE, "pls-pole-author.py"))
v1 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(v1)

CRLF = "\r\n"
JOINT_HELP = v1.JOINT_HELP
CONN_HELP = v1.CONN_HELP
q = v1.q
REL_HELP = r"joint label\nrestraint 1-6\nx,y,z\nhas master, symmetry code, relative dist\nfrom, to joint, is secondary, fraction"


def rel_attach(label, depth):
    return [q(label), "0 0 0 0 0 0", "0 0 0 0", f"0 0 {depth:g}", "'' '' 0 0 0 0"]


def rad(deg):
    return math.radians(deg)


# Nonlinear solver settings (name, default). IBC's stayed models run max imbalance 0.1, ascorm 1000, dasat 1e8 with
# 1000 iterations: a single pole held by one guy does not converge under wind with the 16.81 defaults.
CONVERGENCE = (("max_imbalance", 0.1), ("ascorm", 1.0), ("dasat", 1000000.0), ("pwiter", 3.0), ("min_stiffness", 0.0))


def pole_block(spec, material, lib):
    """One pole-material block: connectivity, each pole of that material, then its multiple-pole selection."""
    poles = [p for p in spec["poles"] if p["material"] == material]
    title = {"steel": "Steel Pole", "concrete": "Concrete Pole", "wood": "Wood Pole"}[material]
    L = [lib, f"{len(poles)} ; {title} Connectivity: {CONN_HELP}"]
    for p in poles:
        att = p["attachments"]
        rel_title = "Relative Attachment Labels and Holes for" if material == "wood" else "Relative Attachment Labels for"
        L += [p["label"], "", "", p["property"], f"{len(att)} ; {rel_title}: {REL_HELP}"]
        for name, depth in att.items():
            L += rel_attach(f"{p['label']}:{name}", depth)
        L += [f"           0 {p.get('x_m', 0.0):12g} {p.get('y_m', 0.0):12g}            0            0",
              "0            0            0"]
        if material == "concrete":
            L.append("           0            0")
        if material == "wood":
            L += [spec["species"], "           0            0 0 0.000000 0.000000 0.000000 2 0.000000 0.000000 '' ''"]
    sel = spec.get("selection", {}).get(material, [])
    L.append(str(len(sel)))
    L += [f"{q(n)} {flag}" for n, flag in sel]
    if material == "wood":
        L.append("0  ; wood pole strength check")
    return L


def author(spec, comp, out):
    lib = spec.get("components", {})
    c = lambda kind: os.path.join(comp, lib[kind]) if lib.get(kind) else ""  # noqa: E731
    a = spec["analysis"]
    cv = a.get("convergence", {})
    L = [
        f"TYPE='PLS_POLE INPUT FILE' VERSION='20.3' UNITS='INTERNAL' SOURCE='PLS-POLE Version 16.81' "
        f"USER='DS canonical library (authored from spec {spec['family']})' FILENAME='{out}'",
        "0 ; use edf suffixes", spec["title"], spec.get("notes", ""),
        "0 0 ; write saps sum, page length",
        "1.225000 0.000000 1e-08 ; rho, input temp, EP1",
        f"{a['type']} {a['max_iterations']} {a['points_on_cable']} ; analysis type, max iterations, # points on cable",
        " ".join(f"{cv.get(k, v):.10f}" for k, v in CONVERGENCE) + " ; max imbalance, ascorm, dasat, pwiter, min stiffness",
        f"0 ; Joints Geometry: {JOINT_HELP}", "",
        "0 ; number truss properties", "", "0  ; number beam properties", "", "0  ; number cable properties", "",
        "0 ; number subs properties",
        f"{a.get('option', 0)} 0 1 0 0 0.000000 0; analysis option, print rotations, echo input, gen diffs, load type to use, followed by .lca, .lic, .eia filename lines",
        a.get("lca", ""), a.get("lic", ""), a.get("eia", ""),
        "9 0 0",
        f"{a.get('interaction', '0 1 0 0.01 2 0.25')} ; show ID views, use user ID ratios, use negative ratios, min ratio, max ratio, increment",
        "1 1 1 1 1 ; offset: arms, braces, guys, posts, strains",
        "1 2 0 3 ; auto add to parts list, part add action",
        "1000.000000000000 0.142857142857 0.000000000000 ; wind reference height, power ground elevation",
        "'' '' '' 0 1 1 0 1 1 1 1 0 0 0 0 0 1 0 1 1 0 0 0 131072; postproc exe name, post proc output name, post proc cmd line, post proc options, postproc name options, post proc sheet options",
        "''", "''", "", "",
        c("ssl"),
        c("cab"), f"0 ; Structure Cable Connectivity: {CONN_HELP}",
        c("cab"), f"{len(spec.get('guys', []))} ; Guy Connectivity: {CONN_HELP}",
    ]
    for n, g in enumerate(spec.get("guys", []), 1):
        L += [g["label"], g["joint"], g.get("anchor", f"$Gnd{n}"), g["property"],
              "           0            0            0            0 3; x, y, z, lead length",
              f"{rad(g['azimuth_deg']):.11g} {rad(g['slope_deg']):.12g} {g.get('tension_pct', 2):12g} ; azimuth, slope, % tension",
              "'' ; shared anchor"]
    L += [c("brc"), f"{len(spec.get('braces', []))} ; Brace Connectivity: {CONN_HELP}"]
    for b in spec.get("braces", []):
        L += [b["label"], b["from"], b["to"], b["property"], "0 ; is fuse"]
    L += [c("dvt"), f"0 ; Davit Arm Connectivity: {CONN_HELP}",
          c("tdv"), f"0 ; Tubular Davit Arm Connectivity: {CONN_HELP}",
          c("xrm"), f"0 ; X-Arm Connectivity: {CONN_HELP}",
          c("xtm"), f"{len(spec.get('xarms', []))} ; Tubular X-Arm Connectivity: {CONN_HELP}"]
    for x in spec.get("xarms", []):
        pts = sorted(x["points"], key=lambda p: p["at_m"])
        inner = [p for p in pts if 0.0 < p["at_m"] < x["length_m"]]
        L += [x["label"], "", "", x["property"], f"{len(inner)} ; Attachment Labels Relative to: {REL_HELP}"]
        for p in inner:
            L += rel_attach(f"{x['label']}:{p['name']}", p["at_m"])
        ends = [{"name": "O", "at_m": 0.0}] + inner + [{"name": "E", "at_m": x["length_m"]}]
        L += [f"{rad(x.get('azimuth_deg', 0.0)):12.11g} {0:12g}", str(len(ends))]
        L += [f"{q(x['label'] + ':' + p['name'])} {q(p.get('pole_joint', ''))} {p['at_m']:12g} 2" for p in ends]
    L += [c("eqp"), f"0 ; Equipment Connectivity: {CONN_HELP}",
          c("can"), f"0 ; CAN Connectivity: {CONN_HELP}",
          "0 ; attach label, desc",
          c("mat"),
          f"{len(spec.get('dead_loads', []))} ; load name, attach label, dead load, tran wind area, long wind area"]
    for d in spec.get("dead_loads", []):
        L.append(f"{q(d['name'])} {q(d['joint'])} {d['load_N']:g} {d.get('tran_wind_area_m2', 0):g} {d.get('long_wind_area_m2', 0):g}")
    L.append("0 ; appurt name, from joint, to joint, is flat, is inside, unit weight, width, perimeter")
    L.append(f"{len(spec['poles'])} ; support label, long shear, tran shear, comp, uplift, long moment, tran moment, torsional moment, resultant")
    for p in spec["poles"]:
        L += [f"{q(p['label'] + ':g')}" + "            0" * 10, "           0" * 6,
              f"0 {p.get('embedment_m', 1.8):12g}            0 0            0 0 0            0"]
    L += ["0.076200000000 27578979.301976032555 0.152400000000 0.152400000000 0.005000000000 413684689.529640495777 0",
          c("inl"), f"{len(spec.get('clamps', []))} ; Clamp Insulator Connectivity"]
    for k in spec.get("clamps", []):
        L.append(f"{q(k['label'])} {q(k['joint'])} {q(k['tip'])} {q(k['property'])} 0 0.000000 ; label, str attach joint, tip label, property")
    L += ["0 ; Suspension Insulator Connectivity", f"{len(spec.get('strains', []))} ; Strain Insulator Connectivity"]
    for s in spec.get("strains", []):
        L += [f"{q(s['label'])} {q(s['joint'])} {q(s['tip'])} {q(s['property'])} 0 0.000000 ; label, str attach joint, tip label, property",
              f"{rad(s['azimuth_deg']):.6f} ; azimuth"]
    L.append(f"{len(spec.get('posts', []))} ; Post Insulator Connectivity")
    for p in spec.get("posts", []):
        L += [f"{q(p['label'])} {q(p['joint'])} {q(p['tip'])} {q(p['property'])} 1 0.000000 ; label, str attach joint, tip label, property",
              f"'' {rad(p.get('azimuth_deg', 0.0)):.8f} ; brace attach label, azimuth"]
    L += ["0 ; 2-Parts Insulator Connectivity", "0 ; Guy Strain Insulator Connectivity",
          f"{len(spec['links'])} ; insulator link: insulator label, tip label, set name, insulator type, set #, phase #, is dead end"]
    for k in spec["links"]:
        L.append(f"{q(k['insulator'])} {q(k['tip'])} {q(k['set_name'])} {k['type']} {k['set']} {k['phase']} {k.get('dead_end', 0)} ''")
    L += pole_block(spec, "steel", c("spp"))
    L += pole_block(spec, "concrete", c("cpp"))
    L += pole_block(spec, "wood", c("wpp"))
    L += [c("lpp"), f"0 ; Laminated Wood Pole Connectivity: {CONN_HELP}", "0",
          c("fpp"), f"0 ; FRP Pole Connectivity: {CONN_HELP}", "0",
          c("mas"), f"0 ; Mast Connectivity: {CONN_HELP}", f"0 ; Vang Connectivity: {CONN_HELP}",
          c("frm"), f"0 ; Framing Connectivity: {CONN_HELP}",
          "0 ; defect desc, attach, azimuth, diameter/width, long left, tran left, defect type, radial offset",
          "3 1 1 1", "0 1.524 1 0 1 1", "15.555555555556"]
    prov = spec.get("provenance", {})
    if isinstance(prov, list):   # v2 specs list {topic, text}; the notes report takes topic -> text
        prov = {p["topic"]: p["text"] for p in prov}
    L += v1.layout({**spec, "provenance": prov}, out)
    with open(out, "w", newline="", encoding="latin-1") as f:
        f.write(CRLF.join(L) + CRLF)
    return len(L)


if __name__ == "__main__":
    args = sys.argv[1:]
    over = {}
    for flag in ("--option", "--lic"):
        if flag in args:
            k = args.index(flag)
            over[flag[2:]] = int(args[k + 1]) if flag == "--option" else args[k + 1]
            del args[k:k + 2]
    spec_path, comp, out = args
    spec = json.load(open(spec_path, encoding="utf-8"))
    spec["analysis"].update(over)
    print(f"authored {out}: {author(spec, comp, out)} lines")
