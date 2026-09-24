//! The correspondence door (ds-brain `docs/contracts/correspondence.md`):
//! parties, records, threads and task blockers on `POST /api/v1/pm`, for only
//! the saved, audience-fenced selected project — and the one rule for how a
//! named refusal from that door, or from the assets catalogue, becomes the
//! failure a `ds pm` or `ds assets` command documents.
//!
//! ## The refusal rule
//!
//! Both routes refuse BY NAME: `PM_REFUSED` / `ASSET_REFUSED` carry
//! `details.reason` set to a token the contract lists —
//! `record_source_required`, `party_exists`, `asset_bytes_not_held`, … — and
//! the ids or numbers the rule names (`record_id`, `bound`, `external_url`).
//! [`map_named_refusal`] relays that token AS THE CODE, so a caller plans
//! against the contract's own vocabulary rather than a paraphrase, and puts
//! every named detail under `detail`. The token is not chosen here: the
//! closed list, its `when` and its remedy live with the commands that
//! declare it (`ds-cli-pm::CORRESPONDENCE_REFUSALS`, the assets domain's
//! own), and each domain's classifier renames a token it does not declare to
//! its generic refusal — so a token the server gains tomorrow can never ship
//! undocumented through here. The two concurrency envelopes keep `ds pm`'s
//! names: `PM_REVISION_CONFLICT` and `PM_CONTEXT_VERSION_CONFLICT` are both
//! `work_revision_conflict` (re-read, decide again); a 401/403 is
//! `work_not_permitted`; a 404 with no token is `project_not_visible`.

use ds_cli_contract::Failure;
use ds_client_core::{ErrorKind, ServiceRefusal, ServiceRefusalDetail};
use serde_json::{Map, Value, json};

use super::{HeadlessNamedProject, headless_named_project, map_client_kind, now};

/// One correspondence action for the project named on this request.
pub fn project_correspondence_for_project(
    lane_value: &str,
    project: &str,
    action: &ds_client_core::project_correspondence::Action,
) -> Result<HeadlessNamedProject<Value>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.project_correspondence(project, action),
        |client, project| client.project_correspondence(project, action, now()),
    )
}

/// The named details a route carried, as a JSON object a receipt can show.
fn details_of(refusal: &ServiceRefusal) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in refusal.details() {
        let value = match value {
            ServiceRefusalDetail::Text(text) => json!(text),
            ServiceRefusalDetail::Integer(integer) => json!(integer),
            ServiceRefusalDetail::Flag(flag) => json!(flag),
        };
        out.insert(key.clone(), value);
    }
    out
}

/// One named refusal of the correspondence door or the assets catalogue, as
/// the failure the calling domain will classify. See the module notes.
pub fn map_named_refusal(kind: ErrorKind, refusal: &ServiceRefusal, message: String) -> Failure {
    let mut detail = details_of(refusal);
    detail.insert("http_status".into(), json!(refusal.status()));
    detail.insert("service_code".into(), json!(refusal.code()));
    detail.insert("service_message".into(), json!(refusal.message()));
    let detail = Value::Object(detail);
    let sentence = refusal
        .message()
        .map(str::to_owned)
        .unwrap_or_else(|| message.clone());
    match (refusal.status(), refusal.code()) {
        (409, Some("pm_revision_conflict" | "pm_context_version_conflict")) => {
            Failure::conflict(
                "work_revision_conflict",
                "the record, party or plan moved while the command was in flight",
            )
            .detail(detail)
            .remedy("re-read it and issue the command again")
        }
        (401 | 403, _) => Failure::unauthorized("work_not_permitted", sentence)
            .detail(detail)
            .remedy("ask a project admin for the access the message names")
            .next("ds pm plan"),
        (404, None | Some("not_found")) => Failure::unauthorized(
            "project_not_visible",
            "the selected project is not a project this account is a member of, or what the request named is not in it",
        )
        .detail(detail)
        .remedy("choose an exact id from auth project list, or check the id you named")
        .next("ds auth project list"),
        // The contract's own token, relayed as the code. The domain that
        // documents it attaches the remedy; one it does not document is
        // renamed there.
        (400 | 404 | 409 | 422, Some(token)) => {
            let failure = if refusal.status() == 409 {
                Failure::conflict(token, sentence)
            } else {
                Failure::invalid(token, sentence)
            };
            failure.detail(detail)
        }
        (400 | 409 | 422, None) => Failure::invalid("pm_refused", sentence)
            .detail(detail)
            .remedy("read detail.service_message; correct the flag it names and retry"),
        _ => map_client_kind(kind, message.clone())
            .with_message(message)
            .detail(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(status: u16, code: Option<&str>, details: Value) -> ServiceRefusal {
        ServiceRefusal::with_details(status, code, Some("the rule in words"), details.as_object())
    }

    #[test]
    fn a_contract_token_is_the_code_and_its_named_ids_are_the_detail() {
        let failure = map_named_refusal(
            ErrorKind::InvalidInput,
            &refusal(
                400,
                Some("record_not_awaiting"),
                json!({"reason": "record_not_awaiting", "record_id": "R5", "response_status": "none"}),
            ),
            "correspondence refused this request (HTTP 400): the rule in words".into(),
        );
        assert_eq!(failure.code(), "record_not_awaiting");
        let detail = failure.detail_value().expect("detail");
        assert_eq!(detail["record_id"], "R5");
        assert_eq!(detail["response_status"], "none");
        assert_eq!(detail["http_status"], 400);
        assert_eq!(detail["service_code"], "record_not_awaiting");

        let exists = map_named_refusal(
            ErrorKind::InvalidInput,
            &refusal(409, Some("party_exists"), json!({"party_id": "p_1"})),
            "correspondence".into(),
        );
        assert_eq!(exists.code(), "party_exists");
        assert_eq!(exists.detail_value().unwrap()["party_id"], "p_1");
    }

    #[test]
    fn the_concurrency_and_permission_envelopes_keep_the_pm_names() {
        for code in ["pm_revision_conflict", "pm_context_version_conflict"] {
            let failure = map_named_refusal(
                ErrorKind::InvalidInput,
                &refusal(409, Some(code), json!({"current_revision": 9})),
                "correspondence".into(),
            );
            assert_eq!(failure.code(), "work_revision_conflict", "{code}");
            assert_eq!(failure.detail_value().unwrap()["current_revision"], 9);
        }
        let forbidden = map_named_refusal(
            ErrorKind::AuthenticationRejected,
            &refusal(
                403,
                Some("asset_class_forbidden"),
                json!({"sensitivity": "restricted", "capability": "assets.read.commercial"}),
            ),
            "correspondence".into(),
        );
        assert_eq!(forbidden.code(), "work_not_permitted");
        assert_eq!(
            forbidden.detail_value().unwrap()["capability"],
            "assets.read.commercial"
        );
        let missing = map_named_refusal(
            ErrorKind::ResourceNotFound,
            &refusal(404, Some("not_found"), json!({})),
            "correspondence".into(),
        );
        assert_eq!(missing.code(), "project_not_visible");
        let named_missing = map_named_refusal(
            ErrorKind::ResourceNotFound,
            &refusal(404, Some("record_not_found"), json!({"record_id": "R9"})),
            "correspondence".into(),
        );
        assert_eq!(named_missing.code(), "record_not_found");
        let generic = map_named_refusal(
            ErrorKind::InvalidInput,
            &refusal(400, Some("validation_failed"), json!({})),
            "correspondence".into(),
        );
        assert_eq!(
            generic.code(),
            "validation_failed",
            "a non-token envelope code still crosses; the domain renames what it does not declare"
        );
    }
}
