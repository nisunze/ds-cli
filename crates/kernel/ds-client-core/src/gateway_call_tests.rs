use super::*;
use crate::profile::{DeploymentLane, ProfileParts};
use crate::transport::{CallAuthority, KnownColumnsCall, ProjectFormsCall, StatusProcessingCall};
use serde_json::json;

fn profile() -> crate::ClientProfile {
    crate::ClientProfile::from_parts(ProfileParts {
        lane: DeploymentLane::Canary,
        source_revision: "0123456789abcdef0123456789abcdef01234567",
        descriptor_sha256: &"a".repeat(64),
        firebase_project_id: "firebase-project",
        firebase_api_key: "public-firebase-key",
        gateway_api_key: "public-gateway-key",
        gateway_origin: "https://eds-gateway-canary-3c0q477h.ue.gateway.dev",
    })
    .unwrap()
}

fn authority() -> CallAuthority<'static> {
    CallAuthority {
        bearer_token: "fixture-memory-bearer",
        canonical_uid: "uid-a",
        canonical_email: "user@example.com",
        device_id: Some("device-a"),
    }
}

#[test]
fn capture_keeps_the_typed_project_body_identity_deadline_and_generated_route() {
    let profile = profile();
    let call = ProjectFormsCall {
        profile: &profile,
        ds_project: "project-a",
        authority: authority(),
    };
    let request = call.capture_gateway();
    assert_eq!(request.operation(), Some(Operation::ProjectForms));
    assert_eq!(request.path(), "/api/v1/project-forms");
    assert_eq!(request.method(), "POST");
    assert_eq!(
        request.body(),
        br#"{"action":"activate","project_id":"project-a"}"#
    );
    assert_eq!(request.bearer_token(), "fixture-memory-bearer");
    assert_eq!(request.canonical_email(), "user@example.com");
    assert_eq!(
        (request.timeout_seconds(), request.response_limit()),
        (120, 32 * 1024 * 1024)
    );
    assert_eq!(request.send(), GatewaySend::Bytes);
    assert_eq!(request.processing_lane(), None);
}

#[test]
fn dynamic_get_is_explicit_empty_bytes_and_missing_swagger_route_stays_closed() {
    let profile = profile();
    let call = KnownColumnsCall {
        profile: &profile,
        project: "project-a",
        patch: None,
        authority: authority(),
    };
    let request = call.capture_gateway();
    assert_eq!(request.operation(), None);
    assert_eq!(request.path(), "/config/project-a/known-columns");
    assert_eq!(request.method(), "GET");
    assert!(request.body().is_empty());
    assert_eq!(request.send(), GatewaySend::Bytes);
    assert_eq!(
        (request.timeout_seconds(), request.response_limit()),
        (60, 4 * 1024 * 1024)
    );
}

#[test]
fn the_only_fixed_processing_lane_and_ndjson_accept_survive_capture() {
    let profile = profile();
    let payload = json!({"action":"status","eds_project_id":"project-a"});
    let processing = StatusProcessingCall {
        profile: &profile,
        payload: &payload,
        authority: authority(),
    };
    let request = processing.capture_gateway();
    assert_eq!(request.operation(), Some(Operation::DsSystemProcess));
    assert_eq!(request.processing_lane(), Some("standard"));
    let events = crate::SreEventsCall {
        profile: &profile,
        payload: json!({"action":"query_table"}),
        authority: authority(),
    };
    let request = events.capture_gateway();
    assert_eq!(request.accept(), "application/x-ndjson");
    assert_eq!(request.content_type(), "application/json");
    assert_eq!(request.processing_lane(), None);
}

#[test]
fn ordinary_get_keeps_the_exact_query_and_does_not_turn_into_an_empty_byte_send() {
    let profile = profile();
    let call = crate::ProjectListCall {
        profile: &profile,
        status: crate::ProjectStatus::Archived,
        authority: authority(),
    };
    let request = call.capture_gateway();
    assert_eq!(
        request.url(),
        format!(
            "{}/api/v1/user/projects?status=archived",
            profile.gateway_origin()
        )
    );
    assert_eq!(request.send(), GatewaySend::Call);
    assert!(request.body().is_empty());
    assert_eq!(request.timeout_seconds(), 30);
}

#[test]
fn closed_asset_and_version_actions_select_generated_bulk_or_run_routes() {
    let profile = profile();
    for (action, operation) in [
        ("list", Operation::ProjectAssetsAction),
        ("finalize_asset_semantic", Operation::DataInterop),
    ] {
        let call = crate::SharedAssetsCall {
            profile: &profile,
            payload: json!({"action":action,"project_id":"project-a"}),
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), Some(operation));
        assert_eq!(captured.path(), operation.path_template());
        assert_eq!(captured.method(), operation.method().as_str());
        assert_eq!(captured.body(), call.body().as_bytes());
        assert_eq!(captured.response_limit(), call.response_limit());
        assert_eq!(captured.timeout_seconds(), call.timeout_seconds());
    }
    for (action, operation) in [
        ("list_versions", Operation::DesignVersionsAction),
        ("import_transformer_version", Operation::DataInterop),
        (
            "publish_transformer_version_revision",
            Operation::DataInterop,
        ),
        ("get_transformer_version_revision", Operation::DataInterop),
        (
            "promote_transformer_version_revision",
            Operation::DataInterop,
        ),
    ] {
        let call = crate::DesignVersionsCall {
            profile: &profile,
            payload: json!({"action":action,"eds_project_id":"project-a"}),
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), Some(operation));
        assert_eq!(captured.path(), operation.path_template());
        assert_eq!(captured.body(), call.body().as_bytes());
        assert_eq!(captured.response_limit(), call.response_limit());
        assert_eq!(captured.timeout_seconds(), call.timeout_seconds());
    }
}

