//! One fixed existing PM door for the closed shared note owner.
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
pub use ds_command_kernel::project_management::project_notes::Command;
use serde_json::Value;
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
    authoring_model: Option<
        &ds_command_kernel::project_management::authoring_model::AuthoringModel,
    >,
) -> Result<Value, ClientError> {
    let mut payload = command.payload(project).map_err(|_| {
        ClientError::new(ErrorKind::InvalidInput, "Project Notes request is invalid")
    })?;
    if let Some(model) = authoring_model {
        payload["authoring_model"] =
            serde_json::to_value(model).expect("validated authoring declaration");
    }
    let value = super::send(profile, transport, authority, payload)?;
    command.verify_response(project,&value).map_err(|_|ClientError::new(ErrorKind::UnreadableResponse,"Project Notes response differs from its captured project, identities or complete receipt"))?;
    Ok(value)
}

#[cfg(test)]
mod epoch_tests {
    use super::*;
    use serde_json::json;

    struct NoteTransport { response: Value, calls: usize }
    macro_rules! unrelated {
        ($method:ident, $call:ident) => {
            fn $method(&mut self, _: crate::$call<'_>) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("note query must not acquire unrelated data")
            }
        };
    }
    impl AuthorizedTransport for NoteTransport {
        unrelated!(list_projects, ProjectListCall);
        unrelated!(transformer_context, TransformerContextCall);
        unrelated!(project_forms, ProjectFormsCall);
        unrelated!(project_form_editor, ProjectFormEditorCall);
        unrelated!(solar_snapshot, SolarSnapshotCall);
        unrelated!(survey_query, SurveyQueryCall);
        unrelated!(survey_entries_select, SurveyEntriesSelectCall);
        unrelated!(survey_entries_changes, SurveyEntriesChangesCall);
        unrelated!(survey_entry_create, SurveyEntryCreateCall);
        unrelated!(project_data, ProjectDataCall);
        unrelated!(upload_bytes, UploadBytesCall);
        unrelated!(survey_control, SurveyControlCall);
        unrelated!(styles, StylesCall);
        unrelated!(layers, LayersCall);
        unrelated!(tiles, TileCall);
        unrelated!(project_report, ProjectReportCall);
        fn project_management(&mut self, call: crate::ProjectManagementCall<'_>) -> Result<crate::TransportResponse, crate::TransportError> {
            self.calls += 1;
            assert_eq!(call.authority.canonical_uid, "uid");
            assert_eq!(call.payload["ds_project"], "p");
            assert_eq!(call.payload["action"], "project_note_query");
            assert_eq!(call.payload["project_note"]["limit"], 5);
            Ok(crate::TransportResponse::new(200, serde_json::to_vec(&json!({"success":true,"data":self.response})).unwrap()))
        }
    }
    #[test]
    fn server_numeric_note_epoch_passes_fixed_transport_without_relaxing_page_admission() {
        let command = Command::Query(serde_json::from_value(json!({"tag_match":"any","sort":"updated","direction":"desc","limit":5})).unwrap());
        let page = json!({"project_id":"p","items":[],"scanned":0,"truncated":false,"complete":true,"migration_required":[],"context_revision":0});
        for (field, value, accepted) in [
            ("context_revision",json!(0),true), ("context_revision",json!(17),true),
            ("context_revision",json!("a".repeat(64)),false), ("context_revision",json!(-1),false),
            ("project_id",json!("foreign"),false), ("truncated",json!(true),false),
            ("items",json!(null),false), ("scanned",json!(201),false),
        ] {
            let mut response=page.clone(); response[field]=value;
            let mut transport=NoteTransport { response:response.clone(), calls:0 };
            let result=run(&crate::data_distribution::tests::profile_for_tests(), &mut transport,
                Authority { bearer_token:"bearer", uid:"uid", email:"user@example.com", device_id:None }, "p", &command, None);
            assert_eq!(transport.calls,1);
            if accepted { assert_eq!(result.unwrap(),response); }
            else { assert_eq!(result.unwrap_err().kind(),ErrorKind::UnreadableResponse); }
        }
    }
}
