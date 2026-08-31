---
name: ds-report-authoring
description: Write or revise a consumer-facing engineering or business report without exposing the product name, internal technology, commands, repositories, build details, or implementation trace. Use for report narrative, titles, captions, filenames, document properties, and delivery metadata; not for internal incident or architecture reports.
---

# Author a consumer-facing report

Write for the person using the result. They need the work, evidence, limits and
decision — never the machinery that produced it.

## Establish the report boundary

Before drafting, identify from the request and available evidence:

- the consumer and the decision the report supports;
- the exact subject, place, period and deliverable scope;
- the calculations, observations and approved source evidence;
- required standards, contractual references and external attribution.

Do not invent a missing result. State an evidence gap in domain language and
keep it separate from an implementation failure.

## Keep the deliverable consumer-facing

Include only material that helps interpret or act on the work:

- purpose, scope and assumptions;
- inputs and methods expressed in the professional domain;
- results with units and evidence;
- limitations, risks, conclusions and recommendations;
- author, organization, date and required approval or attribution.

Never put any of the following in the report, including its title, body,
headers, footers, captions, appendices, filenames or document properties:

- the product or platform name;
- command-line, agent or model-control terminology;
- internal services, protocols, storage, frameworks or programming languages;
- command ids, raw receipts, logs, source revisions, branches or repository paths;
- build, deployment, environment or infrastructure details;
- internal debugging messages or implementation workarounds.

An external professional tool, published dataset or standard may be named only
when it is material to reproducibility, interpretation, attribution or a
contractual requirement. Describe calculation logic that affects the result;
do not confuse implementation technology with methodology.

## Separate traceability from the report

Preserve technical provenance in an operator-only receipt or handover when it
is needed for verification. Keep that record outside the consumer deliverable
and do not package it as a report appendix. The internal record may identify
commands, versions and artifact digests; the report may identify only the
consumer-relevant source, revision date and approved evidence.

## Final leakage review

Before delivery, inspect every visible field and embedded property:

1. Confirm the report stands alone without knowledge of the producing system.
2. Replace implementation explanations with domain methods or remove them.
3. Check titles, headers, footers, captions, filenames, links, attachment names,
   authoring properties and export metadata for internal names or technology.
4. Confirm units, assumptions, limitations and required external attribution
   remain intact.
5. Keep any operator receipt separate and label it internal; do not deliver it
   to the consumer unless the user explicitly changes the audience and scope.

If the requested deliverable is an internal incident, architecture or software
report, do not apply this consumer-report boundary to it.
