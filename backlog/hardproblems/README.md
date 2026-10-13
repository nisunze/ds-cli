# Parked hard cases

[OPEN.md](OPEN.md) holds the tried hard problems; [FEEDBACK.md](FEEDBACK.md) holds their unclosed feedback. Each feedback prefix has one maintained `evidence.json` with its full identity, known version, tried proof, exact blocker, bounded next attempt and SHA-256 pointers. `report.json`, where present, is the admitted acceptance snapshot.

Reuse retained models, binaries and receipts at their recorded paths after verifying their hashes. Update evidence in place when the stop condition changes. Do not repeat the same discovery or run without a new hypothesis. Remove the index row, feedback row and evidence only after acceptance is proven and feedback closed through ds.
