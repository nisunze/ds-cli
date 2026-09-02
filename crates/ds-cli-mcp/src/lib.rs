//! `ds mcp` — the Model Context Protocol face of the command line.
//!
//! Some agent hosts (VS Code, Copilot, Claude Desktop, Cursor, Codex) learn
//! tools only through MCP: a JSON-RPC server on stdio answering
//! `tools/list` and `tools/call`. This domain gives them `ds` without giving
//! them a second product surface:
//!
//! - no command schema is hand-written. Every command's inputs, effect,
//!   authority and refusals are read from `ds capabilities` at startup,
//!   descriptor by descriptor, so a command's typing can never drift from the
//!   CLI it fronts. What *is* written here is the routing above that: under
//!   the default `--exposure chapters` the bounded bootstrap and chapter tools
//!   and their prose live in `surface.rs`, and four profiles select by
//!   command id rather than by chapter. Those lists are held to the live
//!   registry by tests rather than by assertion — see
//!   `every_declared_chapter_except_the_catalog_is_routed` here and
//!   `crates/ds/tests/mcp.rs`;
//! - every `tools/call` is literally a `ds <path> … --output json` process,
//!   and the CLI's typed envelope (result or refusal) is returned verbatim;
//! - pairing with the running DS GridDesign stays the only authority. The
//!   server adds no credential, no network listener and no cache.
//!
//! What it deliberately does NOT do: batch calls, invent convenience tools,
//! or expose anything `ds capabilities` does not list. The moment it grows a
//! tool the CLI lacks, it is the second surface the product ruled out.

mod identity;
pub mod install;
pub mod resources;
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

pub(crate) const DESKTOP_NOT_PAIRED: Refusal = Refusal {
    code: "desktop_not_paired",
    when: "an MCP-invoked command whose live descriptor requires desktop authority cannot find or pair the installed DS GridDesign application within the bounded wait",
    remedy: "start DS GridDesign, sign in, and retry the MCP tool call",
};

pub(crate) const DESKTOP_SIGNED_OUT: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "an MCP-invoked command whose live descriptor requires a desktop user or project reaches a paired session that is signed out or has no selected project",
    remedy: "sign in and select the intended project in DS GridDesign, then retry the MCP tool call",
};

pub(crate) const PROFILE_EXPOSURE_INVALID: Refusal = Refusal {
    code: "mcp_profile_exposure_invalid",
    when: "a specialized profile was requested without typed command exposure",
    remedy: "pass `--exposure commands --profile <name>`, or omit `--profile`",
};

pub(crate) const PROFILE_TOO_BROAD: Refusal = Refusal {
    code: "mcp_profile_too_broad",
    when: "a specialized profile would exceed its bounded tool limit including `ds_catalog` and `ds_diagnostics`",
    remedy: "split the profile by operator workflow before publishing it",
};

pub(crate) const HOST_UNKNOWN: Refusal = Refusal {
    code: "mcp_host_unknown",
    when: "`--host` names a host this command has no configuration recipe for",
    remedy: "pass one of the supported host tokens reported by `ds mcp install --output json`",
};

pub(crate) const HOST_OS_MISMATCH: Refusal = Refusal {
    code: "mcp_host_os_mismatch",
    when: "the selected host cannot locally spawn this executable on this operating system",
    remedy: "run MCP installation from the compatible ds executable on the machine where the selected host runs",
};

pub(crate) const HOST_WRITE_UNSUPPORTED: Refusal = Refusal {
    code: "mcp_host_write_unsupported",
    when: "the selected adapter has a verified printable shape but no verified automatic merge",
    remedy: "copy the exact proposed entry into the reported user-level target by hand",
};

pub(crate) const CONFIG_UNWRITABLE: Refusal = Refusal {
    code: "mcp_config_unwritable",
    when: "the host's configuration file could not be read, parsed in its verified JSON or TOML format, or written",
    remedy: "read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand",
};

pub(crate) const CONFIG_CONFLICT: Refusal = Refusal {
    code: "mcp_config_conflict",
    when: "the derived lane/platform entry or legacy `ds` entry differs from this exact proposal",
    remedy: "inspect the previews; remove or rename only the conflict, then retry",
};
