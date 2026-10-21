"""Fictional logos (PNG/JPG) and a fictional A3 landscape border DXF for the
PLS-CADD plan & profile printing lab. Everything here is invented: the
companies do not exist. The DXF is drawn 1:1 in centimetres (A3 = 42.0 x 29.7)
with (0,0) at the lower-left page corner, which is what PLS-CADD expects for a
"P&P View" attachment; the title strip occupies the bottom 4.75 cm (16 % of the
page height) so the Page Layout 'Sheet' Ymax = 84 % leaves it free."""
import sys, pathlib
from PIL import Image, ImageDraw, ImageFont

out = pathlib.Path(sys.argv[1])
(out / "logos").mkdir(parents=True, exist_ok=True)
(out / "borders").mkdir(parents=True, exist_ok=True)

def font(size):
    for name in ("arialbd.ttf", "arial.ttf", "DejaVuSans-Bold.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()

def logo(name, text, sub, bg, fg, shape, size=(900, 450), fmt="PNG"):
    im = Image.new("RGB", size, "white")
    d = ImageDraw.Draw(im)
    w, h = size
    if shape == "circle":
        d.ellipse((20, 20, h - 20, h - 20), fill=bg)
        d.text((h + 20, h * 0.28), text, fill=bg, font=font(int(h * 0.26)))
        d.text((h + 20, h * 0.62), sub, fill="black", font=font(int(h * 0.13)))
    elif shape == "bar":
        d.rectangle((0, 0, w, int(h * 0.62)), fill=bg)
        d.text((40, h * 0.12), text, fill=fg, font=font(int(h * 0.34)))
        d.text((40, h * 0.68), sub, fill=bg, font=font(int(h * 0.16)))
    else:  # bolt
        d.polygon([(60, 30), (250, 30), (170, 200), (260, 200), (60, 420), (120, 250), (40, 250)], fill=bg)
        d.text((300, h * 0.22), text, fill=bg, font=font(int(h * 0.3)))
        d.text((300, h * 0.62), sub, fill="black", font=font(int(h * 0.13)))
    p = out / "logos" / f"{name}.{fmt.lower()}"
    im.save(p, fmt)
    return p

paths = [
    logo("acme-power", "ACME POWER", "Fictional Employer", (20, 60, 140), "white", "bolt"),
    logo("riverlight-utility", "RIVERLIGHT", "Fictional Funder", (0, 120, 70), "white", "circle"),
    logo("northstar-engineering", "NORTHSTAR", "Fictional Contractor", (220, 100, 0), "white", "bar", fmt="JPEG"),
    logo("ds-lab-square", "DS", "LAB", (90, 0, 120), "white", "circle", size=(450, 450)),
]

# ---------------- fictional A3 border, hand-written DXF R12 ----------------
W, H = 42.0, 29.7
M = 1.0            # margin
TB = 4.75          # title strip height (bottom)
lines = []         # (layer, x1, y1, x2, y2)
texts = []         # (layer, x, y, height, text, rotation)

def rect(layer, x1, y1, x2, y2):
    lines.extend([(layer, x1, y1, x2, y1), (layer, x2, y1, x2, y2), (layer, x2, y2, x1, y2), (layer, x1, y2, x1, y1)])

rect("BORDER", M, M, W - M, H - M)                      # outer frame
lines.append(("BORDER", M, M + TB, W - M, M + TB))     # title strip top edge
# title strip cells (x boundaries)
cells = [M, 7.0, 13.0, 19.0, 27.0, 33.0, 37.0, W - M]
for x in cells[1:-1]:
    lines.append(("TITLE", x, M, x, M + TB))
labels = ["EMPLOYER", "FUNDER", "CONTRACTOR", "PROJECT", "DRAWING", "SCALE / DATE", "SHEET"]
for (x1, x2), lab in zip(zip(cells, cells[1:]), labels):
    texts.append(("TEXT", x1 + 0.3, M + TB - 0.6, 0.28, lab, 0))
    lines.append(("TITLE", x1, M + TB - 0.85, x2, M + TB - 0.85))
# static fictional text in the PROJECT and DRAWING cells; the SCALE/DATE and
# SHEET cells stay EMPTY on purpose: PLS-CADD annotation (%p %q %d %s1 %s2)
# fills them so the DXF never carries a page number or a date.
texts.append(("TEXT", 19.3, M + 2.6, 0.32, "FICTIONAL 30 kV LINE - LAB", 0))
texts.append(("TEXT", 19.3, M + 1.9, 0.25, "Somewhere District, Nowhere Province", 0))
texts.append(("TEXT", 27.3, M + 2.6, 0.32, "PLAN AND PROFILE", 0))
texts.append(("TEXT", 27.3, M + 1.9, 0.25, "Contract DS-LAB-0001", 0))
texts.append(("TEXT", 1.3, H - M - 0.9, 0.35, "DS PRINTING LAB - FICTIONAL BORDER - NOT FOR CONSTRUCTION", 0))
# dimension notes deliberately on their own layer so they can be hidden
texts.append(("DIMS", W / 2 - 4, H - 0.6, 0.3, "A3 landscape 42.0 x 29.7 cm, margin 1.0, title strip 4.75", 0))

def dxf():
    o = []
    def g(code, val): o.append(f"{code}\n{val}")
    g(0, "SECTION"); g(2, "HEADER"); g(9, "$ACADVER"); g(1, "AC1009")
    g(9, "$EXTMIN"); g(10, 0.0); g(20, 0.0); g(9, "$EXTMAX"); g(10, W); g(20, H)
    g(0, "ENDSEC")
    g(0, "SECTION"); g(2, "TABLES"); g(0, "TABLE"); g(2, "LAYER"); g(70, 4)
    for name, color in (("BORDER", 7), ("TITLE", 7), ("TEXT", 7), ("DIMS", 8)):
        g(0, "LAYER"); g(2, name); g(70, 0); g(62, color); g(6, "CONTINUOUS")
    g(0, "ENDTAB"); g(0, "ENDSEC")
    g(0, "SECTION"); g(2, "ENTITIES")
    for layer, x1, y1, x2, y2 in lines:
        g(0, "LINE"); g(8, layer); g(10, f"{x1:.4f}"); g(20, f"{y1:.4f}"); g(30, 0.0); g(11, f"{x2:.4f}"); g(21, f"{y2:.4f}"); g(31, 0.0)
    for layer, x, y, h, t, rot in texts:
        g(0, "TEXT"); g(8, layer); g(10, f"{x:.4f}"); g(20, f"{y:.4f}"); g(30, 0.0); g(40, f"{h:.3f}"); g(1, t); g(50, rot)
    g(0, "ENDSEC"); g(0, "EOF")
    return "\n".join(o) + "\n"

p = out / "borders" / "A3 - DS LAB - fictional.dxf"
p.write_text(dxf().replace("\n", "\r\n"), encoding="ascii", newline="")
paths.append(p)
for p in paths:
    print(p, p.stat().st_size)
