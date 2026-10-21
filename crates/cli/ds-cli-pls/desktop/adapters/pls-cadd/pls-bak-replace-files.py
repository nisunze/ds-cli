"""Replace embedded files in a PLS-CADD text backup (.bak), e.g. cable files re-saved by PLS-CADD itself.

A DS export .bak is a zip with one member: a PLS text backup. Each embedded file is framed as
    TYPE='***PLSBACKUPFILE***' ... FILENAME='<path>'\n
    <path>\n
    <spaces><byte count> text 1\n
    <date line>\n
    <content: exactly <byte count> bytes>
The tool parses that framing, proves a byte-identical round trip, swaps the named members' content (byte counts
rewritten), re-zips under the same member name and prints both sha256 values.

usage: pls-bak-replace-files.py <in.bak> <out.bak> <embedded path suffix>=<replacement file> ...
  e.g. 'cables\\opgw.wir=G:\\...\\opgw-ui.wir' 'cables\\acsr 70-12mm2=G:\\...\\acsr-ui.wir'
Owner rule 2026-09-26: cable creep (15 C temperature shift) is set in PLS-CADD's own Cable Data editor; the
resulting files are injected here because the DS cable definition has no creep field (feedback filed).
"""
import hashlib
import io
import re
import sys
import zipfile

HEAD = re.compile(rb"TYPE='\*\*\*PLSBACKUPFILE\*\*\*'[^\n]*\n")


def parse(text):
    """Return [(prefix_bytes, path, count_line, date_line, content)], trailing bytes."""
    out, pos = [], 0
    while True:
        m = HEAD.search(text, pos)
        if not m:
            return out, text[pos:]
        lead = text[pos:m.start()]
        p = m.end()
        path_end = text.index(b"\n", p)
        path = text[p:path_end]
        cnt_end = text.index(b"\n", path_end + 1)
        cnt_line = text[path_end + 1:cnt_end]
        mc = re.match(rb"\s*(\d+) (text|binary|directory) (\d+)\s*$", cnt_line)
        if not mc:
            raise SystemExit(f"unexpected count line after {path!r}: {cnt_line!r}")
        date_end = text.index(b"\n", cnt_end + 1)
        date_line = text[cnt_end + 1:date_end]
        n = int(mc.group(1))
        content = text[date_end + 1:date_end + 1 + n]
        if len(content) != n:
            raise SystemExit(f"truncated member {path!r}")
        out.append((lead + text[m.start():m.end()], path, cnt_line, date_line, content))
        pos = date_end + 1 + n


def build(entries, trailer):
    b = io.BytesIO()
    for head, path, cnt_line, date_line, content in entries:
        b.write(head + path + b"\n" + cnt_line + b"\n" + date_line + b"\n" + content)
    b.write(trailer)
    return b.getvalue()


ALLOW_TYPE_CHANGE = False


def main():
    global ALLOW_TYPE_CHANGE
    args = sys.argv[1:]
    if "--allow-type-change" in args:
        args.remove("--allow-type-change")
        ALLOW_TYPE_CHANGE = True
    src, dst, *pairs = args
    if not pairs:
        raise SystemExit(__doc__)
    raw = open(src, "rb").read()
    zipped = raw[:2] == b"PK"
    if zipped:
        z = zipfile.ZipFile(io.BytesIO(raw))
        names = z.namelist()
        if len(names) != 1:
            raise SystemExit(f"expected one zip member, found {names}")
        member = names[0]
        text = z.read(member)
    else:
        member, text = None, raw
    entries, trailer = parse(text)
    if build(entries, trailer) != text:
        raise SystemExit("round trip is not byte-identical; refusing to edit")
    todo = {}
    for p in pairs:
        suffix, repl = p.split("=", 1)
        todo[suffix.replace("/", "\\").encode("latin-1").lower()] = open(repl, "rb").read()
    done = []
    for i, (head, path, cnt_line, date_line, content) in enumerate(entries):
        for suffix, new in todo.items():
            if path.strip().lower().endswith(suffix):
                mc = re.match(rb"(\s*)(\d+)( text \d+\s*)$", cnt_line)
                if not mc:
                    raise SystemExit(f"{path!r} is not a text member: {cnt_line!r}")
                width = len(mc.group(1)) + len(mc.group(2))
                # Refuse a file-kind change: a PLS-POLE input file (Method 4 link 'A <.POL> pls_pole.exe', embedded
                # PRT/PLT/Insulator Library) in place of a STRUCT FILE member is not what PLS-CADD exported. A
                # hand-built backup of that kind froze PLS-CADD 16.81's Restore on 2026-09-29 (pid 11340).
                old_type = re.match(rb"TYPE='([^']*)'", content)
                new_type = re.match(rb"TYPE='([^']*)'", new)
                if old_type and new_type and old_type.group(1) != new_type.group(1) and not ALLOW_TYPE_CHANGE:
                    raise SystemExit(
                        f"{path.decode('latin-1')}: member is {old_type.group(1).decode()}, replacement is "
                        f"{new_type.group(1).decode()}; refusing a file-kind change (--allow-type-change to force)")
                # The replacement was saved elsewhere: point its OWN FILENAME headers back at the project path.
                # Only values equal to the file's own saved path are rewritten. Empty values (embedded PRT/PLT/
                # Insulator Library blocks) and links to other files stay as they are; rewriting '' made each
                # block claim the structure file itself as its library.
                own = re.search(rb"FILENAME='([^'\r\n]*)'", new)
                if own and own.group(1):
                    source_path = own.group(1)
                    target = b"FILENAME='" + path.strip() + b"'"
                    new = re.sub(rb"FILENAME='" + re.escape(source_path) + rb"'", lambda _m: target, new)
                new_cnt = str(len(new)).rjust(width).encode() + mc.group(3)
                entries[i] = (head, path, new_cnt, date_line, new)
                done.append((path.decode("latin-1"), len(content), len(new)))
    missing = [s.decode("latin-1") for s in todo if not any(d[0].encode("latin-1").strip().lower().endswith(s) for d in done)]
    if missing:
        raise SystemExit(f"no embedded file ends with: {missing}")
    out_text = build(entries, trailer)
    check, _ = parse(out_text)
    if len(check) != len(entries):
        raise SystemExit("re-parse of the edited backup failed")
    if zipped:
        b = io.BytesIO()
        with zipfile.ZipFile(b, "w", zipfile.ZIP_DEFLATED) as z:
            z.writestr(member, out_text)
        out = b.getvalue()
    else:
        out = out_text
    open(dst, "wb").write(out)
    for path, a, n in done:
        print(f"replaced {path}: {a} -> {n} bytes")
    print("in ", hashlib.sha256(raw).hexdigest(), src)
    print("out", hashlib.sha256(out).hexdigest(), dst)


if __name__ == "__main__":
    main()
