"""Write a PLS-POLE wire-loads file (.lic) from a declarative JSON spec.

A .lic carries what PLS-CADD/Lite hands PLS-POLE for an allowable-spans (Method 1) or interaction-diagram (Method 2)
analysis: the line-angle range, weight/wind span ratio, minimum weight span, wind-span search limit, the three
conditions (wind, cold, ice) and, per load case, the strength factors, structure wind pressure and the unit
weight / unit wind / concentrated loads and back/ahead tensions at every wire attachment. PLS-CADD 16.81 Lite exits
on this host whenever a Lite document opens (2026-09-26), so the lab writes the .lic itself and runs PLS-POLE directly;
the analysis PLS-POLE performs is the same one Lite would start.

Grammar: LIC FILE version 16, as PLS-CADD 20.01 writes it (IBC's 70_lic\\j-w-toff.lic).
usage: pls-lic-write.py <spec.json> <out.lic>        (schema ds.lab.pls_lic.v1)
"""
import json
import sys

CRLF = "\r\n"


def q(s):
    return "'" + s.replace("'", "") + "'"


def write(spec, out):
    cases = spec["load_cases"]
    n_points = len(cases[0]["points"])
    if any(len(c["points"]) != n_points for c in cases):
        raise SystemExit("every load case must list the same load points")
    a0, a1, da = spec["line_angles_deg"]
    cond = spec["conditions"]
    L = [f"TYPE='LIC FILE' VERSION='16' UNITS='SI' SOURCE='PLS-CADD Version 16.81' USER='DS line optimization lab' FILENAME='{out}'",
         f"{spec['structure_height_m']:.6f} 0 ; total structure height,fplmode",
         f"{len(cases)} {q(spec['description'])} ; m_nApplicableLoadCases",
         f"{n_points} ; num_load_points",
         spec.get("options_line", "1 1 1 1 0.142857 10 0 0 1 0 1 1 0 0 1 1 0 1.6 0 0 1 0 10000 3"),
         f"{a0:g} {a1:g} {da:g} ; line angle min,max,inc",
         f"{spec['span_ratio']:.6f} {spec.get('check_neg_wind', 0)} {spec['min_weight_span_m']:.6f} "
         f"{q(cond['wind'])} {q(cond['cold'])} {q(cond['ice'])} {spec['wind_span_search_limit_m']:g} "
         "; span ratio,check_neg_wind,min wgt span, wind,cold,ice"]
    for c in cases:
        tp, lp = c["wind_pressure_pa"]
        L += [c["name"],
              f"{c['neg_wind']} {c['add_insulator']} ; neg wind copy, do not add insulator weights",
              " ".join(f"{x:g}" for x in c["strength_factors"]) + " ; strength factors",
              f"{c['structure_weight_factor']:.5f} ; Structure weight load factor",
              f"{tp:.5f} {lp:.5f} -1 ; Factored structure transverse and longitudinal wind pressures",
              c.get("tail", "1 0 0 0 0 1")]
        for label, uw, cw, ud, cd, tl, tr, note in c["points"]:
            L += [label, f"{uw:.10f} {cw:.10f} {ud:.10f} {cd:.10f} {tl:.10f} {tr:.10f} {q(note)} ; UnitWt,ConcWt,UnitWd,ConcWd,TenL,TenR"]
    with open(out, "w", newline="", encoding="latin-1") as f:
        f.write(CRLF.join(L) + CRLF)
    return len(L)


if __name__ == "__main__":
    spec_path, out = sys.argv[1:3]
    print(f"wrote {out}: {write(json.load(open(spec_path, encoding='utf-8')), out)} lines")
