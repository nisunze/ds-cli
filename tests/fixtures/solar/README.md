# Solar consumer reference fixtures

The sealed Bere and Gagal reference files are exact-byte copies of
`ds-solar/fixtures/reference/aderm_{bere,gagal}/` at Solar revision
`c3e625a93458a518bd18e5cdf104c847e49de7bb`.
The native reference exchange tests own these copies so they can compile and run
without reading the producer repository. Gagal remains the wrong-site refusal
control for Bere; the original manifest and binary bytes are retained.

The opened calculation tuples and their numeric bit-level provenance are recorded
separately in [opened/README.md](opened/README.md).
