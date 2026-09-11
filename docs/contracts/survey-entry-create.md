# Survey entry create kernel

`survey_entry_create` is the transport-free authority for one create-only
Survey mutation. Hosts provide a selected project and perform the request;
the kernel constructs the canonical JSON body and validates a successful
receipt. It has no URL, session, service-error classification, retry, or
Firestore dependency.

The typed API is `Request`, `Origin`, `plan(&Request, project_id)`, and
`validate_receipt(&Plan, status, response)`. `Refusal::InvalidInput` means
the closed request grammar was refused. `Refusal::UnreadableResponse` means a
purported successful response was not an exact create receipt. Hosts retain
their public error types and map those two categories.

The `surveyEntryCreate` WASM entry point accepts JSON with schema
`ds.survey-entry-create/v1`. Operation `plan` returns
`{ identity, payload }`; operation `receipt` additionally accepts `status`
and the full response JSON and returns the verified receipt. Operation
`normalize` has the same output as `plan` and accepts durable aliases
`eds_project_id`, `form_slug`, and `entry_id`. A supplied `row` can provide
the stable identity, idempotency key, and a millisecond `created_at` fallback;
every duplicate identity or idempotency value must agree. It never generates
an identity or replay key. Empty legacy `context_key` becomes absent.

`surveyEntryGeometryValidate` accepts
`{ "schema": "ds.survey-entry-geometry/v1", "geometry": ... }` and returns
the accepted GeoJSON object. It shares the exact strict GeoJSON grammar used
by create, so browser update adapters do not reimplement it.
