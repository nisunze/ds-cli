//! Read one retained A4 preview through the shared native core. Hosts supply
//! authorization, a scope fence and verified storage; opening cannot render,
//! capture current engineering inputs or wake the publication queue.
use crate::{Gateway, GatewayError, Reads, SyncRoute};
use ds_command_kernel::{compute_artifact_inventory as inventory, printing::retained_preview};
use serde_json::{Value, json};
use std::path::Path;

/// Keep host scope refusals and authority verdicts intact for the caller.
#[derive(Debug)]
pub enum ReadError<E> {
    Scope(E),
    Gateway(GatewayError),
    Unreadable(String),
    HeadMoved,
}

/// The explicit project and transformer stay fixed across both authority
/// reads and the verified download. A moved head is refused even when the
/// replacement declares identical PDF bytes.
pub fn read_a4<E>(
    gateway: &dyn Gateway,
    reads: &dyn Reads,
    project: &str,
    transformer: &str,
    destination: &Path,
    guard: &dyn Fn() -> Result<(), E>,
) -> Result<inventory::Head, ReadError<E>> {
    guard().map_err(ReadError::Scope)?;
    let operation = format!("export-{transformer}");
    let value = gateway
        .post(
            SyncRoute::ComputeArtifacts,
            &json!({"action":"read","project_id":project,"engine":"network_reporter",
                "operation":operation,"variant":retained_preview::VARIANT}),
        )
        .map_err(ReadError::Gateway)?;
    let head = decode_head(&value)?;
    retained_preview::validate_head(project, transformer, &head).map_err(ReadError::Unreadable)?;
    read_output(
        gateway,
        reads,
        project,
        &head,
        &head.outputs[0].output_id,
        destination,
        guard,
    )?;
    Ok(head)
}

/// Download one already validated immutable declaration. The ticket must name
/// this exact complete head, revision and output; a moved head never substitutes.
pub fn read_output<E>(
    gateway: &dyn Gateway,
    reads: &dyn Reads,
    project: &str,
    head: &inventory::Head,
    output_id: &str,
    destination: &Path,
    guard: &dyn Fn() -> Result<(), E>,
) -> Result<(), ReadError<E>> {
    guard().map_err(ReadError::Scope)?;
    let output = head
        .outputs
        .iter()
        .find(|output| output.output_id == output_id)
        .ok_or_else(|| {
            ReadError::Unreadable("The selected output is absent from its head.".into())
        })?;
    let ticket = gateway
        .post(
            SyncRoute::ComputeArtifacts,
            &json!({"action":"download","project_id":project,"engine":head.engine,
            "operation":head.operation,"variant":head.variant,"output_id":output_id}),
        )
        .map_err(ReadError::Gateway)?;
    let url = admitted_ticket_url(head, output, &ticket)?;
    guard().map_err(ReadError::Scope)?;
    reads
        .download_verified(url, destination, &output.sha256, output.size_bytes)
        .map_err(ReadError::Unreadable)?;
    guard().map_err(ReadError::Scope)?;
    Ok(())
}

fn decode_head<E>(value: &Value) -> Result<inventory::Head, ReadError<E>> {
    inventory::decode_head(
        &serde_json::to_vec(value).map_err(|error| ReadError::Unreadable(error.to_string()))?,
    )
    .map_err(ReadError::Unreadable)
}

