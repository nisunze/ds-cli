"""Author a PLS-POLE 16.81 input model (.POL) for one wood-pole family from a declarative spec.

The owner's route (2026-09-26): author structures natively in our stack from known parts and geometry, not by
converting 20.01 files. This writes the outer PLS_POLE INPUT FILE section in the grammar PLS-POLE 16.81 reads
(version 20.3, as its shipped example tangent_directembed.pol) and the sheet and notes sub-documents it requires;
PLS-POLE 16.81 then opens it and saves it natively, writing its own derived sub-documents (STRUCT, parts, plot,
library snapshots).

usage: pls-pole-author.py <spec.json> <components-dir> <out.POL> [--option N] [--lic <wire-loads.lic>]
Spec schema ds.lab.pls_pole_family.v1: see specs/a-w.json in the line optimization lab.
"""
import json
import os
import sys

CRLF = "\r\n"
JOINT_HELP = r"joint label\nrestraint 1-6\nx,y,z\nhas master, symmetry code, relative dist\nfrom, to joint, is secondary, fraction"
CONN_HELP = r"label\n origin\n joint\n property "


def q(s):
    return "'" + s.replace("'", "") + "'"


EMPTY_ANNOTATION = ["0 ; Number annotation records", "-1 -1 -1 -1 -1 0 -1 -1 -1 0 '' 0", "''", "''", "'' ''"]
# Inset structure views of a PLS-POLE sheet (16.81 grammar, INSET STRUCT VIEWS version 1): the four views IBC's
# family models carry (placement on the sheet in %, view angles, 36.576 m = 120 ft view extent in SI).
INSET_VIEWS = [
    ("0  55.0000 95.0000 10.0000 85.0000  0.0000 0.0000 0.0000  70.0000 120.0000  0.6000 ''", "36.5760 1 0", "Isometric", "1 0"),
    ("0  8.0000 38.0000 4.0000 25.0000  0.0000 0.0000 0.0000  0.0000 90.0000  0.6000 ''", "36.5760 4 0", "Plan", "1 1"),
    ("0  8.0000 38.0000 30.0000 85.0000  0.0000 0.0000 0.0000  90.0000 90.0000  0.6000 ''", "36.5760 4 0", "Longitudinal", "1 1"),
    ("0  40.0000 55.0000 30.0000 85.0000  0.0000 0.0000 0.0000  90.0000 0.0000  0.6000 ''", "36.5760 4 0", "Transverse", "1 1"),
]
REPORT_STYLES = ["Annotation", "Inset Report Lines", "Inset Report Text", "Sheet Border", "Inset View", "Structure Inset View"]
REPORT_STYLE_PEN = {"Annotation": "0.3175 0 0 0 0", "Inset Report Lines": "0 0 0 0 16777215",
                    "Inset Report Text": "0 0 0 0 16777215", "Sheet Border": "0.3175 0 0 0 0",
                    "Inset View": "0.8382 0 0 0 0", "Structure Inset View": "0.3175 0 0 0 0"}


def rtf_escape(s):
    s = s.replace("\\", "\\\\").replace("{", "\\{").replace("}", "\\}")
    return "".join(ch if ord(ch) < 256 else f"\\u{ord(ch)}?" for ch in s)


