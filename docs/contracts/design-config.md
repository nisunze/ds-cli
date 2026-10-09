# Settings model

`design_config::evaluate` / WASM `designConfig` is the pure Settings owner.
Requests are closed, internally tagged `op` values; unknown operations and
fields refuse. Input is bounded to 16 MiB. Successful replies carry
`schema: ds.design-config/v1` and `result`. Inspection and material propagation
retain their existing separate entry points.

The model owns group priority, sheet kind, model/control lookup, column order,
rule-set shapes, names, duplication, blank-row compaction, canonical parameter
names, aliases, scalar/list/checkbox coercion, inactive selections, semantic
diff and validated sheet writes. Hosts supply JSON documents and observations;
they own authentication, file/cache IO, clocks, snapshot lifetime, UI grants,
localization and locale-sensitive display sorting. Unknown group and column
order follows document order. Insertion-order serde_json was already enabled
by dependencies; the direct dependency now declares that requirement explicitly.

`process_settings` re-exports the one canonical parameter-name function and
uses Settings' restricted processor-value grammar. That grammar only admits
booleans and finite numbers; arbitrary text controls retain strings. Native
and web saves prepare the existing persistence contract's canonical names,
boolean/numeric string values and rule-set map shape before fresh readback.
The server remains authority for grants, option references and persistence.
Whole-sheet saves retain the server's existing concurrency semantics; this
model does not promise a cross-client editing lock.

`project_config::admit` / `projectConfigAdmit` accepts only host facts: cache
presence, captured/current generation, account/audience/origin scope match,
invalidation in progress, response status/presence, timeout and connectivity.
Scope mismatch, generation mismatch and invalidation take precedence over all
cache/fetch outcomes. Hosts must re-evaluate after every yielding boundary
before returning or writing bytes. A failed refresh may use a cache only when
these fences still match. Native reads have no browser cache to substitute.

## Parity and deliberate changes

The byte-identical native and web `settings-model-parity.json` has 234 cases
captured by executing the original TypeScript at ds-web `707deedf`, ten new
contract cases, and 32 cache-admission cases. `expected_ts` is the captured
answer; differing `expected` / `expected_error` carries its reason. Independent
integration tests exercise native transport writes/readback and browser races.

- Legacy `##` alias strings are accepted. Alias edits emit arrays, drop empty
  sentinels and deduplicate case-insensitively while preserving first spelling.
- Parameter matching uses the processor's canonical grammar everywhere,
  preserving the leading underscore for experimental parameters.
- Scalar boolean text is case-insensitive. A scalar `007` is numeric; malformed
  bracketed JSON remains text. List and checkbox controls keep their own grammar.
- Key reordering is not dirty. Row order, whitespace cell edits, missing values
  and blank-row additions remain observable.
- Both editors share parameters → engineering → header_mapping → data_cleaning
  priority; the global editor previously swapped the last two groups.
- Duplicate intent refuses dirty sheets, ambiguous/missing sources, empty names
  and normalized target collisions. Unique-name exhaustion uses a deterministic
  suffix search instead of host time.

`read`, `list` and `diff_page` produce bounded native evidence. Rows are paged
at 1–100; cell/metadata previews are depth/width/string bounded and mark
`truncated`. `more` reports omitted page items. Complete sheet export is an
explicit host file action. Rule sets are paged by rows, never as a single
unbounded nested item. Inspection previews must never be submitted as saves.

## One sheet copied from another project

`design_config::plan_sheet_copy` / op `sheet_copy_plan` decides copying one
Settings sheet from a source project's configuration read into a destination's;
`copy_sheet` re-decides it and returns the value to write only while the
`plan_sha256` the caller reviewed still holds (`config_copy_plan_changed`).
The key is admitted only when the source holds it AND its configuration
`groups` declare it (`config_sheet_unknown`, `config_sheet_not_admitted`), so
attached overlays and stray keys never travel. The value passes exactly the
validation an edit passes: `save_sheet` against the destination's own sheet,
or against the source's model for that sheet when the destination lacks it.
`save_sheet` itself still refuses a sheet the document does not hold; only this
reviewed copy creates one. The plan names `create | replace | identical`, both
sides' key-order independent SHA-256 and row counts, the declaring `group` and
how many other destination sheets are kept, and binds both project ids. Hosts
read both configurations under the caller's own authority (source READ), write
through the destination's Settings save (destination WRITE) and verify a fresh
read-back; ds-client-core does this as `ProjectConfigurationChange::CopySheet`
behind `ds design config copy plan|apply --sheet <key>`.
