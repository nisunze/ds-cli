//! Publication policy data. Hosts supply facts; engine differences belong
//! here, never in a second native execution path.
use super::Identity;

pub struct Engine {
    pub name: &'static str,
    pub fingerprint_free_operations: &'static [&'static str],
    pub retained_preview_variant: Option<&'static str>,
    pub legacy_source_in_base: bool,
    pub publication: PublicationPolicy,
    dependency: Option<Dependency>,
}

#[derive(Clone, Copy)]
pub enum PublicationPolicy {
    Report,
    Solar,
}

struct Dependency {
    dependent_prefix: &'static str,
    prerequisite_prefix: &'static str,
    output_id: &'static str,
    variant: &'static str,
}

pub const ENGINES: &[Engine] = &[
    Engine {
        name: "network_reporter",
        fingerprint_free_operations: &["export-combined_transformer", "export-compounded_report"],
        retained_preview_variant: Some(crate::printing::retained_preview::VARIANT),
        legacy_source_in_base: true,
        publication: PublicationPolicy::Report,
        dependency: None,
    },
    Engine {
        name: "solar",
        fingerprint_free_operations: &[],
        retained_preview_variant: None,
        legacy_source_in_base: true,
        publication: PublicationPolicy::Solar,
        dependency: Some(Dependency {
            dependent_prefix: "report-",
            prerequisite_prefix: "calculate-",
            output_id: "report-input",
            variant: "default",
        }),
    },
];

pub fn engine(name: &str) -> Option<&'static Engine> {
    ENGINES.iter().find(|engine| engine.name == name)
}

/// Compatibility selectors for callers still requesting an engine-scoped
/// inventory. They select table data, never a second publication sequence.
pub fn report_engine() -> &'static Engine {
    &ENGINES[0]
}

pub fn solar_engine() -> &'static Engine {
    &ENGINES[1]
}

impl Engine {
    pub fn requires_source(&self, operation: &str) -> bool {
        !self.fingerprint_free_operations.contains(&operation)
    }

    pub fn refreshes_equal_bytes(&self, variant: &str) -> bool {
        self.retained_preview_variant == Some(variant)
    }

    /// A report pins the prerequisite output hash, rather than the head's
    /// inventory digest or revision counter.
    pub fn prerequisite(&self, identity: &Identity) -> Option<(Identity, &'static str)> {
        let dependency = self.dependency.as_ref()?;
        let resource = identity
            .operation
            .strip_prefix(dependency.dependent_prefix)?;
        if resource.is_empty() {
            return None;
        }
        Some((
            Identity {
                engine: self.name.into(),
                operation: format!("{}{resource}", dependency.prerequisite_prefix),
                variant: dependency.variant.into(),
            },
            dependency.output_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_aggregate_operations_are_fingerprint_free() {
        for engine in ENGINES {
            assert!(engine.requires_source("export-tx_1"));
            assert!(engine.requires_source("calculate-city"));
            assert!(engine.requires_source("report-city"));
            assert!(engine.requires_source("export_combined_transformer_report"));
            assert!(engine.requires_source("export-combined_transformer-suffix"));
            for operation in engine.fingerprint_free_operations {
                assert!(!engine.requires_source(operation));
            }
        }
        assert!(engine("unregistered").is_none());
    }
}
