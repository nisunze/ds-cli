//! Portable implementation of the native client's transformer reference set.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_TRANSFORMERS: usize = 500;
pub const MAX_NAME_CHARS: usize = 200;

pub fn canonical_transformer_reference(value: &str) -> String {
    let lowered = value.to_lowercase();
    let mut canonical = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            canonical.push(ch);
        } else {
            canonical.push('_');
        }
    }
    let mut canonical = canonical.trim_matches('_').to_owned();
    if matches!(
        canonical.as_str(),
        "all_transformers" | "combined_transformers" | "combined_transformer"
    ) {
        return "combined_transformer".to_owned();
    }
    if canonical.is_empty() {
        canonical.push_str("transformer");
    }
    if canonical.as_bytes()[0].is_ascii_digit() {
        canonical.insert(0, '_');
    }
    canonical
}

pub fn transformer_set(
    names: impl IntoIterator<Item = String>,
) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        if name.is_empty()
            || name.trim() != name
            || name.chars().count() > MAX_NAME_CHARS
            || name
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return Err("a transformer name is empty, untrimmed, or outside its bound");
        }
        let canonical = canonical_transformer_reference(&name);
        if canonical.chars().count() > MAX_NAME_CHARS {
            return Err("a canonical transformer name is outside its bound");
        }
        if seen.insert(canonical.clone()) {
            out.push(canonical);
        }
        if out.len() > MAX_TRANSFORMERS {
            return Err("one request names more transformers than the fixed bound");
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Admitted {
    pub transformer: String,
    pub revision: u64,
}

/// Admitted rooms retain first-seen order. Aliases with conflicting revisions
/// cannot identify one exact input room and are refused before staging.
pub fn admitted(inputs: Vec<Admitted>) -> Result<Vec<Admitted>, String> {
    if inputs.len() > MAX_TRANSFORMERS {
        return Err("Combined report exceeds 500 admitted rooms".into());
    }
    let names = transformer_set(inputs.iter().map(|i| i.transformer.clone()))?;
    let mut revisions = HashMap::new();
    for input in inputs {
        let name = canonical_transformer_reference(&input.transformer);
        if crate::report::is_reserved_report_identity(&name) {
            return Err(format!("Combined report transformer {name} is reserved."));
        }
        if input.revision > 9_007_199_254_740_991 {
            return Err(format!(
                "Combined report transformer {name} has no exact admitted revision."
            ));
        }
        if revisions
            .insert(name.clone(), input.revision)
            .is_some_and(|prior| prior != input.revision)
        {
            return Err(format!(
                "Combined report transformer {name} has conflicting admitted revisions."
            ));
        }
    }
    if names.len() < 2 {
        return Err(
            "A local combined report requires at least 2 admitted transformer rooms.".into(),
        );
    }
    Ok(names
        .into_iter()
        .map(|transformer| Admitted {
            revision: revisions[&transformer],
            transformer,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_reference_rules_keep_first_seen_order_and_fold_aliases() {
        assert_eq!(canonical_transformer_reference("!!!"), "transformer");
        assert_eq!(canonical_transformer_reference(""), "transformer");
        assert_eq!(canonical_transformer_reference("3 A"), "_3_a");
        assert_eq!(
            canonical_transformer_reference("ALL TRANSFORMERS"),
            "combined_transformer"
        );
        assert_eq!(
            transformer_set(["Z".into(), "a-b".into(), "A B".into()]).unwrap(),
            ["z", "a_b"]
        );
        assert!(transformer_set(["".into()]).is_err());
    }
    #[test]
    fn exact_revision_admission_refuses_conflicts_reserved_and_too_small_sets() {
        let a = |name: &str, revision| Admitted {
            transformer: name.into(),
            revision,
        };
        assert_eq!(
            admitted(vec![a("Z", 1), a("a-b", 2), a("A B", 2)]).unwrap(),
            vec![a("z", 1), a("a_b", 2)]
        );
        assert!(admitted(vec![a("z", 1), a("Z", 2)]).is_err());
        assert!(admitted(vec![a("combined_transformer", 1), a("z", 2)]).is_err());
        assert!(admitted(vec![a("z", 1), a("Z", 1)]).is_err());
    }
}
