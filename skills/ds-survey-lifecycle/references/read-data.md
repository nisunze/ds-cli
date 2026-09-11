# Read survey data for a decision

Read the contract for the selected command once. The command IDs below route
the task; the installed contract supplies flags, authority and refusals.

## Progress, quality or coverage questions

1. Bind to the requested project (`auth.project.status`). Resolve only unknown
   form slugs via `survey.project-forms.list`; use `survey.form.read` only to
   resolve a relevant field or schema revision. Do not guess a slug from “poles”.
2. Choose the measure: record count, distinct asset identity, or distinct
   surveyors. `survey.query` accepts `count` or `count_distinct`, at most two
   grouping fields and eight closed filters. Public `created_by` is supported.
   Take other field names from served form/query vocabulary, not UI labels or
   presumed database paths. Resolve whether the requested date means capture
   time or replication time; they answer different questions.
3. Ask one bounded aggregate question. Example, after replacing `FORM` with the
   exact participating slug:

   ```text
   ds survey query --form FORM --metric count --group-by created_by --output json
   ```

   Add only requested area/time/quality filters using the live `--filter`
   grammar. A grouped response is capped at 200 rows. If truncated, narrow or
   split the question into explicit disjoint scopes and retain each scope;
   never total overlapping groups or present the first page as the whole project.
4. For coverage, obtain the intended area, target asset list or agreed expected
   count through its owner. Report observations against that denominator.
   Without it, report recorded activity and unknown coverage—not a fabricated
   completion percentage. Zero rows do not prove a place was never visited.
5. Return the measure, units, scope and freshness with any unresolved evidence.
   Quality flags identify review candidates; they do not authorize deletion or
   a guessed correction. A requested existing-record edit needs its supported
   mutation workflow; `survey.entries.create` is not an update workaround.

## Spatial evidence or a design handoff

Use `survey.entries.select` with an exact form and WGS84 bbox in
`west,south,east,north` order. Default 100, maximum 500 rows. The reply contains
only entry identity, geometry, creator and replication time; preserve its
`selection_digest`, project/form/bbox and `complete`/`truncated` flags.

A truncated selection requires a narrower bbox; there is no pagination cursor.
If several selections are necessary, preserve each receipt and reconcile
boundary overlaps by canonical identity. This mutable mirror digest is not an
immutable design revision. The selection does not include every survey field,
photos or an engineering network model. Name required missing attributes before
claiming a design-ready handoff; use their declared owner operation if available.
Never invent conductor sizes, asset ratings, connections or source provenance.

For viewing/holding survey rows in the existing map, use `map.survey.download`.
It consumes the active Working Area; `--entire-project` is an explicit full-scope
request and can replace that scope. Verify the map project before invoking it.
Do not materialize the whole project to answer a simple count.

## Incremental delivery

`survey.entries.changes` takes one form, inclusive `--updated-after`, a bounded
`--limit` and optional exact `--cursor`. It returns coalesced mirror state,
including tombstones—not every edit or hard-delete history.

For a complete requested interval, continue `has_more` pages with unchanged
project/form/lower clock/limit and the returned cursor. Retain the old completed
checkpoint until the last page is `complete`; then advance to `upper_fence`.
Deduplicate by entry identity plus `firestore_updated_at`; apply tombstones to
remove live rows. An expired fence restarts from the previous completed
checkpoint, never the unfinished upper fence. Stop on a non-retryable refusal
and retain the exact partial state. Do not interpret a create/sync receipt as
proof the mirror contains that entry; governed readback establishes that.
