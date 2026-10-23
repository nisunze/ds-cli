use super::*;

// Golden requests use the pre-migration constructor and literal wire values.
// Compare full serialized requests, including omitted fields and all controls.
fn unchanged<C: Exchange>(call: C, expected: Request, retries: u32) {
    let actual = call.plan().unwrap();
    assert_eq!(
        serde_json::to_vec(&actual.request).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert_eq!(actual.retries, retries);
}

fn get(path: &str) -> Request {
    Request {
        method: Method::Get,
        body: None,
        ..Request::post(path, Value::Null)
    }
}

#[test]
fn account_get_put_and_post_plans_keep_their_complete_wire_bytes() {
    unchanged(
        account::IdentityRepair {
            uid: "uid-a".into(),
            email: "user@example.com".into(),
        },
        get("/api/v1/user/info").timeout_ms(15_000),
        1,
    );
    unchanged(account::Profile, get("/api/v1/user/profile"), 0);
    let fields = json!({"display_name":"Example"})
        .as_object()
        .unwrap()
        .clone();
    unchanged(
        account::ProfileWrite {
            fields: fields.clone(),
        },
        Request {
            method: Method::Put,
            ..Request::post("/api/v1/user/profile", Value::Object(fields))
        },
        0,
    );
    unchanged(
        account::Capabilities {
            project: Some("project-a".into()),
        },
        Request::post(
            "/api/v1/user/capabilities",
            json!({"project_id":"project-a"}),
        )
        .timeout_ms(120_000),
        0,
    );
    unchanged(
        sre::Overview,
        get("/api/v1/sre/overview").timeout_ms(60_000),
        0,
    );
}

#[test]
fn closed_processing_and_geodata_selectors_keep_bodies_and_defaults() {
    for (domain, path) in [("mv_extrapolation", "/mv"), ("sketch_lv", "/process")] {
        unchanged(
            uploads::ProcessingStart {
                project: "project-a".into(),
                domain: domain.into(),
                file_name: "input.xlsx".into(),
                size: 42,
                content_type: "application/octet-stream".into(),
            },
            Request::post(
                path,
                json!({"action":"upload_start","eds_project_id":"project-a","upload_domain":domain,"file_name":"input.xlsx","size":42,"content_type":"application/octet-stream"}),
            ),
            0,
        );
    }
    unchanged(
        geodata::GeodataList { project: None },
        Request::post("/api/v1/user_data", json!({"action":"list"})),
        0,
    );
    unchanged(
        geodata::GeodataList {
            project: Some("project-a".into()),
        },
        Request::post(
            "/api/v1/project_data",
            json!({"action":"list","project_id":"project-a"}),
        ),
        0,
    );
}

#[test]
fn all_design_collaboration_selectors_keep_caps_deadlines_and_retry_budgets() {
    use design_collab::Surface;
    for (surface, action, path, timeout, cap, retries) in [
        (
            Surface::Selections,
            "list",
            "/api/v1/design/selections",
            30_000,
            8 * 1024 * 1024,
            1,
        ),
        (
            Surface::Attachments,
            "list",
            "/api/v1/design/attachments",
            30_000,
            8 * 1024 * 1024,
            1,
        ),
        (
            Surface::Annotations,
            "set_tags",
            "/api/v1/design/annotations",
            30_000,
            8 * 1024 * 1024,
            0,
        ),
        (
            Surface::Versions,
            "list_versions",
            "/api/v1/design/versions",
            30_000,
            8 * 1024 * 1024,
            1,
        ),
        (
            Surface::Versions,
            "get_transformer_version_revision",
            "/api/v1/data",
            120_000,
            64 * 1024 * 1024,
            1,
        ),
        (
            Surface::Versions,
            "import_transformer_version",
            "/api/v1/data",
            120_000,
            64 * 1024 * 1024,
            0,
        ),
        (
            Surface::Versions,
            "publish_transformer_version_revision",
            "/api/v1/data",
            120_000,
            64 * 1024 * 1024,
            0,
        ),
        (
            Surface::Versions,
            "promote_transformer_version_revision",
            "/api/v1/data",
            120_000,
            64 * 1024 * 1024,
            0,
        ),
    ] {
        let body = json!({"action":action,"project_id":"project-a"});
        let mut request = body.as_object().unwrap().clone();
        request.insert("discarded_null".into(), Value::Null);
        unchanged(
            design_collab::DesignCollab { surface, request },
            Request::post(path, body)
                .timeout_ms(timeout)
                .response_cap(cap),
            retries,
        );
    }
}

#[test]
fn pipeline_reads_and_writes_keep_distinct_bodies_bounds_and_retries() {
    unchanged(
        pipeline::PipelineCall::Status {
            project: "project-a".into(),
        },
        Request::post(
            "/api/v1/pipeline",
            json!({"action":"pipeline_status","eds_project_id":"project-a"}),
        )
        .timeout_ms(60_000)
        .response_cap(8 * 1024 * 1024),
        2,
    );
    unchanged(
        pipeline::PipelineCall::TileList {
            project: "project-a".into(),
            include_global: Some(true),
        },
        Request::post(
            "/api/v1/tiles",
            json!({"include_global":true,"action":"list","eds_project_id":"project-a"}),
        )
        .timeout_ms(60_000)
        .response_cap(8 * 1024 * 1024),
        2,
    );
    unchanged(pipeline::PipelineCall::Migrate {
        project: "project-a".into(), target_project: Some("project-b".into()), dry_run: true, delete_source: None, delete_only: None, overwrite: None, filter: None,
    }, Request::post("/api/v1/pipeline", json!({"dry_run":true,"target_project_id":"project-b","action":"migrate","eds_project_id":"project-a"})).timeout_ms(600_000).response_cap(8 * 1024 * 1024), 0);
}

#[test]
fn messaging_keeps_query_order_minimal_headers_and_the_exact_body() {
    for (endpoint, action, path) in [
        (
            messaging::MessagingEndpoint::Commands,
            "config",
            "/api/v1/messaging",
        ),
        (
            messaging::MessagingEndpoint::Attachments,
            "download",
            "/api/v1/messaging/attachments",
        ),
    ] {
        let body = json!({"action":action});
        let mut payload = body.as_object().unwrap().clone();
        payload.insert("discarded_null".into(), Value::Null);
        let expected = Request {
            query: vec![("wire_contract".into(), "messages-v1".into())],
            ..Request::post(path, body)
        }
        .timeout_ms(12_345)
        .response_cap(2 * 1024 * 1024)
        .minimal_headers();
        unchanged(
            messaging::Messaging {
                endpoint,
                payload,
                timeout_ms: Some(12_345),
            },
            expected,
            0,
        );
    }
}

#[test]
fn generated_config_encoding_and_survey_stream_keep_the_old_wire_controls() {
    unchanged(
        design::DesignCall::ProjectConfig {
            project: "électricité@RW".into(),
        },
        get("/config/%C3%A9lectricit%C3%A9%40RW")
            .timeout_ms(180_000)
            .response_cap(32 * 1024 * 1024),
        2,
    );
    let body = json!({"action":"query_entries","eds_project_id":"project-a"});
    unchanged(
        survey::Query {
            project: "project-a".into(),
            body: body.clone(),
        },
        Request::post("/api/v1/data", body)
            .timeout_ms(900_000)
            .response_cap(256 * 1024 * 1024)
            .ndjson(1_000_000),
        0,
    );
}

#[test]
fn invalid_generated_parameters_refuse_and_domain_validation_still_precedes_planning() {
    for parameters in [
        vec![],
        vec![("other", "project-a")],
        vec![("eds_project_id", ".")],
        vec![("eds_project_id", "project\na")],
        vec![("eds_project_id", "p"), ("eds_project_id", "q")],
    ] {
        assert_eq!(
            Request::generated_parameters(Operation::DsSystemGetConfig, &parameters, None)
                .unwrap_err()
                .code,
            "gateway_route_invalid"
        );
    }
    for project in ["..", "project/other", "project\na"] {
        assert_eq!(
            design::DesignCall::ProjectConfig {
                project: project.into()
            }
            .plan()
            .unwrap_err()
            .code,
            PROJECT_INVALID
        );
    }
    assert_eq!(
        messaging::Messaging {
            endpoint: messaging::MessagingEndpoint::Commands,
            payload: json!({"action":"caller_override"})
                .as_object()
                .unwrap()
                .clone(),
            timeout_ms: None
        }
        .plan()
        .unwrap_err()
        .code,
        messaging::INPUT_INVALID
    );
}
