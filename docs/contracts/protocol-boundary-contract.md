# Protocol boundary contract

This repository is `ds-cli`. The product is a command line: one executable,
one command registry, one output envelope. MCP is a *way to reach it*, not
what it is, and the same is true of whatever replaces MCP.

## The rule

**A host protocol may know about the CLI. The CLI may not know about a host
protocol.**

Concretely:

- `ds-cli-mcp` links `ds-cli-contract` and `ds-cli-skills`, and no domain
  crate. It reaches a command the way a person at a terminal does: it runs
  `ds <argv>` and reads the envelope (`run_cli` in
  `crates/ds-cli-mcp/src/tools.rs`). It has no private route into a domain,
  because it has no domain in its dependency graph to route into.
- Only `crates/ds` — the binary — depends on `ds-cli-mcp`, and it registers
  `mcp serve` / `mcp install` as one ordinary domain with the same
  `Entry { command, handler, render }` shape as every other domain. Removing
  the protocol is removing rows from a table.
- No domain crate contains the crate name, a module path, or an environment
  variable named for a host protocol.

A new protocol is therefore a sibling leaf crate written against two things:
the `Command` descriptor in `ds-cli-contract`, and `run_cli`. It is not a pass
over twenty-six domain crates.

## Prose is not coupling

`ds-cli-contract` has doc comments that name MCP — "MCP uses this when it
projects a descriptor into a tool". That is documentation of a consumer and it
costs nothing to delete. The fields those comments describe are CLI-native and
would exist with MCP deleted: `Chapter` is declared by commands in every
domain, `Authority::from_token` parses the CLI's own authority vocabulary,
`command_json_unchecked` is the CLI's own schema without a live availability
probe. Nothing in the pin objects to the word MCP in a comment.

What the pin forbids is *code* coupling: a crate name, a module path, a
protocol-named variable.

## Why an environment variable is the dangerous one

The crate graph catches a domain that links a protocol — the compiler shows
the edge, and a reviewer sees the manifest line. An environment variable
crosses the same boundary with no edge at all. Two did:

| Was | Read in | Is now |
|---|---|---|
| `DS_MCP_CHILD` | `ds-cli-auth`, to refuse a password prompt | `DS_CLI_NONINTERACTIVE` |
| `DS_MCP_SCHEMA_ONLY` | `crates/ds/src/meta.rs`, in `ds capabilities` | `DS_CLI_SCHEMA_ONLY` |

Neither fact is about MCP. One is *"no human is at a terminal"*; the other is
*"a schema is enough without resolving live availability"*. Both are true of
any machine host. Under the old names the next protocol had two bad options:
impersonate MCP by setting a variable named for a protocol it does not speak,
or add a second branch inside the auth domain. Under the new names it sets the
same two variables and nothing below the adapter changes.

**The test to apply: name the fact, not the caller.** If the name of a host
appears in something a domain reads, the fact has been named after one of its
consumers and the boundary has already moved.

## What holds it

`crates/ds/tests/protocol_boundary.rs` pins the inventory rather than this
prose, for the reason `process-boundary-contract.md` gives: a structural claim
enforced by reading stays true until the afternoon somebody adds one line, with
every other test still green. The suite asserts the dependency edges, that the
adapter links no domain, and that no domain source carries a coupling token.

Adding a row to `PROTOCOL_CONSUMERS` or `COUPLING_OWNERS` is a deliberate act.
It means a domain now compiles against a protocol, and the next protocol will
cost an edit there.
