//! Selected account-private note operations, without project discovery.
use super::{
    now, profile, require_restore_before_context, restored_device_session, Client, Lane,
    NativeRefreshStore, NativeTransport,
};
use ds_cli_contract::{spec::Refusal, Failure};
use ds_client_core::personal_notes::Action;
use serde_json::{json, Value};

pub const OWN_REFUSALS: &[Refusal] = &[
    Refusal { code: "notes_request_invalid", when: "the selected note or checklist edit is outside the closed owner contract", remedy: "use a stable note id and the declared checklist edit fields and bounds" },
    Refusal { code: "notes_response_unreadable", when: "the owner receipt does not match the selected note and version", remedy: "retry only the exact original command; update the client and Brain together if the mismatch repeats" },
    Refusal { code: "notes_not_found", when: "the selected note is not held by this authenticated account", remedy: "restore the intended account and use its own note id" },
    Refusal { code: "notes_version_conflict", when: "the selected note moved after the reviewed version", remedy: "read that note checklist, review the change and submit a new command id against its version" },
    Refusal { code: "notes_command_conflict", when: "the command id already names a different intent", remedy: "retry the exact original command or use a new id for a new intent" },
    Refusal { code: "notes_not_permitted", when: "the notes owner refused this identity or selected item evidence", remedy: "restore the intended account; personal notes cannot adopt project model pins" },
    Refusal { code: "notes_unavailable", when: "the notes owner could not serve the selected operation", remedy: "retry later with the same command id, version and edit" },
];

pub fn execute(lane_value: &str, action: &Action) -> Result<Value, Failure> {
    action.validate().map_err(map)?;
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.personal_notes(action).map_err(map);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client.personal_notes(action, now()).map_err(map)
}

fn map(error: ds_client_core::ClientError) -> Failure {
    let message = error.to_string();
    let local = match message.as_str() {
        "notes_request_invalid" | "checklist_request_invalid" => Some("notes_request_invalid"),
        "notes_response_unreadable" => Some("notes_response_unreadable"),
        _ => None,
    };
    if let Some(code) = local {
        let refusal = OWN_REFUSALS.iter().find(|r| r.code == code).unwrap();
        return closed_failure(code, message).remedy(refusal.remedy);
    }
    if let Some(service) = error.service_refusal() {
        let code = match (service.status(), service.code()) {
            (_, Some("note_version_conflict")) => "notes_version_conflict",
            (_, Some("command_id_conflict")) => "notes_command_conflict",
            (401 | 403, _) => "notes_not_permitted",
            (404, _) => "notes_not_found",
            (400 | 413 | 422, _) => "notes_request_invalid",
            (409, _) => "notes_version_conflict",
            _ => "notes_unavailable",
        };
        let refusal = OWN_REFUSALS.iter().find(|r| r.code == code).unwrap();
        return closed_failure(code, message)
            .remedy(refusal.remedy)
            .detail(json!({"http_status":service.status(),"service_code":service.code()}));
    }
    super::map_client(error)
}

fn closed_failure(code: &str, message: String) -> Failure {
    match code {
        "notes_request_invalid" => Failure::invalid("notes_request_invalid", message),
        "notes_response_unreadable" => Failure::unavailable("notes_response_unreadable", message),
        "notes_not_found" => Failure::invalid("notes_not_found", message),
        "notes_version_conflict" => Failure::conflict("notes_version_conflict", message),
        "notes_command_conflict" => Failure::conflict("notes_command_conflict", message),
        "notes_not_permitted" => Failure::unauthorized("notes_not_permitted", message),
        _ => Failure::unavailable("notes_unavailable", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::ExitClass;

    #[test]
    fn private_note_refusals_match_the_closed_owner_vocabulary() {
        let expected = [
            ("notes_request_invalid", ExitClass::InvalidInput),
            ("notes_response_unreadable", ExitClass::Unavailable),
            ("notes_not_found", ExitClass::InvalidInput),
            ("notes_version_conflict", ExitClass::Conflict),
            ("notes_command_conflict", ExitClass::Conflict),
            ("notes_not_permitted", ExitClass::Unauthorized),
            ("notes_unavailable", ExitClass::Unavailable),
        ];
        assert_eq!(expected.len(), OWN_REFUSALS.len());
        for ((code, class), declared) in expected.iter().zip(OWN_REFUSALS) {
            assert_eq!(*code, declared.code);
            let failure = closed_failure(code, "owner refusal".into());
            assert_eq!(failure.code(), *code);
            assert_eq!(failure.class(), *class);
        }
        assert_eq!(
            closed_failure("unadmitted", String::new()).code(),
            "notes_unavailable"
        );
    }
}
