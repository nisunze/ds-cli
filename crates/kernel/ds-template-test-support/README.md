# Template test support

Dev dependencies only. Fixture engineering values are authored test data, not
production defaults. The test mechanics and vocabulary snapshot come from the reviewed, read-only ds-network slice/template-invariance. Kernel owns this test-only copy so its portable production graph remains independent of ds-network. See [the printing contract](../ds-command-kernel/docs/printing.md) and [authored catalogue data](../ds-command-kernel/data/README.md).

- `fixture_config()` / `fixture_job()`: complete template and local LV request.
- `fixture_unit_wire_catalogue()`: migrated unit identities with numeric areas
  deliberately different from their label digits.
- `Alien::from_template()` / `apply()` / `restore()`: deterministic simultaneous
  label/id/key translation; inverse roundtrip is a required precondition.
- `source_violations(repo)`: Rust syntax and embedded asset/script scan
  of master vocabulary in production literals and rendered text, including
  macros and compound labels, excluding test/bench/example/docs/fixture code
  and scratch `out`/`_discardable` at the scan or crate root. Member names
  of `json!` objects are wire names and are not counted; their values are.

The seed vocabulary snapshot must be regenerated when the master schema changes;
its source digest is recorded. The seed snapshot is never passed to compute as
fixture defaults. Complete live-master export is a separate DS interface gap.

`assert_source_ratchet(repo, baseline_path)` requires the checked-in occurrence
inventory and count to match the scan. `repo` may be one crate or a directory
of crates (Kernel scans `crates/kernel`); the baseline path and recorded paths
are relative to it. Occurrences compare as multisets of token and literal
hash: path and line are information, so moving a literal between files or
crates is never new, while an added copy reports every `path:line` of that
literal. Removals require reducing the baseline in the same change. Git history
forbids raising an established baseline for any literal. A shallow checkout is
rejected; CI must fetch full history (`fetch-depth: 0`). Every production
occurrence counts; the baseline is debt accounting, never a production file
allowlist.

Capture a reviewed snapshot explicitly with:

```sh
cargo run -p ds-template-test-support --example literal_baseline -- REPO > BASELINE
```

The first train-5 snapshot includes all inherited literals. Its count can only
fall afterward. Fixture/test/doc exclusions are scope, not baseline exemptions.
