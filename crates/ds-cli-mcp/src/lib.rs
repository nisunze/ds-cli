//! `ds mcp` — the Model Context Protocol face of the command line.
//!
//! Some agent hosts (VS Code, Copilot, Claude Desktop, Cursor, Codex) learn
//! tools only through MCP: a JSON-RPC server on stdio answering
//! `tools/list` and `tools/call`. This domain gives them `ds` without giving
//! them a second product surface:
//!
//! - the tool list is BUILT from `ds capabilities` at startup, descriptor by
//!   descriptor — nothing here is hand-written, so the list can never drift
//!   from the CLI it fronts;
//! - every `tools/call` is literally a `ds <path> … --output json` process,
//!   and the CLI's typed envelope (result or refusal) is returned verbatim;
//! - pairing with the running DS GridDesign stays the only authority. The
//!   server adds no credential, no network listener and no cache.
//!
//! What it deliberately does NOT do: batch calls, invent convenience tools,
//! or expose anything `ds capabilities` does not list. The moment it grows a
//! tool the CLI lacks, it is the second surface the product ruled out.

pub mod install;
pub mod serve;
pub mod surface;
pub mod tools;

use ds_cli_contract::spec::{Availability, Domain, Refusal};

pub static DOMAIN: Domain = Domain {
    id: "mcp",
    summary: "Serve `ds` to MCP hosts; install the host entry.",
    commands: &[&serve::COMMAND, &install::COMMAND],
};

pub(crate) fn always() -> Availability {
    Availability::Available
}

pub(crate) const CAPABILITIES_UNAVAILABLE: Refusal = Refusal {
    code: "mcp_capabilities_unavailable",
    when: "`ds capabilities` could not be read from this executable while building the tool list",
    remedy: "run `ds capabilities --output json` by hand and fix what it reports before serving",
};

pub(crate) const STDIO_UNAVAILABLE: Refusal = Refusal {
    code: "mcp_stdio_unavailable",
    when: "standard input closed or failed before the host finished the session",
    remedy: "start the server from an MCP host as a stdio server; it is not an interactive command",
};

pub(crate) const PROFILE_EXPOSURE_INVALID: Refusal = Refusal {
    code: "mcp_profile_exposure_invalid",
    when: "a specialized profile was requested without typed command exposure",
    remedy: "pass `--exposure commands --profile <name>`, or omit `--profile`",
};

pub(crate) const PROFILE_TOO_BROAD: Refusal = Refusal {
    code: "mcp_profile_too_broad",
    when: "a specialized profile would publish more than 15 tools including `ds_catalog`",
    remedy: "split the profile by operator workflow before publishing it",
};

pub(crate) const HOST_UNKNOWN: Refusal = Refusal {
    code: "mcp_host_unknown",
    when: "`--host` names a host this command has no configuration recipe for",
    remedy: "pass one of the hosts listed in `ds mcp install --help`, or omit --host to print the generic entry",
};

pub(crate) const CONFIG_UNWRITABLE: Refusal = Refusal {
    code: "mcp_config_unwritable",
    when: "the host's configuration file could not be read, parsed as JSON, or written",
    remedy: "read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand",
};
