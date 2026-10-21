//! Shared, cheap comparison of an owner's captured and current source basis.
//! The owner supplies a complete identity/source fence. This is neither an
//! authorization decision nor an acquisition policy: no IO, TTL or timer runs.
//! Obtain current stamps only when a consumer intentionally needs current data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
    Unknown,
}

pub fn compare<T: Eq + ?Sized>(held: Option<&T>, current: Option<&T>) -> Freshness {
    match (held, current) {
        (Some(held), Some(current)) if held == current => Freshness::Fresh,
        (Some(_), Some(_)) => Freshness::Stale,
        _ => Freshness::Unknown,
    }
}

pub fn is_fresh<T: Eq + ?Sized>(held: Option<&T>, current: Option<&T>) -> bool {
    compare(held, current) == Freshness::Fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_current_or_captured_basis_never_claims_freshness() {
        for (held, current) in [(None, None), (Some("v0"), None), (None, Some("v0"))] {
            assert_eq!(compare(held, current), Freshness::Unknown);
            assert!(!is_fresh(held, current));
        }
    }

    #[test]
    fn equal_versions_require_the_same_complete_owner_scope() {
        let held = ("uid-a", "canary", "audience-a", "project-a", "report", "v0");
        assert!(is_fresh(Some(&held), Some(&held)));
        for current in [
            ("uid-b", "canary", "audience-a", "project-a", "report", "v0"),
            ("uid-a", "stable", "audience-a", "project-a", "report", "v0"),
            ("uid-a", "canary", "audience-b", "project-a", "report", "v0"),
            ("uid-a", "canary", "audience-a", "project-b", "report", "v0"),
            ("uid-a", "canary", "audience-a", "project-a", "solar", "v0"),
            ("uid-a", "canary", "audience-a", "project-a", "report", "v1"),
        ] {
            assert_eq!(compare(Some(&held), Some(&current)), Freshness::Stale);
        }
    }
}
