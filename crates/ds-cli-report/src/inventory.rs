//! Native final inventory: held verified bytes plus the full heads retained by
//! the one Sync owner. Refresh is an explicit invocation of that same owner.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::report::inventory::{CloudHead, ReadFact, ReadState};
use serde_json::{Value, json};
use std::{io::Write, path::Path};

pub static COMMAND: Command = Command {
    id: "report.inventory",
    path: &["report", "inventory"],
    contract: 1,
    summary: "Read a transformer's verified held and published report inventory.",
    purpose: "Writes a bounded private final inventory document from verified committed local bytes and full authenticated publication heads retained in Sync SQLite. The default makes no cloud call or queue transition. --refresh explicitly runs the existing project Sync pass, which can publish queued work and download changed heads; failure retains clearly held observations. No second queue, freshness probe, generation or local deletion.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::project::PROJECT_ARG,
        crate::project::LANE_ARG,
        Arg::value("transformer", "<exact-key>", "One reportable transformer.").required(),
        Arg::value(
            "out",
            "<absolute-path>",
            "Fresh private JSON output path in an existing private directory.",
        )
        .required(),
        Arg::switch(
            "refresh",
            "Run one governed project Sync pass before reading.",
        ),
        Arg::value(
            "core-state-dir",
            "<absolute-path>",
            "Same state root as the installed Core, if overridden.",
        ),
    ],
    output: "ds.report-inventory-file/v1 receipt: project, transformer, owner, lane, path, sha256, size_bytes. The file is the final ds.report.inventory/v1 view; sync carries the explicit refresh result when requested.",
    examples: &[Example {
        command: "ds report inventory --project <id> --transformer <key> --out <absolute-private-file> --output json",
        note: "Reads held facts without cloud IO; add --refresh for intentional Sync.",
        runnable: false,
    }],
    refusals: crate::outbox::INVENTORY.refusals,
    reference: Some("docs/reference/report.md"),
    search: &["report files", "availability", "held", "download", "sync"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn failed(error: impl std::fmt::Display) -> Failure {
    Failure::failed("report_inventory_unreadable", error.to_string())
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = Path::new(inputs.require("out")?);
    if !out.is_absolute() || out.exists() || !out.parent().is_some_and(Path::is_dir) {
        return Err(failed(
            "Use a fresh absolute output path in an existing private directory.",
        ));
    }
    // Validate the selected identity and local bytes before optional effects.
    let before = crate::outbox::inventory_snapshot(inputs)?;
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let mut sync = inputs
        .switch("refresh")
        .then(|| crate::touch::project(lane, project, inputs.value("core-state-dir")));
    let before_owner = before.owner.clone();
    if let Some(touch) = sync.as_mut() {
        if touch.current
            && touch.published.iter().any(|id| {
                id.engine == "network_reporter"
                    && id.operation
                        == format!("export-{}", inputs.value("transformer").unwrap_or_default())
                    && id.variant == "default"
            })
        {
            if let Err(error) = observe_published_selection(inputs) {
                touch.current = false;
                touch.receipt["state"] = json!("held");
                touch.receipt["reason"] = json!("publication_observation_unavailable");
                touch.receipt["detail"] = json!(error.message());
            }
        }
    }
    let mut snapshot = if sync.is_some() {
        crate::outbox::inventory_snapshot(inputs)?
    } else {
        before
    };
    if snapshot.owner != before_owner {
        return Err(failed("Account changed while refreshing report inventory."));
    }
    let state = crate::outbox::server_state(inputs)?;
    let captured_fence = fence(lane)?;
    if captured_fence.account != snapshot.owner {
        return Err(failed("Account changed while reading report inventory."));
    }
    let store =
        ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).map_err(failed)?;
    attach_cloud(
        &mut snapshot,
        store.as_ref(),
        &captured_fence,
        sync.as_ref().is_some_and(|s| s.current),
        sync.as_ref().filter(|s| !s.current).map(|s| {
            s.receipt["reason"]
                .as_str()
                .unwrap_or("Report Sync did not establish current observations.")
                .to_owned()
        }),
    )?;
    let mut view = serde_json::to_value(
        ds_command_kernel::report::inventory::project(&snapshot).map_err(failed)?,
    )
    .map_err(failed)?;
    let acquired = if let Some(cloud) = snapshot.cloud_head.as_ref() {
        ds_sync_runtime::VerifiedReads::new(state.join("sync-downloads"))
            .retained_head(project, &cloud.head)
            .map_err(failed)?
    } else {
        None
    };
    for output in view["outputs"]
        .as_array_mut()
        .ok_or_else(|| failed("Invalid projected outputs."))?
    {
        output["retained"] = if output["local"].is_object() {
            json!({"source":"committed"})
        } else if acquired.is_some() && output["cloud"].is_object() {
            json!({"source":"sync_download","head_revision":output["cloud_binding"]["head_revision"]})
        } else {
            Value::Null
        };
    }
    if let Some(sync) = sync {
        view["sync"] = sync.receipt;
    }
    let bytes = serde_json::to_vec(&view).map_err(failed)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(failed("Report inventory exceeds its output bound."));
    }
    let mut file = create_private(out)?;
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(failed)?;
        if fence(lane)? != captured_fence {
            return Err(failed("Account changed before inventory delivery."));
        }
        Ok::<_, Failure>(())
    })();
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(out);
        return Err(e);
    }
    Ok(
        json!({"schema":"ds.report-inventory-file/v1","project":project,"transformer":snapshot.transformer,"owner":snapshot.owner,"lane":lane,"path":out,"sha256":ds_compute_runtime::digest(&bytes),"size_bytes":bytes.len()}),
    )
}

