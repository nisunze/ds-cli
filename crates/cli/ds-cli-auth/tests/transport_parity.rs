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
//! transport to every defaulted route of its shared typed client.

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

fn ordinary_routes<'a>(source: &'a str, obligation: Option<&str>) -> BTreeSet<&'a str> {
    block(source, "macro_rules! __ds_gateway_calls {")
        .lines()
        .filter_map(|line| {
            let (_, row) = line.split_once("; ")?;
            let mut fields = row.split(',').map(str::trim);
            let required = fields.next()?;
            let name = fields.next()?;
            obligation
                .is_none_or(|expected| expected == required)
                .then_some(name)
        })
        .collect()
}

fn implemented<'a>(native: &'a str, compatibility: &'a str) -> BTreeSet<&'a str> {
    let body = block(native, "impl Transport for NativeTransport {");
    let mut methods: BTreeSet<_> = body.lines().filter_map(declared).collect();
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("ds_client_core::__ds_gateway_calls!(native,gateway_transport::send);") {
        methods.extend(ordinary_routes(compatibility, None));
    }
    methods
}

#[test]
fn the_native_transport_sends_every_call_the_shared_client_defines() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kernel =
        std::fs::read_to_string(manifest.join("../../kernel/ds-client-core/src/transport.rs"))
            .expect("the shared client transport is in this workspace");
    let native = std::fs::read_to_string(manifest.join("src/transport.rs"))
        .expect("the native transport source");
    let compatibility =
        std::fs::read_to_string(manifest.join("../../kernel/ds-client-core/src/gateway_compat.rs"))
            .expect("the closed ordinary gateway method list");

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
    defaulted.extend(ordinary_routes(&compatibility, Some("optional")));
    assert!(
        defaulted.len() > 20,
        "the trait parse found only {} defaulted routes; the parser is stale",
        defaulted.len()
    );

    let actual = implemented(&native, &compatibility);
    let missing: Vec<&&str> = defaulted.difference(&actual).collect();
    assert!(
        missing.is_empty(),
        "the native transport never sends {missing:?}: every command on these routes would \
         refuse auth_transient before a request leaves the machine, while the UI performs it"
    );
    // Removing the native forwarder must expose the default-Unreachable gap.
    let without_forwarder = native.replace(
        "ds_client_core::__ds_gateway_calls!(native, gateway_transport::send);",
        "",
    );
    assert_ne!(
        without_forwarder, native,
        "the planted control found no forwarder"
    );
    let incomplete = implemented(&without_forwarder, &compatibility);
    assert!(defaulted.difference(&incomplete).next().is_some());
}
