//! Versions and revisions of one project DS Grid model, as the catalog lists them.
//!
//! A model has versions v1, v2, …; every save is a revision of the version
//! that was current when it was saved, and only an explicit bump starts the
//! next version. The catalog lists revisions newest version first, but orders
//! the revisions INSIDE one version by document id, which is a content hash
//! and says nothing about time. The order within a version is therefore read
//! from the only authoritative source it has: each revision names the head it
//! was saved on (`parent_revision_id`), so one version's revisions form a single
//! chain whose first link's parent lies outside the version.
//!
//! A Server that stores `revision_ordinal_within_version` on every revision
//! (legacy rows read as 1: each legacy version held one revision) is the
//! authority for ordinals, and its rows are used as they are. Only rows from
//! a Server that predates stored ordinals are ordered by the chain.
//!
//! Hosts page the catalog and hand the rows here; everything that decides an
//! ordinal, a count or whether either is exact stays in this one place.
use serde_json::{json, Value};

/// Where one revision sits inside its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub version: u64,
    /// 1 for the revision that started the version. `None` when the rows seen
    /// do not reach the version's first revision, or do not form one chain.
    pub ordinal: Option<u64>,
    /// Revisions of this version seen. Exact only when `complete`.
    pub count: u64,
    /// True when every revision of the version was among the rows.
    pub complete: bool,
}

fn version_of(row: &Value) -> Option<u64> {
    row["version"].as_u64().filter(|v| *v > 0)
}

fn revision_of(row: &Value) -> Option<&str> {
    row["revision_id"].as_str().filter(|s| !s.is_empty())
}

/// The ordinal the Server stored on a row, when it stores one.
pub fn stored_ordinal(row: &Value) -> Option<u64> {
    row["revision_ordinal_within_version"]
        .as_u64()
        .filter(|n| *n > 0)
}

/// Order the rows of ONE version oldest first by following the parent chain.
/// `None` when they are not exactly one linear chain: then no ordinal is
/// claimed rather than one guessed from a hash order.
pub fn chronological(rows: &[&Value]) -> Option<Vec<usize>> {
    let ids: std::collections::HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| revision_of(row).map(|id| (id, index)))
        .collect::<Option<_>>()?;
    if ids.len() != rows.len() {
        return None;
    }
    let mut child = std::collections::HashMap::new();
    let mut roots = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match row["parent_revision_id"]
            .as_str()
            .filter(|p| ids.contains_key(p))
        {
            Some(parent) => {
                if child.insert(parent, index).is_some() {
                    return None;
                }
            }
            None => roots.push(index),
        }
    }
    let [mut at] = roots[..] else {
        return None;
    };
    let mut order = vec![at];
    while let Some(&next) = child.get(revision_of(rows[at])?) {
        order.push(next);
        at = next;
    }
    (order.len() == rows.len()).then_some(order)
}

/// The position of `revision` among catalog rows listed newest version first.
///
/// `exhausted` says the rows end where the catalog ends (no further page). The
/// version is complete when a lower version follows it in `rows` or the
/// catalog was exhausted; only then are the count and ordinal exact.
pub fn position(rows: &[Value], revision: &str, exhausted: bool) -> Option<Position> {
    let target = rows.iter().find(|row| revision_of(row) == Some(revision))?;
    let version = version_of(target)?;
    let members: Vec<&Value> = rows
        .iter()
        .filter(|row| version_of(row) == Some(version))
        .collect();
    // Stored ordinals: the version's newest row (the first one listed) holds
    // its count, whatever the walk has or has not reached below it.
    if let Some(ordinal) = stored_ordinal(target) {
        let count = members.iter().filter_map(|row| stored_ordinal(row)).max()?;
        return Some(Position {
            version,
            ordinal: Some(ordinal),
            count,
            complete: true,
        });
    }
    let complete = exhausted
        || rows
            .iter()
            .any(|row| version_of(row).is_some_and(|v| v < version));
    let ordinal = if complete {
        chronological(&members).and_then(|order| {
            order
                .iter()
                .position(|&index| revision_of(members[index]) == Some(revision))
                .map(|at| at as u64 + 1)
        })
    } else {
        None
    };
    Some(Position {
        version,
        ordinal,
        count: members.len() as u64,
        complete,
    })
}

