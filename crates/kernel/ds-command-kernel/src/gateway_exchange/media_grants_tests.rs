use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn grant_row(prefix: &str, expires_at: &str) -> Value {
    json!({"resolver_origin": "https://brain.example", "bucket": "media.example",
        "prefix": prefix, "token": "payload.signature", "expires_at": expires_at})
}

fn answered(body: Value) -> Result<MediaGrant, Refusal> {
    let Step::Fetch { continuation, .. } = start(MediaGrantFetch {
        project: "kigali".into(),
    }) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn the_project_tree_grant_is_chosen_among_what_brain_issues() {
    let issued = answered(
        json!({"grant": grant_row("projects/kigali/", "2027-04-01T00:00:00Z"),
        "grants": [grant_row("projects/kigali/", "2027-04-01T00:00:00Z"),
            grant_row("kigali/forms/", "2027-04-01T00:00:00Z")], "ttl_seconds": 1}),
    )
    .expect("the project grant admits");
    assert_eq!(issued.prefix, "projects/kigali/");
    let page = serde_json::to_value(&issued).unwrap();
    assert_eq!(page["resolverOrigin"], "https://brain.example");
    assert_eq!(page["expiresAt"], "2027-04-01T00:00:00Z");

    let refused =
        answered(json!({"grants": [grant_row("projects/other/", "2027-04-01T00:00:00Z")]}))
            .expect_err("another project's tree is refused");
    assert_eq!(refused.code, INVALID);
    let mut unusable = grant_row("projects/kigali/", "2027-04-01T00:00:00Z");
    unusable["token"] = "no-signature".into();
    assert!(answered(json!({"grants": [unusable]})).is_err());
}

#[cfg(feature = "durable-store")]
mod held_grants {
    use super::super::{grant, held};
    use super::grant_row;
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    use serde_json::json;

    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    #[test]
    fn a_held_grant_serves_offline_until_it_lapses_and_renews_a_day_before() {
        let edge = TestEdge::new();
        let now = edge.now.get();
        let at = |ms: u64| {
            time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
                .unwrap()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap()
        };
        edge.json(json!({"grants": [grant_row("projects/kigali/", &at(now + 30 * DAY_MS))]}));
        let first = block_on(grant(&edge, "kigali")).expect("a grant is issued");
        let again = block_on(grant(&edge, "kigali")).expect("the held grant serves");
        assert_eq!(first, again);
        assert_eq!(edge.requests().len(), 1);

        edge.online.set(false);
        assert_eq!(
            block_on(held(&edge, "kigali")).unwrap(),
            Some(first.clone())
        );
        edge.now.set(now + 29 * DAY_MS + DAY_MS / 2);
        assert!(block_on(grant(&edge, "kigali")).is_err());
        assert_eq!(block_on(held(&edge, "kigali")).unwrap(), Some(first));
        edge.now.set(now + 31 * DAY_MS);
        assert_eq!(block_on(held(&edge, "kigali")).unwrap(), None);
    }
}
