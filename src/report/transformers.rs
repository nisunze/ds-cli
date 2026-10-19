//! Portable implementation of the native client's transformer reference set.
use std::collections::HashSet;

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

/// Canonicalize and de-duplicate names, then return byte-wise identity order.
/// Selection order cannot change the admitted request or its report receipts.
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
    out.sort_unstable();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_reference_rules_sort_canonical_identities_and_fold_aliases() {
        assert_eq!(canonical_transformer_reference("!!!"), "transformer");
        assert_eq!(canonical_transformer_reference(""), "transformer");
        assert_eq!(canonical_transformer_reference("3 A"), "_3_a");
        assert_eq!(
            canonical_transformer_reference("ALL TRANSFORMERS"),
            "combined_transformer"
        );
        assert_eq!(
            transformer_set(["Z".into(), "a-b".into(), "A B".into()]).unwrap(),
            ["a_b", "z"]
        );
        assert!(transformer_set(["".into()]).is_err());
    }
    #[test]
    fn selection_permutations_fold_to_the_same_canonical_set() {
        assert_eq!(
            transformer_set([
                "TX-1".into(),
                "001 Main".into(),
                "tx_1".into(),
                "combined-transformers".into()
            ])
            .unwrap(),
            ["_001_main", "combined_transformer", "tx_1"]
        );
    }
}