/// Group one catalog page by version, newest version first and each
/// version's revisions oldest first, every row carried verbatim with its
/// `revision_ordinal_within_version`.
///
/// A page cuts at most the LAST group short of its first revision (the page
/// runs newest first). Its ordinals are then unknown and are reported as
/// null with `ordinals_exact: false`; the caller follows the cursor. Every
/// other group reaches its first revision on this page, so its ordinals are
/// exact even when newer revisions of it were on a previous page.
pub fn group_page(rows: &[Value], more: bool) -> Result<Vec<Value>, &'static str> {
    let mut versions: Vec<u64> = Vec::new();
    for row in rows {
        let version = version_of(row).ok_or("catalog revision has no positive version")?;
        match versions.last() {
            Some(last) if *last == version => {}
            Some(last) if *last < version => {
                return Err("catalog revisions are not listed newest version first")
            }
            _ => versions.push(version),
        }
    }
    let last = versions.last().copied();
    let mut groups = Vec::with_capacity(versions.len());
    for version in versions {
        let members: Vec<&Value> = rows
            .iter()
            .filter(|row| version_of(row) == Some(version))
            .collect();
        let stored: Option<Vec<u64>> = members.iter().map(|row| stored_ordinal(row)).collect();
        let (exact, revisions): (bool, Vec<Value>) = match stored {
            // The Server's own ordinals are exact on every page, a cut
            // group included; they only need save order.
            Some(ordinals) => {
                let mut indices: Vec<usize> = (0..members.len()).collect();
                indices.sort_by_key(|&index| ordinals[index]);
                (true, indices.iter().map(|&i| members[i].clone()).collect())
            }
            None => {
                let reaches_first = !(more && Some(version) == last);
                let order = chronological(&members);
                let exact = reaches_first && order.is_some();
                let indices = order.unwrap_or_else(|| (0..members.len()).rev().collect());
                let rows = indices
                    .iter()
                    .enumerate()
                    .map(|(at, &index)| {
                        let mut row = members[index].clone();
                        row["revision_ordinal_within_version"] = if exact {
                            json!(at as u64 + 1)
                        } else {
                            Value::Null
                        };
                        row
                    })
                    .collect();
                (exact, rows)
            }
        };
        groups.push(json!({
            "version": version,
            "revisions_listed": revisions.len(),
            "ordinals_exact": exact,
            "revisions": revisions,
        }));
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str, version: u64, parent: &str) -> Value {
        json!({"revision_id":id,"version":version,"parent_revision_id":parent})
    }
    /// v1: a <- b <- c ; v2 (bumped from c): d <- e. Listed as the catalog
    /// lists them: version desc, then document id desc (hash order).
    fn catalog() -> Vec<Value> {
        vec![
            row("rev-e", 2, "rev-d"),
            row("rev-d", 2, "rev-c"),
            row("rev-c", 1, "rev-b"),
            row("rev-b", 1, "rev-a"),
            row("rev-a", 1, ""),
        ]
    }
    #[test]
    fn ordinals_follow_the_parent_chain_not_the_hash_order() {
        let mut rows = catalog();
        // Hash order inside v1 puts the middle save first.
        rows.swap(2, 3);
        let at = position(&rows, "rev-c", true).unwrap();
        assert_eq!(
            at,
            Position {
                version: 1,
                ordinal: Some(3),
                count: 3,
                complete: true
            }
        );
        assert_eq!(position(&rows, "rev-d", true).unwrap().ordinal, Some(1));
        assert_eq!(position(&rows, "rev-e", true).unwrap().ordinal, Some(2));
    }
    #[test]
    fn an_unfinished_walk_claims_no_ordinal() {
        let rows = &catalog()[..2];
        // v2 has not been seen to end: rev-c (v1) is not among the rows.
        let at = position(rows, "rev-e", false).unwrap();
        assert!(!at.complete);
        assert_eq!(at.ordinal, None);
        assert_eq!(at.count, 2);
        // With the next version's first row the walk is complete.
        let at = position(&catalog()[..3], "rev-e", false).unwrap();
        assert_eq!(at.ordinal, Some(2));
        assert!(position(&catalog(), "rev-missing", true).is_none());
    }
    #[test]
    fn a_broken_chain_is_not_ordered_by_guess() {
        let rows = vec![row("rev-y", 1, "rev-x"), row("rev-z", 1, "rev-x")];
        let refs: Vec<&Value> = rows.iter().collect();
        assert!(chronological(&refs).is_none());
        assert_eq!(position(&rows, "rev-y", true).unwrap().ordinal, None);
    }
    #[test]
    fn pages_group_by_version_and_only_the_cut_group_is_inexact() {
        let groups = group_page(&catalog()[..4], true).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["version"], 2);
        assert_eq!(groups[0]["ordinals_exact"], true);
        assert_eq!(groups[0]["revisions"][0]["revision_id"], "rev-d");
        assert_eq!(
            groups[0]["revisions"][0]["revision_ordinal_within_version"],
            1
        );
        assert_eq!(
            groups[0]["revisions"][1]["revision_ordinal_within_version"],
            2
        );
        // v1 lost rev-a to the next page.
        assert_eq!(groups[1]["ordinals_exact"], false);
        assert!(groups[1]["revisions"][0]["revision_ordinal_within_version"].is_null());
        let whole = group_page(&catalog(), false).unwrap();
        assert_eq!(whole[1]["revisions"][2]["revision_id"], "rev-c");
        assert_eq!(
            whole[1]["revisions"][2]["revision_ordinal_within_version"],
            3
        );
        let mut unordered = catalog();
        unordered.swap(0, 4);
        assert!(group_page(&unordered, false).is_err());
    }
    #[test]
    fn a_servers_stored_ordinals_are_the_authority_even_on_a_cut_page() {
        let stored = |id: &str, version: u64, ordinal: u64| json!({"revision_id":id,"version":version,"revision_ordinal_within_version":ordinal});
        // Newest first by creation, as a Server with stored ordinals lists.
        let rows = vec![
            stored("rev-y", 3, 2),
            stored("rev-x", 3, 1),
            stored("rev-w", 2, 7),
        ];
        let at = position(&rows[..1], "rev-y", false).unwrap();
        assert_eq!((at.ordinal, at.count, at.complete), (Some(2), 2, true));
        let groups = group_page(&rows, true).unwrap();
        assert_eq!(groups[0]["revisions"][0]["revision_id"], "rev-x");
        // v2 was cut by the page, but its stored ordinal is still exact.
        assert_eq!(groups[1]["ordinals_exact"], true);
        assert_eq!(
            groups[1]["revisions"][0]["revision_ordinal_within_version"],
            7
        );
    }
}
