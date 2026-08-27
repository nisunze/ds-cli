# TODO — governance actions through `ds` and MCP

**Status: recorded, not implemented.** `ds feedback close` shipped on this
branch as an ordinary paired command. The governance surface it belongs to is
described here so it is designed once, deliberately, rather than grown one
admin command at a time.

## The rule

Closing a feedback report is an **admin action**, not ordinary product use. So
are the other governance actions the stack already gates. Every one of them
must be:

1. reachable by a user whose **JWT carries the capability** — not by a
   privileged CLI, a service identity, or a local flag; and
2. exposed as an **MCP tool**, so an agent driving the stack for that user can
   perform the governance step in the same surface as everything else.

## What is true today

- Authority runs through the paired DS GridDesign session. `ds` holds no
  token; the application calls ds-brain under its signed-in Firebase user.
  The "right JWT" is therefore the desktop user's, and nothing else can be.
- ds-brain gates the governance actions server-side on the `platform.admin`
  capability: feedback status/resolution triage
  (`internal/handlers/feedback.go`), the SRE overview and event reads
  (`internal/handlers/sre.go`), the Project Work preview
  (`internal/handlers/pm.go`), and the platform-admin tabular sources
  (`internal/bulk/services/tabular.go`).
- `ds` already carries two of these: `ds sre …` and now `ds feedback close`.
  Both learn the gate only by being refused — `feedback_not_permitted` is a
  classification of the application's own message, after the round trip.
- MCP tools are generated from the live descriptors, so `feedback.close`
  already becomes a tool in the `operations` chapter and profile. Nothing in
  that generation knows it is privileged.

## The gaps

1. **No governance rung in the authority vocabulary.** `Authority` is
   `none | desktop_pairing | desktop_user | project`. A command that needs a
   platform capability declares `desktop_user`, exactly like one that does
   not. A caller cannot tell before running which is which.
2. **No capability discovery.** Nothing answers "may this session triage?"
   before the mutation is attempted. `ds desktop status` reports pairing and
   build profile, not the signed-in user's resolved capabilities. Every
   governance attempt by a non-admin costs a round trip and a refusal.
3. **No governance shape in MCP.** Privileged tools are published to every
   host identically. There is no profile that says "these are the governance
   tools", and no annotation that would let a host hide, or a model avoid,
   what the current session cannot use.
4. **The actor is invisible in the CLI's own record.** ds-brain stamps
   `updated_by` from the verified principal, which is correct — but `ds`
   reports the outcome without ever naming who the close was made as. An
   agent closing a backlog item should have to see the identity it is acting
   under.

## Sketch (not a decision)

- Add one authority rung — `desktop_admin` or `capability` — declared by
  `feedback.close`, `sre.overview`, `sre.events`. It changes help, the
  descriptor, and the MCP annotation together, from one field.
- Add a read-only probe that returns the paired session's resolved
  capabilities (the application already holds them), so a governance command
  can refuse locally with a named code and an agent can branch before it acts.
- Publish a `governance` MCP profile over exactly the commands on that rung,
  and mark those tools as privileged in the tool annotation the host reads.
- Report the acting identity in every governance command's success payload.

## Open decisions for the owner

- **D1** — Is `platform.admin` the one governance capability for the CLI, or
  does a narrower `feedback.triage` cap get introduced in ds-brain first?
- **D2** — Should a governance command refuse locally when the probe says the
  session lacks the capability, or always attempt and let ds-brain be the only
  authority? (Local refusal is cheaper; server-only is one source of truth.)
- **D3** — Does the `governance` MCP profile ship to every host, or only when
  the operator asks for it at `ds mcp install` time?
- **D4** — Reopening a closed report is deliberately absent from `ds`. Does it
  stay a human-only action in the `fb` tab, or is it the next governance
  command?

Nothing in this file is implemented. Do not treat it as a contract.