#[test]
fn solar_and_messaging_select_generated_paths_and_keep_the_domain_query() {
    let profile = profile();
    for (artifacts, operation) in [
        (false, Operation::SolarAction),
        (true, Operation::ComputeArtifactsAction),
    ] {
        let call = crate::SolarProjectCall {
            profile: &profile,
            payload: json!({"action":"list","project_id":"project-a"}),
            artifacts,
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), Some(operation));
        assert_eq!(captured.path(), operation.path_template());
        assert_eq!(captured.body(), call.body().as_bytes());
    }
    for (command, operation, path) in [
        (
            crate::messaging::Command::Notifications {
                cursor: None,
                limit: 10,
            },
            Operation::NotificationsAction,
            "/api/v1/notifications",
        ),
        (
            crate::messaging::Command::Acknowledge {
                ids: vec!["notification-a".into()],
            },
            Operation::NotificationsAction,
            "/api/v1/notifications",
        ),
        (
            crate::messaging::Command::ResolveNotifications,
            Operation::MessagingCommands,
            "/api/v1/messaging?wire_contract=messages-v1&accepts=attachment-storage-max",
        ),
    ] {
        let call = crate::messaging::MessagingCall {
            profile: &profile,
            authority: authority(),
            command: &command,
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), Some(operation));
        assert_eq!(captured.path(), path);
        assert_eq!(captured.origin_kind(), GatewayOrigin::Messaging);
        assert_eq!(captured.body(), call.body().as_bytes());
    }
}

#[test]
fn every_survey_route_is_generated_except_the_undeclared_private_template_read() {
    use crate::survey_control::SurveyRoute;
    let profile = profile();
    for (route, operation, method, path) in [
        (
            SurveyRoute::Factory,
            Some(Operation::FormFactoryV2),
            "POST",
            "/api/v1/form-factory",
        ),
        (
            SurveyRoute::ProjectForms,
            Some(Operation::ProjectForms),
            "POST",
            "/api/v1/project-forms",
        ),
        (
            SurveyRoute::Templates,
            Some(Operation::TemplatesAction),
            "POST",
            "/api/v1/projects/templates",
        ),
        (
            SurveyRoute::Projects,
            Some(Operation::ProjectsAction),
            "POST",
            "/api/v1/projects",
        ),
        (
            SurveyRoute::FromTemplate,
            Some(Operation::ProjectFromTemplate),
            "POST",
            "/api/v1/projects/from-template",
        ),
        (
            SurveyRoute::Template("template-a".into()),
            None,
            "GET",
            "/api/v1/projects/templates/template-a",
        ),
    ] {
        let call = crate::SurveyControlCall {
            profile: &profile,
            payload: json!({"action":"list"}),
            route,
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), operation);
        assert_eq!((captured.method(), captured.path()), (method, path));
        if method == "GET" {
            assert!(captured.body().is_empty());
            assert_eq!(captured.send(), GatewaySend::Bytes);
        } else {
            assert_eq!(captured.body(), call.body().as_bytes());
        }
    }
}

#[test]
fn admitted_kernel_configuration_save_uses_report_and_read_keeps_its_expanded_path() {
    use ds_command_kernel::gateway_exchange::{design::DesignCall, Exchange};
    let profile = profile();
    for (command, operation, method, path) in [
        (
            DesignCall::ProjectConfig {
                project: "project-a".into(),
            },
            None,
            "GET",
            "/config/project-a",
        ),
        (
            DesignCall::SaveConfigSheet {
                project: "project-a".into(),
                sheet: "feeders".into(),
                rows: json!([]),
                expected: None,
            },
            Some(Operation::DsSystemReport),
            "POST",
            "/report",
        ),
    ] {
        let call = crate::ProjectConfigurationCall {
            profile: &profile,
            request: command.plan().unwrap().request,
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), operation);
        assert_eq!((captured.method(), captured.path()), (method, path));
        if method == "GET" {
            assert!(captured.body().is_empty());
        } else {
            assert_eq!(captured.body(), call.body().as_bytes());
        }
    }
}

#[test]
fn admitted_sync_requests_use_the_same_generated_operations_without_new_admission() {
    let profile = profile();
    let heartbeat = crate::NativeSyncRequest::heartbeat_without_addition(
        crate::InstallHeartbeatWithoutAddition {
            install_id: "install-a",
            device: "device-a",
            arch: "x86_64",
            app_version: "1.0.0",
            channel: "stable",
            os_version: "linux",
            webview_version: "none",
        },
    )
    .unwrap();
    let list = crate::NativeSyncRequest::for_project(
        crate::SyncGatewayOperation::ComputeArtifacts,
        "project-a",
        &json!({"action":"list","project_id":"project-a","limit":200}),
    )
    .unwrap();
    for (request, operation) in [
        (&heartbeat, Operation::EdgeAppInstallHeartbeat),
        (&list, Operation::ComputeArtifactsAction),
    ] {
        let call = crate::SyncGatewayCall {
            profile: &profile,
            request,
            authority: authority(),
        };
        let captured = call.capture_gateway();
        assert_eq!(captured.operation(), Some(operation));
        assert_eq!(captured.path(), operation.path_template());
        assert_eq!(captured.body(), call.body().as_bytes());
        assert_eq!(captured.response_limit(), call.response_limit());
        assert_eq!(captured.timeout_seconds(), call.timeout_seconds());
    }
}
