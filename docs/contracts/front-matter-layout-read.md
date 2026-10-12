# Front matter layout reads

The project printing catalogue groups governed standard pages under its existing
Front matter purpose and page roles. A `governed_standard` location carries the
project, document id, template id, immutable revision and the API catalogue's
content SHA-256. Older locations can omit the content pin while listing pages;
read-only preview requires it and otherwise names the need to refresh the
catalogue before acquiring any page.

`printing::standard_read` admits a standard document against that location. It
checks captured project, kind, id, template, revision, role and content pin,
then uses the existing `standards::Templates::document_by_id` authority to
validate the body, paper, typography, semantic layer declarations and authored
logo slots. Recomputing a changed body's digest cannot replace the held
catalogue pin. The returned `TemplateDocument` preserves the original layout
and unbound logo slots.

The same operation is available through the declared JSON request of
`report.layout.edit`: `op: "read_governed_standard"`, `location`, and `response`
containing the captured `report.standard.get --kind standard` data. It has no
project mutation or rendering effect. Composition separately binds project
identity, logos and engineering context through the existing CLI controls.

Native Desktop standard reads accept `held.standard_location`. That location
must address the request's explicit project and standard id, and must have
valid pins before a cache read or acquisition. Every returned answer is
admitted against the held location, including retained answers. The canonical
`report.standard.get` response remains unchanged, with its original document.

The owning unit tests cover MV and LV page parity, CLI JSON request parity,
crossed identities and revisions, altered content, unchanged-revision digest
forgery, and the preservation of intentionally unbound authored logo slots.
