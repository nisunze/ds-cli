//! Selected account-private note operations, without project discovery.
pub mod backup_export;
use super::{
    Client, Lane, NativeRefreshStore, NativeTransport, now, profile,
    require_restore_before_context, restored_device_session,
};
use ds_cli_contract::{Failure, spec::Refusal};
use ds_client_core::personal_notes::Action;
use serde_json::{Value, json};

pub const OWN_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "notes_migration_required",
        when: "the selected private note has an old stored shape or no retained current baseline",
        remedy: "use notes canonicalize prepare, review its exact source-pinned plan and apply it explicitly; missing or corrupt retained history may require a verified backup restore",
    },
    Refusal {
        code: "notes_request_invalid",
        when: "the selected note, history version, restoration hash or checklist edit is outside the closed owner contract",
        remedy: "use one stable private note id and the declared version, source hash or checklist edit fields",
    },
    Refusal {
        code: "notes_response_unreadable",
        when: "the owner receipt does not match the selected note and version",
        remedy: "after an uncertain create inspect notes list, or read the selected note after an uncertain update; retry reads or declared replay-safe controls with exact intent; update client and Brain together if mismatches repeat",
    },
    Refusal {
        code: "notes_not_found",
        when: "the selected private note or historical revision is unavailable to this account",
        remedy: "restore the intended account, select its own note id and inspect notes versions; unavailable history cannot be reconstructed",
    },
    Refusal {
        code: "notes_version_conflict",
        when: "the selected note moved after the reviewed version",
        remedy: "read that private note, review the change and submit the declared operation against its current version",
    },
    Refusal {
        code: "notes_command_conflict",
        when: "the command id already names a different intent",
        remedy: "retry the exact original command or use a new id for a new intent",
    },
    Refusal {
        code: "notes_not_permitted",
        when: "the notes owner refused this identity or selected item evidence",
        remedy: "restore the intended account; personal notes cannot adopt project model pins",
    },
    Refusal {
        code: "notes_unavailable",
        when: "the notes owner could not serve the selected operation",
        remedy: "retry reads later; after uncertain writes inspect current state before resubmitting, and retain exact intent only for declared replay-safe controls",
    },
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
            (_, Some("personal_note_migration_required")) => "notes_migration_required",
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
        "notes_migration_required" => Failure::conflict("notes_migration_required", message),
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
            ("notes_migration_required", ExitClass::Conflict),
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
