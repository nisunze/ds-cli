# LV differential CLI verification

The LV owner is `ds-network::network::differential`. The canonical intent body
is `{"selected_feeders":["feeder-id"]}`; AutoProcess instead sends an empty
selection and its captured edit intent in `auto_process`. Map processing,
native LV batch jobs and the web use that same body. Preview accepts the layers,
intent and customer-source addresses, deriving its own scope and counts.

The original installed CLI refused the new `map.design.process --request`
flag. The implementation replaces its selector flags with that JSON request;
there is no old-shape reader. Headless `design.autoprocess.plan` derives scope
from GDFs through the LV owner and rejects host-authored mapping verdicts.

The native pins are ds-network `7d5d304580c403524eb27d23f2e3345dab1eab47`
and ds-command-kernel `c39ba60701dc2ed560cf4b1330a630bcdc42e8e9` (also the
client-core pin). Cargo refreshed only stale dependency records from the
geometry toolkit already on the integration base; no Grid/MV code changed.

The decision and output corpus is recorded in the sibling ds-network tests:
`crates/ds-network/tests/lv-differential-parity.md`. Its 43 decision cases,
54 complete native outputs and 54 raw browser outputs passed. The only reporting
correction is the nine documented explicit-manual diagnostic receipts; their
engineering output bytes remain unchanged.

## Targeted gates

All commands ran through `scripts/with-network-deps.sh` with two Cargo workers,
shared sccache, and two native Rayon/test threads.

- `cargo check -p ds-cli-design -p ds-cli-map -j 2`: passed, including the
  touched native design-workspace boundary.
- `cargo test -p ds-cli-design --lib autoprocess -j 2`: 3 passed.
- `cargo test -p ds --test domain_smoke autoprocess_plan_answers -j 2`: 1 passed.
- `cargo test -p ds --test domain_smoke map_validates_its_own_inputs -j 2`: 1 passed.
- `cargo test -p ds --test bridge_parity -j 2`: 35 passed.
- `cargo build -p ds -j 2`: passed locally.

The review executable was copied to `/tmp/lv-differential-blind/bin/ds`.
Its SHA-256 is `f4daaf686c35f9c7521f6071ce0c526e1d475d49299a837d556d15147d6c98df`;
it honestly reports the pre-commit CLI revision and dirty source state.
`capabilities map.design.process` reports contract 2 and the `request` input.
A well-formed selection request with an explicitly nonexistent desktop descriptor
reaches `desktop_unreachable`, rather than the baseline `unknown_flag` refusal.
No paired Desktop or live project was mutated by verification.

## One bounded blind CLI trial

One Luna agent received only the review executable, command discovery, shipped
references/skills, and an operator-exported Bisesero3 snapshot. It had no
repository access and ran no live project operation. There was no second trial.
It selected the first feeder listed on the transformer, `LV_Line_11`, and used
its GeoJSON feature id `line_11` in the canonical selection request.

```sh
RAYON_NUM_THREADS=2 /tmp/lv-differential-blind/bin/ds design lv process \
  --input /tmp/lv-differential-blind/bisesero3-first-feeder-request.json \
  --out /tmp/lv-differential-blind/bisesero3-first-feeder-result-v2.json \
  --output json
```

The native receipt reports one job, one success, zero failures, and two Rayon
threads. The complete result artifact is 196542 bytes, SHA-256
`46ed27a78e3ea4a65e06b0422b0d8f0bb2d7cfe28727273e387eb8a1b3497097`.
The request SHA-256 is
`7e9d5b4243541f45668852a8a88cbee9f4a98cb3ac270ae64145ee57c6d1413c`;
the supplied export SHA-256 is
`a73845429124ad282afc3c960e2b982107f9f853ba8b539e0daa388ef6971666`.
Feature counts are customers 17, drafting_errors 2, lv_lines 33, lv_poles 31,
service_cables 17, spans 36, and tr 1. The original exported GDFs, settings and
configuration were forwarded unchanged.

The parent also executed `ds design autoprocess plan --changes
/tmp/lv-differential-blind-scope.json --output json` on the same layers and
selection. Contract 4 returned scope `feeders`, feeder `line_11`, reason
`differential_narrowed`, selected count 1, frozen count 131, blocking diagnostics
false, and manual availability true.

The blind inspection verified unchanged geometry and cable sizes for all
32 unselected LV lines and all 30 unselected poles. It found changed geometry
for 33 of 35 unselected spans, so its complete geometry-preservation check
failed. This is a confirmed existing pipeline limitation, not a migration
parity exception: the recorded `lv-differential-bisesero3.json selection-0 clean`
manual case freezes 35 spans, of which 34 change geometry by stable id and one
remains unchanged. Its new browser output matches the original TS/train-5
output exactly: 196451 bytes, SHA-256
`c2afb4e6d1f035eccab449a5a3f5ac263db34f381c876bc4308afbdda89f3269`.
Those original receipts and freeze indices are already in the ds-network
corpus. Fixing derived span regeneration would change engineering outputs and
is outside this parity migration. No claim of complete frozen-geometry
preservation is made.

The trial recovered from three ordinary refusals without bypassing them: a raw
export needed the advertised `ds.fast-lv.input/v1` jobs envelope; a human feeder
label needed its source feature id; and an existing output path needed a new
path. `doctor` reported a stale installed skill bundle and an unavailable native
client profile catalog; the offline native processor remained usable.
