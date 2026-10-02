"""Write LIBRARY.json for a canonical PLS library folder (cables/, components/, cri/, specs/, structures/, reports/).

Every file gets its sha256 and, where the file carries engineering, its properties read from the file itself:
cables (PLS cable files), structures (Method 2 interaction-diagram files: family, class, length, burial, weight,
load cases, capacity envelope per line angle), the family model (.POL), component libraries (from components.json),
criteria, and the loads that produced the diagrams (specs/*.lic.json). Source documents come from the specs'
provenance. Regenerate after any change: never edit LIBRARY.json by hand.

usage: pls-library-manifest.py <library-dir> [--name tbea_libraries] [--as-of 2026-09-26]
"""
import hashlib
import importlib.util
import json
import math
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("wireloads", os.path.join(HERE, "pls-wire-loads.py"))
wl = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wl)


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def lines(p):
    return open(p, "rb").read().decode("latin-1").split("\r\n")


def structure_file(p, lic_cases):
    L = lines(p)
    name = os.path.basename(p)
    m = re.match(r"(?P<family>.+)-(?P<cls>S\d+)\.(?P<len>\d{3})$", name)
    out = {"family": m.group("family") if m else None, "class": m.group("cls") if m else None,
           "length_m": int(m.group("len")) if m else None, "type": "Method 2 interaction diagrams (PLS-POLE 16.81)"}
    # embedded STRUCT section: pole row 'label' 'property' length embed weight ...
    for l in L:
        mm = re.match(r"'(\w+)' '(S\d+)' ([\d.]+) ([\d.]+) ([\d.]+) ", l)
        if mm:
            out["pole"] = {"property": mm.group(2), "length_m": float(mm.group(3)), "embedment_m": float(mm.group(4)),
                           "weight_N": round(float(mm.group(5)), 1)}
            break
    i = next((k for k, l in enumerate(L) if l.startswith("Results generated")), None)
    if i is None:
        # A member seeded from a project library as it was saved there (a STRUCT file saved by PLS-CADD, or a
        # PLS-POLE model): no interaction-diagram or allowable-span run is embedded to read. Say so, and name
        # what the file declares about itself.
        out["type"] = "structure file without an embedded Method 1/2 run"
        L = [l for part in L for l in part.split("\n")]
        header = next((l for l in L if l.startswith("TYPE=")), "")
        out["header"] = header[:200]
        h = next((k for k, l in enumerate(L) if l.startswith("TYPE='STRUCT FILE'")), None)
        if h is None:
            h = next((k for k, l in enumerate(L) if l.startswith("TYPE='PLS_POLE INPUT FILE'")), None)
            if h is not None and h + 1 < len(L) and L[h + 1].strip().endswith("use edf suffixes"):
                h += 1
        if h is not None and h + 1 < len(L):
            out["description"] = L[h + 1].strip()
        return out
    m = re.search(r'"([^"]*)"?', L[i])
    out["loads_file"] = m.group(1) if m else L[i]
    out["generated"] = L[i + 1]
    # the STRUCT section's own units decide the angle unit: INTERNAL = radians, SI = degrees
    s = max((j for j in range(i) if "TYPE='STRUCT FILE'" in L[j]), default=0)
    deg = "UNITS='SI'" in L[s]
    k = i + 2
    while not L[k].strip() or L[k].startswith("Windspans"):
        k += 1
    na = int(L[k].split()[0]); k += 1
    if len(L[k].split()) != 3:   # Method 1: one row per line angle: angle, wind, weight-type spans...
        out["type"] = "Method 1 allowable spans"
        out["allowable_span_rows"] = [[float(x) for x in L[k + j].split()] for j in range(na)]
        out["negative_weight_span_points"] = sum(1 for r in out["allowable_span_rows"] if any(v < 0 for v in r[1:]))
        return out
    env = []
    for _ in range(na):
        _, ang, ncase = L[k].split()[:3]; k += 1   # row: <0> <line angle> <number of load cases>
        cases = []
        for c in range(int(ncase)):
            idx, n = L[k].split(); k += 1
            pts = []
            for _ in range(int(n)):
                ws, wt = map(float, L[k].split()); k += 1
                pts.append([round(ws, 2), round(wt, 2)])
            cases.append({"load_case": lic_cases[int(idx)] if int(idx) < len(lic_cases) else int(idx),
                          "points_wind_span_weight_span_m": pts})
        a = float(ang) if deg else math.degrees(float(ang))
        env.append({"line_angle_deg": round(a, 3), "cases": cases})
    out["interaction_diagrams"] = env
    neg = sum(1 for a in env for c in a["cases"] for w in c["points_wind_span_weight_span_m"] if w[1] < 0)
    out["negative_weight_span_points"] = neg
    return out


