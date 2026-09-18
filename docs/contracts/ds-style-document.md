# The style document — one owner for the style family's wire

**Status:** implemented on `fable/style-document`, 2026-09-11.
Parent: [The application kernel](ds-application-kernel.md) §2, §4a, §6a.
Sibling: [The layer domain](ds-layer-state.md) — `layer_document` owns the
drawer's document and its envelope; this module owns the style family's wire
and reuses that envelope reading.

## 1. The question this module owns

What a client asks ds-brain for in the style family, and what it accepts back.
Four backend actions, one kernel module, the same answer for `ds`, the desktop
shell and the browser.

| action (kernel) | wire action | operation | response bound |
|---|---|---|---|
| `publish` | `update_style` | `domains.styles.update` | 1 MiB |
| `seed` | `seed_style` | `domains.styles.action` | 4 MiB |
| `read_shared` | `get_user_data_style` | `domains.styles.action` | 4 MiB |
| `assemble_runtime` | `assemble_work_map_runtime_style` | `domains.styles.action` | 4 MiB |

Schema `ds.style-document/v1`; ops `save_payload`, `request`, `admit`. WASM
export `styleDocument`; `ds-client-core::{styles, transport, authorized}`
delegate for the native clients. Every `Err(String)` out of `evaluate` is a
malformed request; every refused **answer** is
`{"outcome":"refused","refusal":{code,message,issues}}`.

## 2. The rules

- **The publish payload is closed.** `style_ref` (1..=500, trimmed, no control
  characters), a non-empty `style_target`, a `style_data` object naming its
  `type`; optional string `slug|template_name|styling_template_id|layer_name|
  project_id`, `available_fields` of strings, a `field_values` object, a bool
  `create_only`. Anything else is `unknown style payload field <k>`. Serialized
  ≤ 2 MiB (`style_authoring::MAX_STYLE_REQUEST_BYTES`).
- **Routing is the editor row's, then the reference's.** `style_save_payload`
  takes target and identity from `/layers.style_editors`. When the row cannot
  route — absent, no `style_target`, or neither `layer_name` nor `slug` — a
  shared-class reference routes itself: `ud/<slug>` → `user_data`,
  `pd/<slug>` → `project_data`, `gt/<slug>` → `global_tile`, with `<slug>` (the
  trimmed, non-empty tail) as the layer name. Any other shape fails closed:
  `no backend style routing for <ref>: /layers.style_editors must declare
  style_target plus layer_name or slug`. Only a project-scoped target carries
  `project_id`.
- **One seed geometry map.** `point|multipoint` → `point`,
  `line|linestring|multilinestring` → `line`, `polygon|multipolygon|fill` →
  `polygon`, case-insensitive; anything else is
  `no styleable seed geometry for <value>`, never a silent point.
- **A publish receipt is the style that was authored** or it is not admitted:
  `updated == true`, a `style_data` object, the authored `style_target`, and
  every optional identity the payload declared answered unchanged. Otherwise
  `unreadable_response` "style publish receipt does not match the authored
  target".
- **Warnings are text.** A string (≤ 4096 bytes), or an issue object
  (`{path|property, message}` or `{property, issue:{message}}`) normalised to
  `"<path>: <message>"`; an object with no message is dropped; ≤ 1000 entries.
  Anything else is "style publish warnings are malformed".
- **A refusal keeps the backend's words**, `"<error> (<status> <code>)"`, plus
  up to 50 `error_details.issues[]` as `"<path>: <message>"` strings.
- **A seed is never persisted**: `persisted == false`, the ref and canonical
  geometry that were asked for, `kind` ∈ {`geojson`,`vt`}, a typed document.
- **An absence is an answer**: `read_shared` admits `found: false`; a `found`
  document without its `style_data` is a contract slip, not an empty style.
- **A runtime family is ds-brain's**: `authority ==
  ds-brain/work-map-runtime-family/v1`, the root and geometry that were asked
  for, non-empty `source_id`/`layer_id`/`style_ref`, 1..=100 layers that each
  name an id, a type and *that* source, ≤ 500 `required_images`.
- **Never panic**: every optional section is read with `.get()`; a
  `serde_json::Map` is never indexed (a missing key panics, and in the browser
  that surfaces as WASM `unreachable`).

## 3. Two decisions to challenge

1. **Derived routing lives in `style_save_payload`, not in `plan_style_edit`.**
   A direct save of a shared-class ref (the Style Center's Save, `ds style`
   over an unregistered layer) now routes itself, but *guided* native
   authoring still refuses before it ever reaches the payload, because
   `plan_style_edit` requires the editor row for the style document, its
   properties and its field domains. The asymmetry is deliberate — guided
   editing has nothing to guide without the row — and it is recorded here
   rather than hidden.
2. **Object warnings are admitted and normalised.** ds-brain's expression
   warnings are `{path, message}` issue objects. The browser has always shown
   them; the native client refused the entire publish receipt over them, so
   the same backend answer succeeded in one host and failed in the other. The
   kernel now normalises them for both. A warning that is neither text nor a
   named issue is still a contract slip.

Also worth a ruling: `StylesCall::response_limit()` still answers the layers
route's 32 MiB, while the registry declares 1 MiB for `domains.styles.update`.
The kernel answers the declared bound; the native transport was left as it is
in this slice.

## 4. The parity proof

`tests/fixtures/style-document-cases.json` — 35 cases, byte-identical to
ds-web's `src/tests/fixtures/style-document/cases.json`. `tests/
style_document_cases.rs` replays every case in the kernel and holds the corpus
to every op, action and outcome; ds-web replays the same file through the
vendored WASM. Re-record with `DS_STYLE_DOCUMENT_CASES_WRITE=1`, and say why.
The 2 MiB payload and 16 MiB input bounds are proven in-module: a fixture both
hosts parse cannot carry them.

## 5. What TypeScript still holds, and why

| Left in TypeScript | Why it is presentation or a binding |
|---|---|
| `api/styles.ts` transport helpers | the browser adapter performs the declared operation and hands the parsed answer to the kernel |
| render-ready preview assembly (`renderReadyStyleForMap`, `buildLocalStyleState`) | a presentation mirror of ds-brain's runtime image and companion minting — a preview, never the published truth |
| `StyleCenterDialog.svelte` banners, tabs, local save | chrome over the kernel's answer; the issue and warning *text* is now the kernel's |
| `layers/user-data-style.ts` opening the dialog | which surface opens, not what is asked for or accepted |

## Independent symbol icon placement

Guided appearance accepts optional `icon_overlap: bool` for symbol documents.
It sets `layout.icon-allow-overlap` and `layout.icon-ignore-placement` together.
On keeps a separate label layer from displacing its own icon. Off restores both
flags to false. The operation preserves label overlap, icon identity, paint,
field dimensions and other authorship. Non-symbol or non-boolean requests refuse.
Only the explicitly addressed screen or print document changes; a seed or clone
is a starting copy, never a live link between those targets.
