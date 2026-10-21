# Solar input-base parity

`solar-input-base-vectors.json` belongs to the command kernel. Brain carries an
exact copy in `internal/services/testdata/` and pins its SHA-256. Refresh that copy
from this owner; never edit it independently. Rust's integration test and Brain's
service test execute the actual production fingerprint functions against these
frozen bytes. The expectations were captured from Brain's governed encoder.

City vectors keep JSON source as a string to preserve Firestore's int64 versus
binary64 values. Integer tokens are exact int64; decimal/exponent tokens and
negative zero are binary64. The encoding uses sorted UTF-8 keys, HTML-safe JSON
strings and shortest float spelling, retaining negative zero. Only the four
governed sections contribute; config contributes its path manifest. This is not
RFC 8785 and not the historical browser snapshot encoding.

The corpus covers reordered objects that remain unchanged, changed city sources,
absent versus null, arrays, numeric and non-BMP keys, controls and HTML escaping,
exact integers above binary64 precision, and float range/notation boundaries.
Reports are fenced to the SHA-256 of exact verified calculation report-input
bytes, including formatting; portfolio membership retains its declared order.

Snapshot body SHA-256 always describes the original retained body. Admission also
reads historical browser snapshots and exact authenticated Brain bodies without
rewriting them. A city-commit client snapshot hash remains a transport digest of
the transmitted bytes; it is not interchangeable with the governed input base.
