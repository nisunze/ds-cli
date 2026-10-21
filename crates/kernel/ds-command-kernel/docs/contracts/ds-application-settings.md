# Application settings — one owner for the operator's effective record

**Status:** kernel implemented on `fable/application-settings-kernel`,
2026-09-12, under the owner's ruling of the same day: the TypeScript that
decides is removed and the decision moves into the kernel. Parent:
[The application kernel](ds-application-kernel.md) §2 (the litmus test).
Revised 2026-09-22 (`v2`): the processing-lane preference and its ops are
gone — there is one way to process a transformer.

## 1. The question this module owns

The browser persists one JSON blob (`application_settings:v1`) in IndexedDB
and holds the effective record in a Svelte store. Everything between the two
was decided in TypeScript at import time (`stores/applicationSettings.ts` ran
`loadApplicationSettings()` as a module side effect, so any test graph that
reached it read a partially mocked database). It is decided by
`application_settings` now (`ds.application-settings/v2`, WASM `applicationSettings`):

| Question | Op | Hosts do |
|---|---|---|
| The record at boot: from the blob and the device | `normalize` | read the key; hold `settings`; write it back only when `persist` |
| The record after a change | `patch` | hold; write |
| The named keys back to their shipped (device-aware) defaults | `reset` | hold; write |
| The MapLibre cache profile for a budget and a browser estimate | `performance_profile` | configure the map |
| The working-area interval as a label | `interval_label` | render |

**What TypeScript keeps:** the IndexedDB I/O (`persistentGet`/`persistentSet`
of the one key) and the Svelte store that holds the answer. Nothing that
decides a key, a default, a bound, a first launch or a reset.

## 2. Shapes

Requests are `{"schema": "ds.application-settings/v2", "op": {"kind", ...}}`.
Op fields are snake_case; the record's keys are the store's camelCase.

```json
{"kind": "normalize", "saved": {...} | null,
 "device": {"phone": false, "my_location_default": false}}
→ {"settings": {...}, "persist": false,
   "first_launch": {"popup_mode": true, "my_location": true}}

{"kind": "patch", "current": {...}, "patch": {...}}                → {"settings"}
{"kind": "reset", "current": {...}, "keys": ["mapMemoryBudget"], "device"}
                                                                   → {"settings"}
{"kind": "performance_profile", "budget": 3,
 "resource_hint"?: {"js_heap_limit_bytes"?, "device_memory_gb"?, "hardware_concurrency"?}}
                                                                   → {"profile": {...}}
{"kind": "interval_label", "value": 90}                            → {"label": "1 hour 30 min"}
```

`settings` is the record the store produced: 43 keys in the store's order
(`SETTING_KEYS`), same units. `profile` is `{budget, label, description,
maxTileCacheSize, cleanupDelayMs, refreshExpiredTiles, resourceLabel,
resourceScale}`.

**Retired in v2 (2026-09-22).** The record no longer carries `processingLane`,
`standardLaneConfirmedAt` or `fastLaneConfirmedAt`, and the ops `lane` and
`set_lane` and the request members `legacy_lane` and `lane_facts` are gone:
there is one way to process a transformer, so there is no lane to prefer,
confirm or resolve. A saved blob that still carries one of the three keys is
read tolerantly, the keys are dropped on normalise, and `persist` is true so
the host writes the cleaned record back once. A request that still sends
`legacy_lane`, `lane_facts` or the v1 schema is the host's error.

## 3. Invariants

- **A saved blob is user data.** An unknown key is dropped, an unreadable
  value takes its default, a blob that is not an object spreads nothing;
  nothing in `saved` is ever refused. A request the host built wrong
  (unknown op or field, a patch that is not an object, a reset key that is
  not a setting, another schema) is an `Err` from `evaluate` — the host's
  bug, not a client answer.
- **Values are read the way the store read them.** `Number(value)` over the
  JSON value space (strings, `0x`/`0o`/`0b` and exponent forms, `''` and
  `null` as 0, arrays by their joined string), `Math.round` with halves toward
  +∞, `String(item).trim()`, `typeof === 'boolean'`, `??` on the guarded keys.
  So `mapMemoryBudget: null` is 1 where `mapMemoryBudget` absent is 3 —
  because that is what the store did.
- **Bounds and defaults are the store's**, transcribed: zooms 0–22 (15, 14);
  exaggeration 0.5–100 (10); alignment gap 0–10 000 (100); corridor 1–500
  (15); terrain gap 1–5 000 (100); clearance and wire drop 0–100 to a tenth
  (6, 0); opacity 0.05–1 to a hundredth (0.25); strokes 0.5–8 to a quarter
  (1.5, 1.25); point width 1–24 to a half (2.5); grid interval 0.1–100 000 or
  the adaptive `null`; GPS 1–300 s (1); budget 1–5 (3); concurrency 1–100
  whole (10); working area 15–1 440 min or never (`null`); hidden ids ≤ 500,
  trimmed, first occurrence kept. Colours are a trimmed lower-cased
  `#rrggbb`; the retired terrain blues `#38bdf8`/`#94a3b8` migrate to
  `#d1d5db`; `profileTerrainPointRadiusPx` is read when the width is absent.
- **The parity baseline.** A record the store already normalised reopens key
  for key and value for value (`normalize-a-full-record-reopens-unchanged`).
- **A retired key is written back once, without itself.** `persist` is true
  exactly when the saved blob carried `processingLane`,
  `standardLaneConfirmedAt` or `fastLaneConfirmedAt`; the answered record
  never does. Otherwise `persist` is false: nothing else is ever written
  from `normalize`.
- **A first launch follows the device and is not persisted.** `popupMode`
  and `showMyLocationControl` take the device's value when the blob has none
  (`== null`); `first_launch` says which. The device recomputes it next
  launch, so writing it would only race a user write. `reset` of those two
  keys lands on the device's value for the same reason.
- **A `null` in a `patch` is the store's `undefined`, on every key.** JSON
  cannot carry `undefined`, so the host sends `null`; the key is removed
  before the record is read and takes its default — never merged, because a
  merged `null` would read as `Number(null)`, 0, on the keys the store did
  not guard with `??` (a stroke width would clip to 0.5 where the store's
  `undefined` gave the shipped 1.5). Only a `null` in a *saved blob* is read
  as `null` (`patch-null-is-the-stores-undefined-on-every-key`).
- **Pure.** Device and browser estimate are inputs; the same request always
  has the same answer. Requests ≤ 4 MiB.

## 4. The proof

`tests/fixtures/application-settings-cases.json` (65 named cases: every
case of ds-web's `application-settings.test.ts`, every bound and reading,
and the retired-key write-back) is replayed by `tests/application_settings_cases.rs` and, byte for byte, by
ds-web from `src/tests/fixtures/application-settings/cases.json` over the
vendored WASM. Re-record with `DS_APPLICATION_SETTINGS_CASES_WRITE=1 cargo
test --test application_settings_cases` after a deliberate rule change.