pub(crate) fn fence(lane: &str) -> Result<ds_command_kernel::sync_store::Fence, Failure> {
    let p = ds_cli_auth::headless_principal_read_only(lane)?;
    Ok(ds_command_kernel::sync_store::Fence {
        account: p.account_uid().into(),
        deployment: p.deployment().into(),
        install_id: p.install_id().into(),
    })
}

pub(crate) fn create_private(path: &Path) -> Result<std::fs::File, Failure> {
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::create_private_output_file(path).map_err(failed)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(failed)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(failed("Private native files are unavailable."))
    }
}

pub(crate) fn attach_cloud(
    snapshot: &mut ds_command_kernel::report::inventory::Snapshot,
    store: Option<&ds_sync_store::Store>,
    fence: &ds_command_kernel::sync_store::Fence,
    current: bool,
    error: Option<String>,
) -> Result<(), Failure> {
    let observation = store
        .map(|s| s.report_heads(fence, &snapshot.project))
        .transpose()
        .map_err(failed)?
        .flatten();
    if let Some(observation) = observation {
        snapshot.cloud_head = observation
            .heads
            .into_iter()
            .find(|h| {
                h["engine"] == "network_reporter"
                    && h["operation"] == format!("export-{}", snapshot.transformer)
                    && h["variant"] == snapshot.variant
            })
            .map(|raw| {
                ds_command_kernel::compute_artifact_inventory::decode_head(
                    &serde_json::to_vec(&raw).map_err(failed)?,
                )
                .map_err(failed)
            })
            .transpose()?
            .map(|head| CloudHead {
                owner: snapshot.owner.clone(),
                project: snapshot.project.clone(),
                head,
            });
        snapshot.cloud_read = ReadFact {
            state: if current {
                ReadState::Ready
            } else {
                ReadState::Held
            },
            error,
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> ds_command_kernel::report::inventory::Snapshot {
        serde_json::from_value(json!({"owner":"owner","project":"project","transformer":"test","variant":"default","expected_input_base_fingerprint":null,"local_read":{"state":"ready","error":null},"sync_read":{"state":"ready","error":null},"cloud_read":{"state":"not_observed","error":null},"local_sets":[],"sync_artifacts":[],"cloud_head":null,"legacy_cloud":null,"required_outputs":[]})).unwrap()
    }
    #[test]
    fn full_observations_are_held_unless_refresh_is_current_and_reduced_heads_never_expand() {
        let mut store = ds_sync_store::Store::in_memory().unwrap();
        let fence = ds_command_kernel::sync_store::Fence {
            account: "owner".into(),
            deployment: "lane".into(),
            install_id: "install".into(),
        };
        let raw = json!({"engine":"network_reporter","operation":"export-test","variant":"default","head_revision":7});
        store
            .observe_report_heads(&fence, "project", 10, &[raw])
            .unwrap();
        assert!(attach_cloud(&mut snapshot(), Some(&store), &fence, true, None).is_err());
        let head = json!({"work_id":"work1","engine":"network_reporter","operation":"export-test","variant":"default","input_base_fingerprint":"a".repeat(64),"head_revision":7,"updated_at":"2026-10-18T00:00:00Z","outputs":[{"output_id":"xlsx","format":"xlsx","content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","sha256":"b".repeat(64),"size_bytes":123}]});
        store
            .observe_report_heads(&fence, "project", 11, &[head])
            .unwrap();
        let mut held = snapshot();
        attach_cloud(
            &mut held,
            Some(&store),
            &fence,
            false,
            Some("offline".into()),
        )
        .unwrap();
        assert!(matches!(held.cloud_read.state, ReadState::Held));
        assert_eq!(held.cloud_read.error.as_deref(), Some("offline"));
        let view = ds_command_kernel::report::inventory::project(&held).unwrap();
        assert_eq!(view.outputs.len(), 1);
        assert!(view.outputs[0].local.is_none());
        assert!(view.outputs[0].generated_at_ms.is_none());
        assert!(view.outputs[0].client_run_id.is_none());
        let mut current = snapshot();
        attach_cloud(&mut current, Some(&store), &fence, true, None).unwrap();
        assert!(matches!(current.cloud_read.state, ReadState::Ready));
    }
}

fn observe_published_selection(inputs: &Inputs) -> Result<(), Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let before = fence(lane)?;
    let operation = format!("export-{}", inputs.require("transformer")?);
    let raw = ds_cli_auth::sync::publication_heads_for_project(
        lane,
        project,
        ds_cli_auth::sync::PublicationHeadQuery::Show {
            engine: "network_reporter",
            operation: &operation,
            variant: "default",
        },
    )?;
    let head = ds_command_kernel::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(&raw).map_err(failed)?,
    )
    .map_err(failed)?;
    if head.engine != "network_reporter"
        || head.operation != operation
        || head.variant != "default"
        || fence(lane)? != before
    {
        return Err(failed("Selected publication observation changed scope."));
    }
    let state = crate::outbox::server_state(inputs)?;
    let mut store = ds_sync_store::Store::open(&state.join("store.sqlite")).map_err(failed)?;
    store
        .observe_report_head(&before, project, ds_sync_runtime::now_ms(), &raw)
        .map_err(failed)?;
    if fence(lane)? != before {
        return Err(failed("Account changed after publication observation."));
    }
    Ok(())
}
