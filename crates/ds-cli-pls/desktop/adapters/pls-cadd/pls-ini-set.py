"""Set one key in a PLS INI file (PLS_CADD.INI) byte-for-byte: only that line changes, CRLF line endings kept.

Close every PLS program first: PLS-CADD rewrites the INI when it exits. Keys learned on host Magese (2026-09-26):
  UNITS=0|1                              [GLOBAL] unit system of all PLS programs: 0 US customary, 1 SI. It sets the
                                         units dialogs show, the units PLS writes files in (a cable saved under 0 is
                                         UNITS='US'), and %L in PLS-POLE file-name templates (feet under 0: 12 m -> .039).
  PREF_WANT_PLSCADD_PROJECT_WIZARD=0|1   [PLS_CADD] 0 = Classic interface (the drivers need it).
Do not edit PLS INIs with Git Bash `sed -i`: it rewrites the whole file with LF endings.

usage: pls-ini-set.py <ini> <KEY> <value> [--out <path>]
"""
import sys


def set_key(src, key, value, dst=None):
    b = open(src, "rb").read()
    tag = f"\r\n{key}=".encode()
    i = b.find(tag)
    if i < 0:
        raise SystemExit(f"{key} not found as a line start in {src}")
    i += 2
    j = b.index(b"\r\n", i)
    old = b[i:j].decode("latin-1")
    nb = b[:i] + f"{key}={value}".encode("latin-1") + b[j:]
    open(dst or src, "wb").write(nb)
    return old


if __name__ == "__main__":
    args = sys.argv[1:]
    out = None
    if "--out" in args:
        k = args.index("--out")
        out = args[k + 1]
        del args[k:k + 2]
    ini, key, value = args
    print(f"{set_key(ini, key, value, out)} -> {key}={value}")