def layout(spec, out):
    """Parts list, sheet and notes sub-documents PLS-POLE 16.81 expects after the input section. No engineering:
    material list, page size, the four inset views, report styles and the notes report."""
    def head(kind, version, filename=out):
        return (f"TYPE='{kind}' VERSION='{version}' UNITS='SI' SOURCE='PLS-POLE Version 16.81' "
                f"USER='DS line optimization lab' FILENAME='{filename}'")
    # The parts list is the one sub-document 16.81 needs before the sheets (probed 2026-09-26: without it the
    # reader never finds 'STRUCTURE SHEETS' and PLS-POLE later crashes). No user columns; one structure assembly.
    parts = spec.get("parts_list", [])
    L = [head("PRT FILE", 4, ""),
         "0 0 13; total number of columns (including user defined columns), sort column, and number of protected columns",
         f"0 {1 if parts else 0} 0 ; number of parts, assemblies, and resolve-list items"]
    if parts:
        L.append(f"{len(parts)} 0")
        for stock, qty, auto in parts:
            L += [stock, f"{qty} {auto} 0"]
        L.append("0 0")
    L += ["0; number of custom counting units"]
    L += [head("DXF ATTACHMENTS", 3), "0 ; number dxf files attached",
         head("BMP ATTACHMENTS", 5), "0 1.000000 ; number bmp files attached, minimum pixel size",
         head("ANNOTATION", 8), *EMPTY_ANNOTATION, head("ANNOTATION", 8), *EMPTY_ANNOTATION,
         head("STRUCTURE SHEETS", 2), "0  86.3600 55.8800  1 0 2 0",
         head("INSET STRUCT VIEWS", 1), f"{len(INSET_VIEWS)} ; number of inset views"]
    for place, extent, name, flags in INSET_VIEWS:
        L += [place, extent, name, f"{flags}  1  36.5760 36.5760  2 5 0 0", head("ANNOTATION", 8), *EMPTY_ANNOTATION]
    L += [head("INSET REPORT VIEWS", 1), "0 ; number of inset views"]
    L += [f"0 0 0 0 0   {REPORT_STYLE_PEN[s]}  '' '' '{s}' '{s}'" for s in REPORT_STYLES]
    notes = [f"{spec['family']}: {spec['title']}", "",
             "Authored in PLS-POLE 16.81 from the declarative spec by pls-pole-author.py (DS line optimization lab).", ""]
    for k, v in spec.get("provenance", {}).items():
        notes += [f"{k}: {v}", ""]
    L += [head("RTF FILE", 1),
          r"{\rtf1\ansi\ansicpg1252\deff0\deflang1033{\fonttbl{\f0\fmodern\fprq1\fcharset0 Courier New;}}",
          r"{\colortbl ;\red0\green0\blue0;}",
          r"\viewkind4\uc1\pard\cf1\f0\fs20 " + rtf_escape(notes[0]) + r"\par"]
    L += [rtf_escape(n) + r"\par" for n in notes[1:]]
    L += ["}"]
    return L


