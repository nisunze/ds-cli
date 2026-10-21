"""Headless edit of the plan & profile SHEET furniture in a PLS-CADD project
DON (16.81, DON v57): raster attachments (BMP ATTACHMENTS v6) and sheet
annotation (ANNOTATION v8, global block). PLS-CADD only reads the DON at
project open, so the sequence is: exit PLS-CADD -> patch -> open -> print.

Learned grammar (Nyamagabe lab, 2026-09-21):

BMP record (one attachment):
  "path" show attach x y z w h rot pixw pixh bits mode a b c flags d e f g
  0
  'Unknown or Unvailable' '' '' '' 0 0 0.000000 0 ''
  <blank>
  <blank>
  0 0
  <timestamp>
  1 0 -1 -1 0 0
  ""
  attach: 2 = Sheet view (P&P). x,y,w,h in cm on the page, origin lower-left,
  (x,y) = the image's UPPER-LEFT corner. The dialog shows Y from the top:
  dialog_y = page_height - y. flags 268435713 opaque, +256 = optimise for
  line work. mode 2 = transparent, 0 = opaque (from the source records).

ANNOTATION v8 record (global block, sheet text):
  7 x y 0.00000 0 -1 height_cm rotation 0 0 0 0 0 0 4 'text'
  x,y in PERCENT of the page, origin UPPER-left. Special codes resolve at
  draw time: %p sheet, %q total sheets, %n project, %L line, %d date, %t time,
  %s1/%s2 station range, %s3/%s4 structure range, %m directory, %C1.. criteria
  notes rows, %R1.. project notes rows.

Usage:
  pls-don-sheet-patch.py <project.don> <patch.json> <out.don>
patch.json = {"page_height_cm": 29.7,
              "bitmaps": [{"path":..., "x":..., "y_from_top":..., "w":..., "h":...,
                           "pixw":..., "pixh":..., "line_work":true, "transparent":false}],
              "annotations": [{"x_pct":..., "y_pct":..., "height_cm":..., "rotation":0, "text":...}]}
Bitmaps REPLACE the block's records; annotations REPLACE the global block.
Optional "dxf_show": {"<file leaf>": 0|1} flips the show flag (first int
after the path) of existing DXF ATTACHMENTS v4 records — attach/detach of a
DXF stays a UI act (Attachment Manager) because its record carries per-layer
rows and entity counts this patcher does not author.
The DON is CRLF; every other byte is preserved.
"""
import json, sys, re, datetime

src, patch_path, out = sys.argv[1:4]
patch = json.load(open(patch_path, encoding="utf-8"))
data = open(src, "rb").read().decode("latin-1")
assert "\r\n" in data, "expected a CRLF DON"
lines = data.split("\r\n")

def find(prefix, start=0):
    for i in range(start, len(lines)):
        if lines[i].startswith(prefix):
            return i
    raise SystemExit(f"section {prefix!r} not found")

ph = float(patch.get("page_height_cm", 29.7))
stamp = datetime.datetime.now().strftime("%I:%M:%S %p %m/%d/%Y").lstrip("0")

# ---- DXF ATTACHMENTS v4: show/hide by leaf -------------------------------
for leaf, show in patch.get("dxf_show", {}).items():
    d0 = find("TYPE='DXF ATTACHMENTS'")
    d_end = find("TYPE='BMP ATTACHMENTS'", d0)
    hit = [i for i in range(d0, d_end) if lines[i].startswith('"') and lines[i].split('"')[1].lower().endswith(leaf.lower())]
    if len(hit) != 1:
        raise SystemExit(f"dxf_show: {leaf!r} matches {len(hit)} DXF records")
    path_part, rest = lines[hit[0]].rsplit('"', 1)
    fields = rest.split()
    fields[0] = str(int(show))
    lines[hit[0]] = path_part + '" ' + " ".join(fields)

# ---- BMP ATTACHMENTS v6 -------------------------------------------------
b0 = find("TYPE='BMP ATTACHMENTS'")
b_end = find("TYPE='ANNOTATION'", b0)
head = lines[b0]
count_line = lines[b0 + 1]
m = re.match(r"(\d+) ([0-9.]+) ; number bmp files attached, minimum pixel size", count_line)
assert m, count_line
min_pixel = m.group(2)
recs = []
for b in patch["bitmaps"]:
    flags = 268435713 + (256 if b.get("line_work") else 0)
    mode = 2 if b.get("transparent") else 0
    y = ph - float(b["y_from_top"])
    recs += [
        f"\"{b['path']}\" 1 2 {float(b['x']):.6f} {y:.6f} 0.000000 {float(b['w']):.6f} {float(b['h']):.6f} "
        f"0.000000000000000 {int(b['pixw'])} {int(b['pixh'])} 24 {mode} 0 4 -1 {flags} 0.000000000 0 255 7",
        "0",
        "'Unknown or Unvailable' '' '' '' 0 0 0.000000 0 ''",
        "", "",
        "0 0",
        stamp,
        "1 0 -1 -1 0 0",
        '""',
    ]
new_bmp = [head, f"{len(patch['bitmaps'])} {min_pixel} ; number bmp files attached, minimum pixel size"] + recs
lines[b0:b_end] = new_bmp

# ---- ANNOTATION v8 (first = global block) --------------------------------
a0 = find("TYPE='ANNOTATION'")
a_next = find("TYPE='ANNOTATION'", a0 + 1)
head = lines[a0]
# the block ends with 4 trailer lines: "-1 -1 ... 0", "''", "''", "'' ''"
trailer = lines[a_next - 4:a_next]
assert trailer[0].startswith("-1 -1") and trailer[-1] == "'' ''", trailer
recs = []
for a in patch["annotations"]:
    text = a["text"].replace("'", "''")
    recs.append(f"7 {float(a['x_pct']):.5f} {float(a['y_pct']):.5f} 0.00000 0 -1 {float(a['height_cm']):g} "
                f"{int(a.get('rotation', 0))} 0 0 0 0 0 0 4 '{text}'")
lines[a0:a_next] = [head, f"{len(recs)} ; Number annotation records"] + recs + trailer

open(out, "wb").write("\r\n".join(lines).encode("latin-1"))
print(f"wrote {out}: {len(patch['bitmaps'])} bitmaps, {len(patch['annotations'])} annotations, {len(lines)} lines")
