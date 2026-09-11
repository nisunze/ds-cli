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