def author(spec, comp, out):
    # component libraries: spec "components" maps a library kind (wpp, inl, mat, ...) to a file in <components-dir>;
    # kinds it omits are written blank (PLS-POLE reads blank library paths). Without "components": default.<kind>.
    lib = spec.get("components")
    c = lambda kind: (os.path.join(comp, lib[kind]) if lib.get(kind) else "") if lib is not None else os.path.join(comp, f"default.{kind}")  # noqa: E731
    a = spec["analysis"]
    L = [
        f"TYPE='PLS_POLE INPUT FILE' VERSION='20.3' UNITS='INTERNAL' SOURCE='PLS-POLE Version 16.81' "
        f"USER='DS line optimization lab (authored from spec {spec['family']})' FILENAME='{out}'",
        "0 ; use edf suffixes",
        spec["title"],
        spec.get("notes", ""),
        "0 0 ; write saps sum, page length",
        "1.225000 0.000000 1e-08 ; rho, input temp, EP1",
        f"{a['type']} {a['max_iterations']} {a['points_on_cable']} ; analysis type, max iterations, # points on cable",
        "0.1000000000 1.0000000000 1000000.0000000000 3.0000000000 0.0000000000 ; max imbalance, ascorm, dasat, pwiter, min stiffness",
        f"0 ; Joints Geometry: {JOINT_HELP}", "",
        "0 ; number truss properties", "",
        "0  ; number beam properties", "",
        "0  ; number cable properties", "",
        "0 ; number subs properties",
        # analysis option = General Data radio: 0 design check, 1 basic allowable spans, 2 create a Method 1 file,
        # 3 allowable-span interaction diagrams, 4 create a Method 2 file
        f"{a.get('option', 0)} 0 1 0 0 0.000000 0; analysis option, print rotations, echo input, gen diffs, load type to use, followed by .lca, .lic, .eia filename lines",
        a.get("lca", ""), a.get("lic", ""), a.get("eia", ""),
        "9 0 0",
        f"{a.get('interaction', '0 0 1 -0.5 2 0.25')} ; show ID views, use user ID ratios, use negative ratios, min ratio, max ratio, increment",
        "1 1 1 1 0 ; offset: arms, braces, guys, posts, strains",
        "1 2 0 3 ; auto add to parts list, part add action",
        "1000.000000000000 0.142857142857 0.000000000000 ; wind reference height, power ground elevation",
        "'' '' '' 0 1 1 0 1 1 1 1 0 0 0 0 0 1 0 1 1 0 0 0 131072; postproc exe name, post proc output name, post proc cmd line, post proc options, postproc name options, post proc sheet options",
        "''", "''", "", "",
        c("ssl"),
        c("cab"), f"0 ; Structure Cable Connectivity: {CONN_HELP}",
        c("cab"), f"0 ; Guy Connectivity: {CONN_HELP}",
        c("brc"), f"0 ; Brace Connectivity: {CONN_HELP}",
        c("dvt"), f"0 ; Davit Arm Connectivity: {CONN_HELP}",
        c("tdv"), f"0 ; Tubular Davit Arm Connectivity: {CONN_HELP}",
        c("xrm"), f"0 ; X-Arm Connectivity: {CONN_HELP}",
        c("xtm"), f"0 ; Tubular X-Arm Connectivity: {CONN_HELP}",
        c("eqp"), f"0 ; Equipment Connectivity: {CONN_HELP}",
        c("can"), f"0 ; CAN Connectivity: {CONN_HELP}",
        "0 ; attach label, desc",
        c("mat"),
        "0 ; load name, attach label, dead load, tran wind area, long wind area",
        "0 ; appurt name, from joint, to joint, is flat, is inside, unit weight, width, perimeter",
    ]
    s = spec["support"]
    L += [
        "1 ; support label, long shear, tran shear, comp, uplift, long moment, tran moment, torsional moment, resultant",
        f"{q(s['label'])}" + "            0" * 10,
        "           0" * 6,
        f"0          {s['embedment_m']}            0 0            0 0 0            0",
        "0.076200000000 27578979.301976032555 0.152400000000 0.152400000000 0.005000000000 413684689.529640495777 0",
        c("inl"),
        f"{len(spec.get('clamps', []))} ; Clamp Insulator Connectivity",
    ]
    for k in spec.get("clamps", []):
        L.append(f"{q(k['label'])} {q(k['joint'])} {q(k['tip'])} {q(k['property'])} 0 0.000000 ; label, str attach joint, tip label, property")
    L += ["0 ; Suspension Insulator Connectivity", "0 ; Strain Insulator Connectivity",
          f"{len(spec.get('posts', []))} ; Post Insulator Connectivity"]
    for p in spec.get("posts", []):
        L.append(f"{q(p['label'])} {q(p['joint'])} {q(p['tip'])} {q(p['property'])} 1 0.000000 ; label, str attach joint, tip label, property")
        L.append(f"'' {p['azimuth_rad']:.8f} ; brace attach label, azimuth")
    L += ["0 ; 2-Parts Insulator Connectivity", "0 ; Guy Strain Insulator Connectivity",
          f"{len(spec['links'])} ; insulator link: insulator label, tip label, set name, insulator type, set #, phase #, is dead end"]
    for k in spec["links"]:
        L.append(f"{q(k['insulator'])} {q(k['tip'])} {q(k['set_name'])} {k['type']} {k['set']} {k['phase']} 0 ''")
    w = spec["wood_pole"]
    L += [c("spp"), f"0 ; Steel Pole Connectivity: {CONN_HELP}", "0",
          c("cpp"), f"0 ; Concrete Pole Connectivity: {CONN_HELP}", "0",
          c("wpp"), f"1 ; Wood Pole Connectivity: {CONN_HELP}",
          w["label"], "", "", w["property"],
          f"{len(w['attachments_from_top_m'])} ; Relative Attachment Labels and Holes for: {JOINT_HELP}"]
    for lab, d in w["attachments_from_top_m"].items():
        L += [q(lab), "0 0 0 0 0 0", "0 0 0 0", f"0 0 {d}", "'' '' 0 0 0 0"]
    L += ["           0            0            0            0            0",
          "0            0            0",
          w["species_line"],
          "           0            0 0 0.000000 0.000000 0.000000 2 0.000000 0.000000 '' ''",
          str(len(w["multiple_pole_selection"]))]
    L += [f"{q(n)} {flag}" for n, flag in w["multiple_pole_selection"]]
    L += ["0  ; wood pole strength check",
          c("lpp"), f"0 ; Laminated Wood Pole Connectivity: {CONN_HELP}", "0",
          c("fpp"), f"0 ; FRP Pole Connectivity: {CONN_HELP}", "0",
          c("mas"), f"0 ; Mast Connectivity: {CONN_HELP}", f"0 ; Vang Connectivity: {CONN_HELP}",
          c("frm"), f"0 ; Framing Connectivity: {CONN_HELP}",
          "0 ; defect desc, attach, azimuth, diameter, long left, tran left",
          "3 1 1 1", "0 1.524 0 0 0 1", "15.555555555556"]
    L += layout(spec, out)
    with open(out, "w", newline="", encoding="latin-1") as f:
        f.write(CRLF.join(L) + CRLF)
    return len(L)


if __name__ == "__main__":
    args = sys.argv[1:]
    over = {}
    for flag in ("--option", "--lic"):   # run variants of one spec: --option 4 --lic <wire loads> for a Method 2 run
        if flag in args:
            k = args.index(flag)
            over[flag[2:]] = int(args[k + 1]) if flag == "--option" else args[k + 1]
            del args[k:k + 2]
    spec_path, comp, out = args
    spec = json.load(open(spec_path, encoding="utf-8"))
    spec["analysis"].update(over)
    n = author(spec, comp, out)
    print(f"authored {out}: {n} lines")
