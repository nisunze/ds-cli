//! Versions and revisions of one project DS Grid model, as the catalog lists them.
//!
//! A model has versions v1, v2, …; every save is a revision of the version
//! that was current when it was saved, and only an explicit bump starts the
//! next version. The catalog lists revisions newest version first. Within
//! each version, save order comes from the Server's positive
//! `revision_ordinal_within_version` on every row.
//!
//! The Server is the authority for ordinals, including older stored rows;
//! missing or invalid ordinals are refused.
//!
//! Hosts page the catalog and hand the rows here; everything that decides an
//! ordinal, a count or whether either is exact stays in this one place.
use serde_json::{json, Value};

fn version_of(row: &Value) -> Option<u64> {
    row["version"].as_u64().filter(|v| *v > 0)
}

/// The ordinal the Server stored on a row, when it stores one.
pub fn stored_ordinal(row: &Value) -> Option<u64> {
    row["revision_ordinal_within_version"]
        .as_u64()
        .filter(|n| *n > 0)
}

/// Group one catalog page by version, newest version first and each
/// version's revisions oldest first, every row carried verbatim with its
/// `revision_ordinal_within_version`.
///
/// Stored ordinals remain exact when a page cuts a version short.
pub fn group_page(rows: &[Value]) -> Result<Vec<Value>, &'static str> {
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
    let mut groups = Vec::with_capacity(versions.len());
    for version in versions {
        let members: Vec<&Value> = rows
            .iter()
            .filter(|row| version_of(row) == Some(version))
            .collect();
        let ordinals: Vec<u64> = members
            .iter()
            .map(|row| stored_ordinal(row).ok_or("catalog revision has no positive stored ordinal"))
            .collect::<Result<_, _>>()?;
        let mut indices: Vec<usize> = (0..members.len()).collect();
        indices.sort_by_key(|&index| ordinals[index]);
        let revisions: Vec<Value> = indices.iter().map(|&i| members[i].clone()).collect();
        groups.push(json!({
            "version": version,
            "revisions_listed": revisions.len(),
            "ordinals_exact": true,
            "revisions": revisions,
        }));
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_ordinals_order_every_group_including_a_cut_page() {
        let stored = |id: &str, version: u64, ordinal: u64| json!({"revision_id":id,"version":version,"revision_ordinal_within_version":ordinal});
        let rows = vec![
            stored("rev-y", 3, 2),
            stored("rev-x", 3, 1),
            stored("rev-w", 2, 7),
        ];
        let groups = group_page(&rows).unwrap();
        assert_eq!(groups[0]["revisions"][0]["revision_id"], "rev-x");
        assert_eq!(groups[1]["ordinals_exact"], true);
        assert_eq!(
            groups[1]["revisions"][0]["revision_ordinal_within_version"],
            7
        );
        assert!(group_page(&[rows[2].clone(), rows[0].clone()]).is_err());
    }

    #[test]
    fn missing_or_invalid_ordinals_are_refused() {
        for ordinal in [Value::Null, json!(0), json!(-1), json!("1")] {
            let row =
                json!({"revision_id":"r","version":1,"revision_ordinal_within_version":ordinal});
            assert!(group_page(&[row]).is_err());
        }
    }
}
