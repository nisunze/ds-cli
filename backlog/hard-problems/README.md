# Reusable hard-problem evidence

Each feedback prefix has one maintained `evidence.json` with its full identity, known version, tried proof, exact blocker, bounded next attempt and SHA-256 pointers to retained files. `report.json`, where present, is the admitted filed acceptance snapshot. Older versions are labeled; no background freshness checks.

Start with [HARD-PROBLEMS.md](../HARD-PROBLEMS.md). Reuse existing models, binaries and runtime receipts at the recorded paths; verify their hashes before consuming them. Copy no large artifacts into this folder. Refresh cards in place when new evidence changes the stop condition. Remove a card and its index row only after filed acceptance is proven and feedback is closed through ds.
