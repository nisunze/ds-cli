use super::*;
use serde_json::json;

const URL: &str = "https://storage.googleapis.com/b/p/project-work/task/t/att/plan.pdf?generation=42&X-Goog-Signature=test";

/// Answers the attachment grant, then the signed read, recording both.
struct Grant {
    grant: Value,
    bytes: Vec<u8>,
    calls: Vec<String>,
}
macro_rules! no_other_call {
    ($name:ident, $call:ident) => {
        fn $name(
            &mut self,
            _: crate::$call<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            panic!("a projected attachment read made an unrelated call")
        }
    };
}
impl AuthorizedTransport for Grant {
    no_other_call!(list_projects, ProjectListCall);
    no_other_call!(transformer_context, TransformerContextCall);
    no_other_call!(project_forms, ProjectFormsCall);
    no_other_call!(project_form_editor, ProjectFormEditorCall);
    no_other_call!(solar_snapshot, SolarSnapshotCall);
    no_other_call!(survey_query, SurveyQueryCall);
    no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
    no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
    no_other_call!(survey_entry_create, SurveyEntryCreateCall);
    no_other_call!(project_data, ProjectDataCall);
    no_other_call!(survey_control, SurveyControlCall);
    no_other_call!(styles, StylesCall);
    no_other_call!(layers, LayersCall);
    no_other_call!(tiles, TileCall);
    no_other_call!(project_report, ProjectReportCall);
    fn upload_bytes(
        &mut self,
        _: crate::UploadBytesCall<'_>,
    ) -> Result<crate::TransportResponse, crate::TransportError> {
        panic!("a projected attachment read never uploads")
    }
    fn project_management(
        &mut self,
        call: ProjectManagementCall<'_>,
    ) -> Result<crate::TransportResponse, crate::TransportError> {
        self.calls.push(call.payload.to_string());
        Ok(crate::TransportResponse::new(
            200,
            serde_json::to_vec(&self.grant).unwrap(),
        ))
    }
    fn shared_asset_bytes(
        &mut self,
        call: DownloadCall<'_>,
    ) -> Result<crate::TransportResponse, crate::TransportError> {
        self.calls.push(call.url().to_owned());
        assert!(call.response_limit() >= self.bytes.len());
        Ok(crate::TransportResponse::new(200, self.bytes.clone()))
    }
}

fn grant(size: u64) -> Grant {
    Grant {
        grant: json!({"success": true, "data": {"url": URL, "expires_at": "2026-10-20T09:00:00Z",
            "source": {"project_id": "project-a", "attachment_id": "att", "version": 3,
                "generation": 42, "size_bytes": size, "content_type": "application/pdf",
                "original_name": "plan.pdf"}}}),
        bytes: b"%PDF".to_vec(),
        calls: Vec::new(),
    }
}

fn read(transport: &mut Grant, id: &str) -> Result<(Value, Vec<u8>), ClientError> {
    super::super::read_bytes(
        &crate::data_distribution::tests::profile_for_tests(),
        transport,
        Authority {
            bearer_token: "bearer",
            uid: "uid",
            email: "user@example.com",
            device_id: None,
        },
        "project-a",
        &format!("{PREFIX}{id}"),
    )
}

#[test]
fn a_projected_attachment_is_read_through_its_generation_pinned_grant() {
    let mut transport = grant(4);
    let (row, bytes) = read(&mut transport, "att").expect("read");
    assert_eq!(bytes, b"%PDF");
    assert_eq!(
        transport.calls[0],
        json!({"action": "download_attachment_url", "ds_project": "project-a", "id": "att"})
            .to_string()
    );
    assert_eq!(transport.calls[1], URL);
    assert_eq!(row["asset_id"], "sys:pm_attachment:att");
    assert_eq!(row["name"], "plan.pdf");
    assert_eq!(row["generation"], 42);
    assert_eq!(
        row["digest"],
        format!("sha256:{:x}", Sha256::digest(b"%PDF"))
    );
}

#[test]
fn a_foreign_unpinned_resized_or_mismatched_grant_is_refused() {
    let mut other_project = grant(4);
    other_project.grant["data"]["source"]["project_id"] = json!("project-b");
    let mut other_generation = grant(4);
    other_generation.grant["data"]["url"] = json!(URL.replace("generation=42", "generation=41"));
    let mut unpinned = grant(4);
    unpinned.grant["data"]["url"] = json!(URL.replace("generation=42&", ""));
    let mut foreign_host = grant(4);
    foreign_host.grant["data"]["url"] = json!(URL.replace("storage.googleapis.com", "example.com"));
    let mut short = grant(5);
    let mut advertised = grant(4);
    advertised.grant["data"]["source"]["advertised_sha256"] = json!("0".repeat(64));
    for transport in [
        &mut other_project,
        &mut other_generation,
        &mut unpinned,
        &mut foreign_host,
        &mut short,
        &mut advertised,
    ] {
        assert!(read(transport, "att").is_err());
    }
    for transport in [&other_project, &other_generation, &unpinned, &foreign_host] {
        assert_eq!(
            transport.calls.len(),
            1,
            "no byte is fetched for a bad grant"
        );
    }
    let mut large = grant(MAX_READ_BYTES + 1);
    assert_eq!(
        read(&mut large, "att").unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    let mut none = grant(4);
    assert_eq!(
        read(&mut none, "a/b").unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert!(none.calls.is_empty());
}
