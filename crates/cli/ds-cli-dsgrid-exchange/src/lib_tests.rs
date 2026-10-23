use crate::request::SHARED_ARGS;
use ds_cli_contract::spec::ArgKind;
use ds_network_contracts::grid_exchange::{
    EXCHANGE_REPEATED_FLAGS, EXCHANGE_SWITCH_FLAGS, EXCHANGE_VALUE_FLAGS,
};
use std::collections::BTreeSet;

/// Every flag `plan`/`convert` declare is a choice the addon's exchange owner
/// admits, and every choice it admits is declared here: a flag the owner did
/// not know would be refused, and one it knows but `ds` never sends would be
/// a capability no caller can reach.
#[test]
fn the_declared_choices_are_exactly_the_contract_flags() {
    let of = |kind: ArgKind| -> BTreeSet<&str> {
        SHARED_ARGS
            .iter()
            .filter(|arg| arg.kind == kind && arg.name != "source")
            .map(|arg| arg.name)
            .collect()
    };
    assert_eq!(of(ArgKind::Value), EXCHANGE_VALUE_FLAGS.iter().copied().collect());
    assert_eq!(
        of(ArgKind::Repeated),
        EXCHANGE_REPEATED_FLAGS.iter().copied().collect()
    );
    assert_eq!(of(ArgKind::Switch), EXCHANGE_SWITCH_FLAGS.iter().copied().collect());
}
