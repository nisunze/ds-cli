//! Every call the shared client can make, this machine's native transport
//! actually sends.
//!
//! `ds_client_core::Transport` gives most calls a default body answering
//! `TransportError::Unreachable`, so test fakes need not stub every route. A
//! route the native transport forgets therefore compiles, and every command
//! on it refuses `auth_transient` before a request leaves the machine, while
//! the UI, holding the same JWT, performs it: `ds auth project create|update`
//! did exactly that until cafbd9e (feedback 44df2bb7, aaaef55b). The owner's
//! rule is that what the UI can do, the CLI can do; this holds the native
//! transport to every defaulted route of the kernel it is pinned to.

use std::collections::BTreeSet;
use std::path::Path;

/// The body of the first `header` block at column zero, up to its closing
/// brace at column zero.
fn block<'a>(source: &'a str, header: &str) -> &'a str {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` is gone; update this parity test"));
    let body = &source[start..];
    let end = body.find("\n}\n").expect("the block closes at column zero");
    &body[..end]
}

/// The name of a four-space-indented `fn`, if this line declares one.
fn declared(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("    fn ")?;
    let end = rest.find(['(', '<'])?;
    Some(&rest[..end])
}

#[test]
fn the_native_transport_sends_every_call_the_shared_client_defines() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kernel = std::fs::read_to_string(
        manifest.join("../../../ds-command-kernel/crates/ds-client-core/src/transport.rs"),
    )
    .expect("the pinned sibling kernel is checked out beside ds-cli");
    let native = std::fs::read_to_string(manifest.join("src/transport.rs"))
        .expect("the native transport source");

    // Routes whose trait body answers Unreachable: the ones the compiler
    // cannot hold an implementation to.
    let mut defaulted = BTreeSet::new();
    let mut current: Option<&str> = None;
    for line in block(&kernel, "pub trait Transport {").lines() {
        if let Some(name) = declared(line) {
            current = Some(name);
        }
        if line.contains("TransportError::Unreachable")
            && let Some(name) = current
        {
            defaulted.insert(name);
        }
    }
    assert!(
        defaulted.len() > 20,
        "the trait parse found only {} defaulted routes; the parser is stale",
        defaulted.len()
    );

    let implemented: BTreeSet<&str> = block(&native, "impl Transport for NativeTransport {")
        .lines()
        .filter_map(declared)
        .collect();
    let missing: Vec<&&str> = defaulted.difference(&implemented).collect();
    assert!(
        missing.is_empty(),
        "the native transport never sends {missing:?}: every command on these routes would \
         refuse auth_transient before a request leaves the machine, while the UI performs it"
    );
}