fn admitted_ticket_url<'a, E>(
    head: &inventory::Head,
    expected_output: &inventory::Declaration,
    ticket: &'a Value,
) -> Result<&'a str, ReadError<E>> {
    let resolved = decode_head(&ticket["head"])?;
    if resolved != *head {
        return Err(ReadError::HeadMoved);
    }
    // Head and download declarations use the same portable normalization owner.
    // Historical Reporter tickets may omit the stored canonical filename.
    let projection = inventory::evaluate(
        &serde_json::to_vec(&json!({"engine":head.engine,"operation":head.operation,
            "outputs":head.outputs,"download":{"output":ticket["output"],"file_name":ticket["file_name"]}}))
        .map_err(|error| ReadError::Unreadable(error.to_string()))?,
    )
    .map_err(ReadError::Unreadable)?;
    #[derive(serde::Deserialize)]
    struct DownloadProjection {
        download_output: inventory::Declaration,
    }
    let projection: DownloadProjection = serde_json::from_str(&projection)
        .map_err(|error| ReadError::Unreadable(error.to_string()))?;
    if projection.download_output != *expected_output {
        return Err(ReadError::HeadMoved);
    }
    ticket["download_url"].as_str().ok_or_else(|| {
        ReadError::Unreadable("The retained PDF has no governed download ticket.".into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque, path::PathBuf};

    fn head() -> inventory::Head {
        use retained_preview::{Binding, SCHEMA, TEMPLATE, VARIANT};
        let binding = Binding {
            schema: SCHEMA.into(),
            template_id: TEMPLATE.into(),
            template_revision_id: "a".repeat(64),
            template_html_sha256: "b".repeat(64),
            analysis_sha256: "c".repeat(64),
            sheets_sha256: "d".repeat(64),
            report_input_fingerprint: "e".repeat(64),
            renderer_style_ref: "renderer_print".into(),
            renderer_revision_id: "renderer_revision".into(),
            renderer_content_sha256: "f".repeat(64),
            transformer_revision: 1,
            content_digest: "1".repeat(64),
        };
        let fingerprint = binding
            .input_base_fingerprint("project_a", "transformer_a")
            .unwrap();
        serde_json::from_value(json!({"printing_preview":binding,"work_id":"work_a",
            "engine":"network_reporter","engine_version":format!("ds-network-reporter@0.1.0+{}","2".repeat(40)),
            "engine_build_manifest_sha256":"3".repeat(64),"operation":"export-transformer_a",
            "variant":VARIANT,"input_base_fingerprint":fingerprint,"head_revision":1,
            "updated_at":"2026-10-05T00:00:00Z","outputs":[{"output_id":"voltage_drop_pdf",
            "filename":ds_command_kernel::report_formats::report_filename("transformer_a","voltage_drop_pdf").unwrap(),
            "format":"voltage_drop_pdf","content_type":"application/pdf","sha256":"4".repeat(64),
            "size_bytes":123,"paper_size":"A4"}]})).unwrap()
    }

    fn ticket(head: &inventory::Head) -> Value {
        json!({"head":head,"output":head.outputs[0],"file_name":head.outputs[0].filename,
            "download_url":"https://storage.googleapis.com/b/object?generation=1"})
    }

    #[test]
    fn general_output_ticket_binds_the_selected_output_not_the_first_output() {
        let mut value = serde_json::to_value(head()).unwrap();
        value.as_object_mut().unwrap().remove("printing_preview");
        value["variant"] = json!("default");
        value["outputs"] = json!([
            {"output_id":"xlsx","format":"xlsx","content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","sha256":"5".repeat(64),"size_bytes":42},
            value["outputs"][0].clone()
        ]);
        let selected = inventory::decode_head(&serde_json::to_vec(&value).unwrap()).unwrap();
        let output = &selected.outputs[1];
        let ticket = json!({"head":selected,"output":output,"file_name":inventory::filename(&selected.engine,&selected.operation,output),"download_url":"https://storage.googleapis.com/b/object?generation=2"});
        assert!(admitted_ticket_url::<()>(&selected, output, &ticket).is_ok());
        assert!(matches!(
            admitted_ticket_url::<()>(&selected, &selected.outputs[0], &ticket),
            Err(ReadError::HeadMoved)
        ));
    }

    #[test]
    fn historical_download_filename_uses_shared_projection_without_weakening_pins() {
        let mut raw = serde_json::to_value(head()).unwrap();
        raw.as_object_mut().unwrap().remove("printing_preview");
        raw["variant"] = json!("default");
        raw["outputs"] = json!([{"output_id":"xlsx","format":"xlsx",
            "content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "sha256":"5".repeat(64),"size_bytes":42}]);
        let selected = inventory::decode_head(&serde_json::to_vec(&raw).unwrap()).unwrap();
        let ticket = json!({"head":raw,"output":raw["outputs"][0],"file_name":"",
            "download_url":"https://storage.googleapis.com/b/object?generation=2"});
        assert!(admitted_ticket_url::<()>(&selected, &selected.outputs[0], &ticket).is_ok());
        for (field, value) in [
            ("sha256", json!("6".repeat(64))),
            ("size_bytes", json!(43)),
            ("filename", json!("Wrong.xlsx")),
        ] {
            let mut wrong = ticket.clone();
            wrong["output"][field] = value;
            assert!(admitted_ticket_url::<()>(&selected, &selected.outputs[0], &wrong).is_err());
        }
        let mut wrong = ticket;
        wrong["file_name"] = json!("Wrong.xlsx");
        assert!(admitted_ticket_url::<()>(&selected, &selected.outputs[0], &wrong).is_err());
    }

    struct Host {
        answers: RefCell<VecDeque<Result<Value, GatewayError>>>,
        events: RefCell<Vec<String>>,
    }
    impl Host {
        fn new(head: &inventory::Head, ticket: Value) -> Self {
            Self {
                answers: RefCell::new(VecDeque::from([Ok(json!(head)), Ok(ticket)])),
                events: RefCell::new(Vec::new()),
            }
        }
        fn guard(&self) -> Result<(), &'static str> {
            self.events.borrow_mut().push("guard".into());
            Ok(())
        }
        fn read(&self, project: &str) -> Result<inventory::Head, ReadError<&'static str>> {
            read_a4(
                self,
                self,
                project,
                "transformer_a",
                Path::new("preview.pdf"),
                &|| self.guard(),
            )
        }
    }
    impl Gateway for Host {
        fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
            assert_eq!(route, SyncRoute::ComputeArtifacts);
            assert_eq!(body["project_id"], "project_a");
            assert_eq!(body["engine"], "network_reporter");
            assert_eq!(body["operation"], "export-transformer_a");
            assert_eq!(body["variant"], retained_preview::VARIANT);
            let action = body["action"].as_str().unwrap();
            assert!(matches!(action, "read" | "download"));
            if action == "download" {
                assert_eq!(body["output_id"], "voltage_drop_pdf");
            }
            self.events.borrow_mut().push(action.into());
            self.answers
                .borrow_mut()
                .pop_front()
                .expect("unexpected authority call")
        }
    }
    impl Reads for Host {
        fn head_root(&self, _: &str, _: &crate::kernel_sync::Identity) -> Result<PathBuf, String> {
            panic!("retained opening does not inspect a sync cache")
        }
        fn download_verified(
            &self,
            url: &str,
            destination: &Path,
            sha: &str,
            size: u64,
        ) -> Result<(), String> {
            assert_eq!(url, "https://storage.googleapis.com/b/object?generation=1");
            assert_eq!(destination, Path::new("preview.pdf"));
            assert_eq!(sha, "4".repeat(64));
            assert_eq!(size, 123);
            self.events.borrow_mut().push("bytes".into());
            Ok(())
        }
    }

    #[test]
    fn opening_only_reads_the_fixed_head_ticket_and_verified_bytes() {
        let head = head();
        let host = Host::new(&head, ticket(&head));
        assert_eq!(host.read("project_a").unwrap(), head);
        assert_eq!(
            *host.events.borrow(),
            [
                "guard", "read", "guard", "download", "guard", "bytes", "guard"
            ]
        );
    }

    #[test]
    fn moved_equal_byte_head_and_crossed_output_never_download() {
        let head = head();
        for change in 0..3 {
            let mut ticket = ticket(&head);
            match change {
                0 => {
                    ticket["head"]["work_id"] = json!("work_b");
                    ticket["head"]["head_revision"] = json!(2);
                }
                1 => ticket["output"]["sha256"] = json!("5".repeat(64)),
                _ => ticket["file_name"] = json!("other.pdf"),
            }
            let host = Host::new(&head, ticket);
            assert!(host.read("project_a").is_err());
            assert_eq!(
                *host.events.borrow(),
                ["guard", "read", "guard", "download"]
            );
        }
    }

    #[test]
    fn cross_project_or_malformed_head_never_requests_a_ticket() {
        let head = head();
        for change in 0..2 {
            let host = Host::new(&head, ticket(&head));
            let mut foreign = json!(head);
            if change == 0 {
                foreign["input_base_fingerprint"] = json!(
                    head.printing_preview
                        .as_ref()
                        .unwrap()
                        .input_base_fingerprint("project_b", "transformer_a")
                        .unwrap()
                );
            } else {
                foreign["outputs"] = json!([]);
            }
            host.answers.borrow_mut()[0] = Ok(foreign);
            assert!(matches!(
                host.read("project_a"),
                Err(ReadError::Unreadable(_))
            ));
            assert_eq!(*host.events.borrow(), ["guard", "read"]);
        }
    }

    #[test]
    fn each_scope_change_stops_the_next_effect_and_retains_host_refusal() {
        let head = head();
        for refused_at in 0..4 {
            let host = Host::new(&head, ticket(&head));
            let calls = std::cell::Cell::new(0);
            let guard = || {
                let call = calls.get();
                calls.set(call + 1);
                if call == refused_at {
                    Err("project_context_changed")
                } else {
                    host.guard()
                }
            };
            let result = read_a4(
                &host,
                &host,
                "project_a",
                "transformer_a",
                Path::new("preview.pdf"),
                &guard,
            );
            assert!(matches!(
                result,
                Err(ReadError::Scope("project_context_changed"))
            ));
            let effects: Vec<_> = host
                .events
                .borrow()
                .iter()
                .filter(|event| *event != "guard")
                .cloned()
                .collect();
            assert_eq!(effects.len(), refused_at);
        }
    }

    #[test]
    fn missing_head_retains_authority_verdict_and_does_not_render() {
        let head = head();
        let host = Host::new(&head, ticket(&head));
        let missing = GatewayError::refused(404, Some("not_found".into()), "No retained preview");
        host.answers.borrow_mut()[0] = Err(missing.clone());
        assert!(
            matches!(host.read("project_a"), Err(ReadError::Gateway(error)) if error == missing)
        );
        assert_eq!(*host.events.borrow(), ["guard", "read"]);
    }
}
