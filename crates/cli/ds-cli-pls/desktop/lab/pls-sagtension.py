"""Ruling-span sag-tension for a linear-elastic cable with creep modelled as a temperature increase (PLS-CADD's
'Linear elastic with permanent stretch due to creep specified as a user input temperature increase'), and
AutoSag: the largest stringing tension that keeps every limit (catenary H/w at a weather case and condition).

Level ruling span, exact catenary; elastic stretch integrated along the catenary:
    stressed length  L  = 2a sinh(S/2a),  a = H/w
    elastic stretch  dL = (H/EA) (S/2 + (a/2) sinh(S/a))
    unstressed length at temperature t: L0(t) = L0_ref (1 + alpha (t - t_ref)); after creep: + alpha * creep_shift
Validated against PLS-CADD 16.81's Ruling Span Sag Tension Report (see pls-native-lab skill).

Library use: `solve(cable, span, weather, limits)` -> tensions per weather case and condition.
CLI: pls-sagtension.py <spec.json>   (schema ds.lab.sagtension.v1: cable, ruling_span_m, weather, autosag)
"""
import json
import math
import sys


def stressed_length(H, w, S):
    a = H / w
    return 2 * a * math.sinh(S / (2 * a))


def elastic_stretch(H, w, S, EA):
    a = H / w
    return (H / EA) * (S / 2 + (a / 2) * math.sinh(S / a))


def unstressed(H, w, S, EA):
    return stressed_length(H, w, S) - elastic_stretch(H, w, S, EA)


def tension_for_unstressed(L0, w, S, EA):
    """Horizontal tension H such that the unstressed length of the span equals L0 (bisection on H)."""
    lo, hi = 1e-3 * w * S, 1e3 * w * S + 1e7
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        # unstressed length decreases as H increases
        if unstressed(mid, w, S, EA) > L0:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def resultant_load(cable, wx):
    """Unit load (N/m): cable weight plus transverse wind load wx (no ice in Rwanda)."""
    return math.hypot(cable["weight_N_m"], wx)


class Cable:
    def __init__(self, c):
        self.c = c
        self.w = c["weight_N_m"]
        self.EA = c["E_GPa"] * 1e9 * c["area_mm2"] * 1e-6
        self.alpha = c["alpha_per_C"]
        self.creep = c.get("creep_shift_C", 0.0)
        self.d = c["diameter_mm"] / 1000.0


def state(cable, S, L0_ref, t_ref, weather, condition):
    """Horizontal tension at a weather case {temp_C, wind_Pa} for condition 'I' (initial) or 'C' (after creep)."""
    wx = weather.get("wind_Pa", 0.0) * cable.d * weather.get("wind_factor", 1.0)
    w = math.hypot(cable.w, wx)
    shift = cable.creep if condition == "C" else 0.0
    L0 = L0_ref * (1 + cable.alpha * (weather["temp_C"] + shift - t_ref))
    H = tension_for_unstressed(L0, w, S, cable.EA)
    return {"H_N": H, "w_N_m": w, "catenary_m": H / w, "wind_N_m": wx}


def from_stringing(cable, S, H_ref, t_ref, w_ref=None):
    """Unstressed length at t_ref from a known initial horizontal tension (no wind)."""
    return unstressed(H_ref, w_ref or cable.w, S, cable.EA)


def autosag(cable, S, weather, limits, t_ref=25.0):
    """Largest EDT-initial stringing (expressed as L0 at t_ref) that satisfies every catenary limit
    {weather, condition, max_catenary_m}. Returns (L0_ref, governing limit)."""
    lo, hi = S * 1.00000001, S * 1.2   # unstressed length bounds: tight (high tension) .. slack
    def ok(L0):
        for lim in limits:
            st = state(cable, S, L0, t_ref, weather[lim["weather"]], lim["condition"])
            if st["catenary_m"] > lim["max_catenary_m"] + 1e-9:
                return False
        return True
    # tension falls as L0 grows; find the smallest L0 (highest tension) that is ok
    if not ok(hi):
        raise ValueError("no stringing satisfies the limits")
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if ok(mid):
            hi = mid
        else:
            lo = mid
    L0 = hi
    gov = max(limits, key=lambda lim: state(cable, S, L0, t_ref, weather[lim["weather"]], lim["condition"])["catenary_m"] / lim["max_catenary_m"])
    return L0, gov


def table(cable, S, L0, t_ref, weather):
    out = {}
    for name, wc in weather.items():
        out[name] = {cond: state(cable, S, L0, t_ref, wc, cond) for cond in ("I", "C")}
    return out


if __name__ == "__main__":
    spec = json.load(open(sys.argv[1], encoding="utf-8"))
    cable = Cable(spec["cable"])
    S = spec["ruling_span_m"]
    if "stringing" in spec:
        L0 = from_stringing(cable, S, spec["stringing"]["H_N"], spec["stringing"]["temp_C"])
        gov = None
    else:
        L0, gov = autosag(cable, S, spec["weather"], spec["autosag"])
    res = table(cable, S, L0, 25.0, spec["weather"])
    print(json.dumps({"governing": gov, "tensions": {k: {c: round(v["H_N"], 1) for c, v in d.items()} for k, d in res.items()}}, indent=1))