def main(lib, name, as_of):
    man = {"schema": "ds.canonical_pls_library.v1", "library": name, "as_of": as_of,
           "root": os.path.abspath(lib), "files": {}}
    specs = os.path.join(lib, "specs")
    lic_cases = {}
    for f in sorted(os.listdir(specs)) if os.path.isdir(specs) else []:
        if f.endswith(".lic.json"):
            rec = json.load(open(os.path.join(specs, f), encoding="utf-8"))
            lic_cases[f[:-9]] = [c["name"] for c in rec["lic_spec"]["load_cases"]]
    # Members copied in under their canonical names, one record file per change (scavenged-<date>.json,
    # seeded-<date>.json): each record names the source, its sha256 and the legacy names.
    scavenged = {}
    for f in sorted(os.listdir(specs)) if os.path.isdir(specs) else []:
        if re.match(r"(scavenged|seeded)-\d{4}-\d{2}-\d{2}\.json$", f):
            scavenged.update({r["name"]: r for r in json.load(open(os.path.join(specs, f), encoding="utf-8"))})
    for root, _, fs in os.walk(lib):
        for f in sorted(fs):
            p = os.path.join(root, f)
            rel = os.path.relpath(p, lib).replace("\\", "/")
            if rel == "LIBRARY.json":
                continue
            e = {"sha256": sha(p), "bytes": os.path.getsize(p)}
            if rel.startswith("cables/"):
                e["kind"] = "cable"
                e["properties"] = wl.read_cable(p)
            elif rel.startswith("structures/") and re.search(r"\.0\d\d$", f):
                # family = the name without its class token (S140..S800, NPD...) and without the length
                fam = re.sub(r"-(S\d+|NPD\w+)(?=[-.])", "", f)[:-4]
                e["kind"] = "structure (Method 1/2 capacity)"
                e["properties"] = structure_file(p, lic_cases.get(fam, []))
                if f in scavenged:
                    e["scavenged_from"] = scavenged[f]
            elif rel.startswith("structures/") and f.lower().endswith(".pol"):
                e["kind"] = "family model (Method 4, PLS-POLE 16.81 native)"
                e["spec"] = f"specs/{f[:-4]}.json"
            elif rel.startswith("components/"):
                e["kind"] = "PLS-POLE component library"
                e["spec"] = "specs/components.json"
            elif rel.startswith("cri/"):
                e["kind"] = "PLS-CADD criteria"
                e["header"] = lines(p)[0][:160]
            elif rel.startswith("specs/"):
                e["kind"] = "spec / loads record"
            elif rel.startswith("reports/"):
                e["kind"] = "native PLS report"
            man["files"][rel] = e
    # source documents: every provenance entry of every spec
    prov = {}
    for f in sorted(os.listdir(specs)) if os.path.isdir(specs) else []:
        if f.endswith(".json") and not f.endswith(".lic.json"):
            d = json.load(open(os.path.join(specs, f), encoding="utf-8-sig"))
            for k in ("provenance", "source", "load_cases_source", "ruling_span_basis"):
                if k in d:
                    prov[f"{f}:{k}"] = d[k]
    man["provenance"] = prov
    json.dump(man, open(os.path.join(lib, "LIBRARY.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(f"LIBRARY.json: {len(man['files'])} files")


if __name__ == "__main__":
    a = sys.argv[1:]
    opt = {"--name": None, "--as-of": ""}
    for k in list(opt):
        if k in a:
            i = a.index(k); opt[k] = a[i + 1]; del a[i:i + 2]
    lib = a[0]
    main(lib, opt["--name"] or os.path.basename(os.path.abspath(lib)), opt["--as-of"])
