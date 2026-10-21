//! Independent invalidation for immutable model-analysis support facts.
//! Profile calculation cannot renew or clear these dependencies. Uncharacterized
//! tables, lost receipt lineage and navigation stay conservative until Analyze.
use ds_grid_engine::CommandOutcome;
use ds_grid_model::TableKind;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub(super) struct Invalidation {
    whole_model: bool,
    changes_seen: bool,
    structures: BTreeSet<String>,
}

impl Invalidation {
    pub(super) fn advance(
        &mut self,
        reconcile: bool,
        receipts: &[CommandOutcome],
        defaults_unchanged: bool,
    ) {
        self.changes_seen = true;
        self.whole_model |= reconcile || receipts.is_empty() || !defaults_unchanged;
        for receipt in receipts {
            let scope = &receipt.delta.recalculation_scope;
            // Only the native section/support dependency contract has been
            // characterized here. In particular, do not borrow Profile terrain
            // windows as independent design-clearance or resistance authority.
            self.whole_model |= scope.whole_model
                || receipt.delta.changed_tables.iter().any(|table| {
                    let support_input = matches!(
                        table,
                        TableKind::Structures
                            | TableKind::TensionSections
                            | TableKind::TensionSectionSupports
                    );
                    // The engine regenerates stored spans as a companion to
                    // support/section edits. An explicitly authored span has no
                    // characterized support-fact fence here.
                    let derived_span = *table == TableKind::Spans
                        && !receipt
                            .delta
                            .declared_write_tables
                            .contains(&TableKind::Spans);
                    !support_input && !derived_span
                });
            self.structures
                .extend(scope.structures.iter().map(ToString::to_string));
        }
    }

    pub(super) fn permits_supports(&self, ids: &[String], structures: &BTreeSet<String>) -> bool {
        self.changes_seen
            && !self.whole_model
            && !ids.is_empty()
            && ids
                .iter()
                .all(|id| structures.contains(id) && !self.structures.contains(id))
    }
}
