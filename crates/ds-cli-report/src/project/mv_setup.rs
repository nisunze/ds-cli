//! Thin native adapter for the canonical project selection in the shared print library.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::printing::mv::{self, ModelField, Resolved, Selection};
use ds_report_artifacts::confined_fs::HeldDirectory;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const REFUSAL: Refusal = Refusal {
    code: "mv_print_setup_refused",
    when: "the canonical MV selection, adopted revision, required text or allowed model differences are incomplete or changed",
    remedy: "read the keyed refusal in detail; inspect ds report project settings and the adopted ds report layout get document, then intentionally repair or select its exact revision",
};
pub const STYLE_REFUSAL: Refusal = Refusal {
    code: "mv_print_style_unresolved",
    when: "the project API has not supplied the governed MV paper binding",
    remedy: "integrate the print-styles resolver for (mv_booklet, project_model, print, project); adopt governed defaults through the API",
};
pub const PROJECT_CRS_CONTEXT_REFUSAL: Refusal = Refusal {
    code: "print_project_crs_context_mismatch",
    when: "the held project configuration and authenticated projection directory differ in project, lane, principal or credential audience",
    remedy: "refresh both reads through the same native account and lane for the exact project, then retry",
};
const OUTPUT_REFUSAL: Refusal = Refusal {
    code: "mv_print_setup_output_invalid",
    when: "--out is not a fresh absolute file in an existing nonsymlink directory, or its held destination changes or cannot persist the complete receipt",
    remedy: "choose a fresh absolute JSON filename in an existing directory; existing files are never overwritten",
};
pub static SET: Command = Command {
    id:"report.project.mv-setup.set", path:&["report","project","mv-setup","set"], contract:1,
    summary:"Select the project's one canonical MV printing revision.",
    purpose:"Save one ds.mv-print-selection/v1 selection in the existing project settings sheet, behind an exact-base concurrent-write fence. The project must hold an MV layout copied from an exact global revision. This never publishes a template, alters geometry, advances an issue date or prints; missing model bindings or held PDF assets remain explicit print-time refusals. Discover selection and layout shapes through report layout schema.",
    chapter:Chapter::Reports, effect:Effect::GlobalWrite, authority:Authority::HeadlessProject,
    execution:Execution::Sync,
    args:&[super::PROJECT_ARG,super::LANE_ARG,Arg::value("selection","<json-file>","ds.mv-print-selection/v1: layout_id, exact revision, external_version and fixed issue_date.").required()],
    output:"Exact project and saved canonical selection. Printing readiness must be checked with the resolved model and held assets.",
    examples:&[], refusals:&super::joined::<{super::NATIVE_WRITE_REFUSALS.len()+1}>(&[super::NATIVE_WRITE_REFUSALS,&[REFUSAL]]),
    reference:Some("docs/reference/report.md"),search:&["front matter","template adoption"], requires:Requires::Server,availability:ds_cli_auth::native_availability,
};
pub static RESOLVE: Command = Command {
    id: "report.project.mv-setup.resolve",
    path: &["report", "project", "mv-setup", "resolve"],
    contract: 2,
    summary: "Resolve canonical MV setup for one model's allowed title differences.",
    purpose: "Resolve the named project's exact adopted printing revision, approved furniture and fixed version/date. Only template-permitted model identity/title may differ; missing selection refuses. By default return the complete native receipt inline. --out writes those same resolved document bytes to a fresh local file and returns bounded identity, lineage, preserved-page pins and digests for MCP. No project/template save, render or asset acquisition; geometry and held PDFs still require reporter validation.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        super::PROJECT_ARG,
        super::LANE_ARG,
        Arg::value(
            "model-identity",
            "<text>",
            "Explicit model identity, only when permitted by the adopted template.",
        ),
        Arg::value(
            "model-title",
            "<text>",
            "Explicit model drawing title, only when permitted by the adopted template.",
        ),
        Arg::value(
            "out",
            "<absolute-json>",
            "Write the complete resolved document to a fresh file in an existing directory; return bounded metadata and exact file SHA-256/bytes. Omit for the full inline receipt.",
        ),
    ],
    output: "Inline: exact native resolved setup. --out: ds.mv-print-resolved-file/v1 with bounded resolved identity/selection/lineage/model fields/preserved-page pins and canonical sha256, plus document path/schema/exact-byte sha256/bytes; ready:false in both modes. No embedded PNG bytes in the file-mode answer.",
    examples: &[],
    refusals: &super::joined::<{ super::NATIVE_READ_REFUSALS.len() + 2 }>(&[
        super::NATIVE_READ_REFUSALS,
        &[REFUSAL, OUTPUT_REFUSAL],
    ]),
    reference: Some("docs/reference/report.md"),
    search: &["front matter", "template adoption"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn resolve(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    resolve_with(inputs, resolve_project)
}

fn resolve_with(
    inputs: &Inputs,
    acquire: impl FnOnce(&str, &str, BTreeMap<ModelField, String>) -> Result<Resolved, Failure>,
) -> Result<Value, Failure> {
    // Destination admission precedes all credential or project acquisition.
    let output = inputs
        .value("out")
        .map(OutputDestination::open)
        .transpose()?;
    let mut fields = BTreeMap::new();
    for (arg, field) in [
        ("model-identity", ModelField::Identity),
        ("model-title", ModelField::Title),
    ] {
        if let Some(value) = inputs.value(arg) {
            fields.insert(field, value.into());
        }
    }
    let resolved = acquire(inputs.require("lane")?, inputs.require("project")?, fields)?;
    match output {
        Some(output) => output.write(&resolved),
        None => Ok(json!({"resolved":resolved,"ready":false})),
    }
}

struct OutputDestination {
    path: PathBuf,
    parent: HeldDirectory,
    name: OsString,
}

fn output_invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(OUTPUT_REFUSAL.code, message.to_string()).remedy(OUTPUT_REFUSAL.remedy)
}

impl OutputDestination {
    fn open(value: &str) -> Result<Self, Failure> {
        let path = Path::new(value);
        if !path.is_absolute() {
            return Err(output_invalid("--out must be an absolute file path"));
        }
        let name = path
            .file_name()
            .ok_or_else(|| output_invalid("--out must name a file"))?
            .to_owned();
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Err(output_invalid("--out already exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(output_invalid(error)),
        }
        let parent = HeldDirectory::open_absolute(
            path.parent()
                .ok_or_else(|| output_invalid("--out has no parent directory"))?,
        )
        .map_err(output_invalid)?
        .ok_or_else(|| output_invalid("--out parent directory does not exist"))?;
        Ok(Self {
            path: path.to_owned(),
            parent,
            name,
        })
    }

    fn write(self, resolved: &Resolved) -> Result<Value, Failure> {
        if !self
            .parent
            .identity_matches_path()
            .map_err(output_invalid)?
        {
            return Err(output_invalid("--out parent changed during resolution"));
        }
        // Serialize the same Value the inline answer carries, including every
        // embedded asset. File transport never projects or re-resolves it.
        let value = serde_json::to_value(resolved).map_err(output_invalid)?;
        let bytes = serde_json::to_vec(&value).map_err(output_invalid)?;
        let limit = ds_command_kernel::printing::MAX_INPUT_BYTES as u64;
        self.parent
            .write_new_bounded(&self.name, &bytes, limit, "MV resolved setup")
            .map_err(output_invalid)?;
        let written = self
            .parent
            .read_bounded(&self.name, limit, "MV resolved setup")
            .map_err(output_invalid)?;
        if written != bytes
            || !self
                .parent
                .identity_matches_path()
                .map_err(output_invalid)?
        {
            return Err(
                output_invalid("--out changed while its receipt was verified")
                    .detail(json!({"path":self.path,"written":true})),
            );
        }
        Ok(json!({
            "schema":"ds.mv-print-resolved-file/v1",
            "resolved": {
                "project_id":resolved.project_id,
                "selection":resolved.selection,
                "inherited_from":resolved.inherited_from,
                "model_fields":resolved.model_fields,
                "preserved_pages":resolved.preserved_pages,
                "sha256":resolved.sha256,
            },
            "document": {
                "path":self.path,
                "schema":resolved.schema,
                "sha256":format!("sha256:{:x}", Sha256::digest(&bytes)),
                "bytes":bytes.len(),
            },
            "ready":false,
        }))
    }
}
pub fn render_resolve(value: &Value) -> String {
    let mut text = format!(
        "project {} · MV setup {} · revision {}\nreceipt {}",
        value["resolved"]["project_id"],
        value["resolved"]["selection"]["layout_id"],
        value["resolved"]["selection"]["revision"],
        value["resolved"]["sha256"]
    );
    if let Some(document) = value.get("document") {
        text.push_str(&format!(
            "\ndocument {} · {} bytes · {}",
            document["path"], document["bytes"], document["sha256"]
        ));
    }
    text
}

pub fn failure(error: mv::Refusal) -> Failure {
    Failure::invalid(REFUSAL.code, error.to_string())
        .remedy(error.remedy.clone())
        .detail(json!({"refusal":error}))
}
pub(crate) fn resolve_project(
    lane: &str,
    project: &str,
    fields: BTreeMap<ModelField, String>,
) -> Result<Resolved, Failure> {
    let configuration = ds_cli_auth::feeder_configuration_for_project(lane, project)?.into_result();
    let sheets = printing_inputs(lane, project, &configuration.document["sheets"])?;
    mv::resolve(&sheets, project, fields).map_err(failure)
}

fn printing_inputs(lane: &str, project: &str, sheets: &Value) -> Result<Value, Failure> {
    if mv::uses_global_default(sheets) {
        return Err(failure(unselected()));
    }
    super::settings::sheets_with_printing_catalogue(lane, project, sheets, None)
}
/// A project without its own MV selection. The global printing runtime scope
/// is retired (template projects are ordinary projects), so no approved global
/// default can be fetched: the project selects an adopted setup of its own.
fn unselected() -> mv::Refusal {
    mv::Refusal {
        code: "mv_print_setup_missing".into(),
        message_key: "mv_print_setup_missing".into(),
        field: mv::SETTING.into(),
        remedy: "Copy the approved MV setup into this project (ds report layout copy), then select its exact revision with ds report project mv-setup set; there is no global default.".into(),
    }
}
/// The server-issued configuration sheets and authenticated project CRS
/// capture every MV print or preview starts from, fenced to one identity.
pub(crate) fn project_print_context(
    lane: &str,
    project: &str,
) -> Result<
    (
        Value,
        Option<ds_command_kernel::printing::project_crs::Capture>,
    ),
    Failure,
> {
    let (sheets, project_crs, _) = project_print_context_with_identity(lane, project)?;
    Ok((sheets, project_crs))
}

/// Preserve the identity already captured by these same authorized reads for a
/// native preview room. No additional discovery or project acquisition.
pub(crate) fn project_print_context_with_identity(
    lane: &str,
    project: &str,
) -> Result<
    (
        Value,
        Option<ds_command_kernel::printing::project_crs::Capture>,
        ds_cli_auth::ProviderIdentity,
    ),
    Failure,
> {
    let scoped = ds_cli_auth::feeder_configuration_for_project(lane, project)?;
    let directory = ds_cli_auth::project_directory(lane)?;
    if scoped.project_id() != project || scoped.identity() != directory.identity() {
        return Err(Failure::invalid(
            "print_project_crs_context_mismatch",
            "print project CRS/configuration context differs",
        )
        .remedy(PROJECT_CRS_CONTEXT_REFUSAL.remedy));
    }
    let identity = scoped.identity().clone();
    let project_crs = directory
        .project_params(project)
        .filter(|params| params["crs"].is_object())
        .map(|params| {
            ds_command_kernel::printing::project_crs::Capture::new(
                project,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                params.clone(),
            )
        })
        .transpose()
        .map_err(|e| Failure::invalid("print_project_crs_invalid", e))?;
    let configuration = scoped.into_result();
    Ok((
        configuration.document["sheets"].clone(),
        project_crs,
        identity,
    ))
}
/// The project's governed style table, as MV paper and renderer bindings read it.
pub(crate) fn style_table(
    lane: &str,
    project: &str,
) -> Result<ds_command_kernel::style_resolution::Snapshot, Failure> {
    let table = ds_cli_auth::style_governance(
        lane,
        project,
        &ds_command_kernel::style_governance::Command::Table,
    )?;
    serde_json::from_value(table).map_err(|error| {
        Failure::failed(STYLE_REFUSAL.code, error.to_string()).remedy(STYLE_REFUSAL.remedy)
    })
}
/// One MV print's resolved setup, paper and renderer bindings, report
/// configuration, project CRS and the style table they came from.
pub(crate) type ProjectPrint = (
    Resolved,
    Value,
    Value,
    Value,
    Option<ds_command_kernel::printing::project_crs::Capture>,
    ds_command_kernel::style_resolution::Snapshot,
);
/// Resolver acquisition seam for print-styles integration. The authorized
/// project API materializes this exact binding; the CLI never searches a
/// catalogue, constructs an id, or supplies a packaged paint default.
pub(crate) fn resolve_project_print(
    lane: &str,
    project: &str,
    fields: BTreeMap<ModelField, String>,
) -> Result<ProjectPrint, Failure> {
    let (configuration, project_crs) = project_print_context(lane, project)?;
    let sheets = printing_inputs(lane, project, &configuration)?;
    let setup = mv::resolve(&sheets, project, fields).map_err(failure)?;
    let snapshot = style_table(lane, project)?;
    let (paper, renderer) = mv::resolve_print_bindings(&setup, &snapshot).map_err(|error| {
        Failure::failed(STYLE_REFUSAL.code, error.to_string()).remedy(STYLE_REFUSAL.remedy)
    })?;
    Ok((
        setup,
        json!(paper),
        json!(renderer),
        sheets,
        project_crs,
        snapshot,
    ))
}

pub fn set(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("selection")?)
        .and_then(|f| f.take(64 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| Failure::invalid(REFUSAL.code, e.to_string()).remedy(REFUSAL.remedy))?;
    if bytes.len() > 64 * 1024 {
        return Err(
            Failure::invalid(REFUSAL.code, "selection exceeds 64 KiB").remedy(REFUSAL.remedy)
        );
    }
    let selected: Selection = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid(REFUSAL.code, e.to_string()).remedy(REFUSAL.remedy))?;
    selected.validate().map_err(failure)?;
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let configuration = ds_cli_auth::feeder_configuration_for_project(lane, project)?.into_result();
    let expected_rows = configuration.document["sheets"]["project_settings"]
        .as_array()
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(REFUSAL.code, "project_settings is not a parameter sheet")
                .remedy(REFUSAL.remedy)
        })?;
    let rows = mv::patch_selection(&expected_rows, &selected).map_err(failure)?;
    let mut sheets = configuration.document["sheets"].clone();
    sheets["project_settings"] = json!(rows);
    let sheets = super::settings::sheets_with_printing_catalogue(lane, project, &sheets, None)?;
    // A model-specific required field is intentionally supplied only by that
    // model at print time. All structural/adoption errors still refuse here.
    if let Err(error) = mv::resolve(&sheets, project, BTreeMap::new())
        && (!matches!(
            error.code.as_str(),
            "mv_print_text_missing" | "mv_print_model_binding_missing"
        ) || !matches!(error.field.as_str(), "model_identity" | "model_title"))
    {
        return Err(failure(error));
    }
    let saved = ds_cli_auth::mv_printing_selection_for_project(
        lane,
        project,
        selected.clone(),
        expected_rows,
    )?;
    let actual = mv::selection(&saved.document["sheets"]).map_err(failure)?;
    if actual != selected {
        return Err(
            Failure::failed(REFUSAL.code, "saved MV selection differs from readback")
                .remedy(REFUSAL.remedy),
        );
    }
    Ok(json!({"project":project,"lane":lane,"selection":actual,"saved":true,"ready":false}))
}
pub fn render(value: &Value) -> String {
    format!(
        "project {} · MV setup {} · revision {} · saved {}",
        value["project"],
        value["selection"]["layout_id"],
        value["selection"]["revision"],
        value["saved"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn inputs(out: Option<&Path>) -> Inputs {
        let mut tokens = vec!["--project".into(), "project_a".into()];
        if let Some(out) = out {
            tokens.extend(["--out".into(), out.to_str().unwrap().into()]);
        }
        ds_cli_contract::parse(&RESOLVE, &tokens).unwrap()
    }

    fn sheets() -> Value {
        serde_json::from_str(include_str!(
            "../../../../../ds-command-kernel/tests/fixtures/mv-printing-setup.json"
        ))
        .unwrap()
    }

    fn resolved() -> Resolved {
        mv::resolve(&sheets(), "project_a", BTreeMap::new()).unwrap()
    }

    #[test]
    fn invalid_destinations_refuse_before_acquisition_and_preserve_files() {
        let temporary = tempfile::tempdir().unwrap();
        let owned = temporary.path().join("owner.json");
        std::fs::write(&owned, b"owner bytes").unwrap();
        let calls = Cell::new(0);
        for path in [
            PathBuf::from("relative.json"),
            owned.clone(),
            temporary.path().join("missing-parent/result.json"),
        ] {
            let refused = resolve_with(&inputs(Some(&path)), |_, _, _| {
                calls.set(calls.get() + 1);
                Ok(resolved())
            })
            .unwrap_err();
            assert_eq!(refused.code(), OUTPUT_REFUSAL.code);
        }
        assert_eq!(calls.get(), 0);
        assert_eq!(std::fs::read(owned).unwrap(), b"owner bytes");
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn linked_destinations_and_parents_refuse_without_acquisition_or_changes() {
        use std::os::unix::fs::symlink;
        let temporary = tempfile::tempdir().unwrap();
        let owned = temporary.path().join("owner.json");
        std::fs::write(&owned, b"owner bytes").unwrap();
        let linked = temporary.path().join("linked.json");
        symlink(&owned, &linked).unwrap();
        let absent = temporary.path().join("absent.json");
        let dangling = temporary.path().join("dangling.json");
        symlink(&absent, &dangling).unwrap();
        let parent = temporary.path().join("parent-link");
        symlink(temporary.path(), &parent).unwrap();
        let calls = Cell::new(0);
        for path in [linked.clone(), dangling.clone(), parent.join("result.json")] {
            let refused = resolve_with(&inputs(Some(&path)), |_, _, _| {
                calls.set(calls.get() + 1);
                Ok(resolved())
            })
            .unwrap_err();
            assert_eq!(refused.code(), OUTPUT_REFUSAL.code);
        }
        assert_eq!(calls.get(), 0);
        assert_eq!(std::fs::read(owned).unwrap(), b"owner bytes");
        assert_eq!(
            std::fs::read_link(linked).unwrap(),
            temporary.path().join("owner.json")
        );
        assert_eq!(std::fs::read_link(dangling).unwrap(), absent);
        assert!(!temporary.path().join("result.json").exists());
    }

    #[test]
    fn concurrent_destination_creation_never_overwrites_the_other_writer() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("result.json");
        let calls = Cell::new(0);
        let refused = resolve_with(&inputs(Some(&path)), |_, _, _| {
            calls.set(calls.get() + 1);
            std::fs::write(&path, b"concurrent owner bytes").unwrap();
            Ok(resolved())
        })
        .unwrap_err();
        assert_eq!(refused.code(), OUTPUT_REFUSAL.code);
        assert_eq!(calls.get(), 1);
        assert_eq!(std::fs::read(path).unwrap(), b"concurrent owner bytes");
    }

    #[cfg(unix)]
    #[test]
    fn replaced_parent_refuses_before_writing_either_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let moved = temporary.path().join("held-parent");
        let path = parent.join("result.json");
        let refused = resolve_with(&inputs(Some(&path)), |_, _, _| {
            std::fs::rename(&parent, &moved).unwrap();
            std::fs::create_dir(&parent).unwrap();
            Ok(resolved())
        })
        .unwrap_err();
        assert_eq!(refused.code(), OUTPUT_REFUSAL.code);
        assert!(!path.exists());
        assert!(!moved.join("result.json").exists());
    }

    #[test]
    fn file_mode_preserves_native_refusals_without_creating_output() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("result.json");
        let mut missing_selection = sheets();
        missing_selection["project_settings"] = json!([]);
        let mut changed_revision = sheets();
        changed_revision["printing_setups"][0]["revision"] = json!("c".repeat(64));
        for (sheets, fields) in [
            (missing_selection, BTreeMap::new()),
            (changed_revision, BTreeMap::new()),
            (sheets(), BTreeMap::from([(ModelField::Title, " ".into())])),
        ] {
            let expected = mv::resolve(&sheets, "project_a", fields.clone()).unwrap_err();
            let expected_failure = failure(expected);
            let refused = resolve_with(&inputs(Some(&path)), |_, _, _| {
                mv::resolve(&sheets, "project_a", fields).map_err(failure)
            })
            .unwrap_err();
            assert_eq!(refused.code(), expected_failure.code());
            assert_eq!(refused.message(), expected_failure.message());
            assert_eq!(refused.remedy_text(), expected_failure.remedy_text());
            assert_eq!(refused.detail_value(), expected_failure.detail_value());
            assert!(!path.exists());
        }
    }

    #[test]
    fn oversized_held_png_receipt_is_exact_in_file_and_bounded_in_the_answer() {
        use base64::Engine;
        let mut sheets = sheets();
        let asset = &mut sheets["printing_setups"][0]["layout"]["assets"]["test_party"];
        let encoded = asset["png_data_url"]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        let mut png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        // Keep the fixture pixel's image chunks intact and add one valid PNG
        // ancillary text chunk. This is synthetic held artwork, not project data.
        let mut text = b"fixture\0".to_vec();
        text.extend(vec![b'x'; 245_000]);
        let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
        chunk.extend(b"tEXt");
        chunk.extend(&text);
        let mut crc = u32::MAX;
        for byte in &chunk[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
            }
        }
        chunk.extend((!crc).to_be_bytes());
        let before_iend = png.len() - 12;
        png.splice(before_iend..before_iend, chunk);
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        asset["png_data_url"] = json!(data_url);
        let resolved = mv::resolve(&sheets, "project_a", BTreeMap::new()).unwrap();
        resolved.validate("project_a").unwrap();
        let native = serde_json::to_value(&resolved).unwrap();
        let bytes = serde_json::to_vec(&native).unwrap();
        assert!(bytes.len() > 256 * 1024);

        let calls = Cell::new(0);
        let inline = resolve_with(&inputs(None), |lane, project, fields| {
            calls.set(calls.get() + 1);
            assert_eq!((lane, project), ("stable", "project_a"));
            assert!(fields.is_empty());
            Ok(resolved.clone())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(inline, json!({"resolved":native,"ready":false}));

        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("resolved.json");
        let answer = resolve_with(&inputs(Some(&path)), |lane, project, fields| {
            calls.set(calls.get() + 1);
            assert_eq!((lane, project), ("stable", "project_a"));
            assert!(fields.is_empty());
            Ok(resolved.clone())
        })
        .unwrap();
        assert_eq!(
            calls.get(),
            2,
            "one acquisition per invocation in either mode"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(answer["document"]["path"], json!(path));
        assert_eq!(answer["document"]["bytes"], bytes.len());
        assert_eq!(
            answer["document"]["sha256"],
            format!("sha256:{:x}", Sha256::digest(&bytes))
        );
        let restored: Resolved = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, resolved);
        restored.validate("project_a").unwrap();
        for field in [
            "project_id",
            "selection",
            "inherited_from",
            "model_fields",
            "preserved_pages",
            "sha256",
        ] {
            assert_eq!(answer["resolved"][field], native[field], "{field}");
        }
        assert_eq!(answer["schema"], "ds.mv-print-resolved-file/v1");
        assert_eq!(answer["document"]["schema"], native["schema"]);
        assert!(answer["resolved"]["schema"].is_null());
        assert_eq!(answer["ready"], false);
        assert!(answer["resolved"]["layout"].is_null());
        let envelope = serde_json::to_value(ds_cli_contract::outcome::success_envelope(
            RESOLVE.id,
            RESOLVE.contract,
            answer,
        ))
        .unwrap();
        let text = serde_json::to_string(&envelope).unwrap();
        assert!(!text.contains("png_data_url"));
        // Include both MCP copies and JSON escaping, not only the data object.
        let result = json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":envelope,
            "isError":false,
        });
        assert!(serde_json::to_vec(&result).unwrap().len() < 16 * 1024);
    }
    #[test]
    fn a_project_without_its_own_mv_selection_is_refused_before_any_read() {
        // No mv_printing_setup row: the retired global default is never fetched.
        let refused = printing_inputs("canary", "project_one", &json!({"project_settings": []}))
            .expect_err("no global fallback");
        assert_eq!(refused.code(), REFUSAL.code);
        assert!(refused.message().starts_with("mv_print_setup_missing"));
        assert!(!refused.message().contains("Global printing runtime scope"));
    }
    #[test]
    fn descriptors_declare_project_identity_mutation_confirmation_and_discovery() {
        assert!(SET.arg("project").unwrap().required);
        assert!(SET.effect.needs_confirmation());
        assert_eq!(RESOLVE.authority, Authority::HeadlessProject);
        assert_eq!(RESOLVE.contract, 2);
        assert_eq!(RESOLVE.effect, Effect::LocalFileWrite);
        assert!(!RESOLVE.arg("out").unwrap().required);
        assert!(!RESOLVE.effect.needs_confirmation());
        assert!(SET.search.contains(&"front matter"));
        assert!(RESOLVE.search.contains(&"front matter"));
    }
    #[test]
    fn only_print_acquisition_commands_declare_the_crs_context_fence() {
        for command in [
            &crate::plan_profile::COMMAND,
            &crate::plan_profile_config::COMMAND,
        ] {
            let refusal = command
                .refusals
                .iter()
                .find(|refusal| refusal.code == PROJECT_CRS_CONTEXT_REFUSAL.code)
                .expect("the print resolver's context refusal must be discoverable");
            assert_eq!(refusal.remedy, PROJECT_CRS_CONTEXT_REFUSAL.remedy);
        }
        for command in [&SET, &RESOLVE] {
            assert!(
                !command
                    .refusals
                    .iter()
                    .any(|refusal| { refusal.code == PROJECT_CRS_CONTEXT_REFUSAL.code })
            );
        }
    }
    #[test]
    fn kernel_refusal_is_preserved_with_field_message_key_and_remedy() {
        let error = mv::resolve(&json!({}), "project_a", BTreeMap::new()).unwrap_err();
        let expected = error.clone();
        let failure = failure(error);
        assert_eq!(failure.code(), REFUSAL.code);
        let value = serde_json::to_value(&expected).unwrap();
        assert_eq!(value["message_key"], "mv_print_setup_missing");
        assert_eq!(failure.remedy_text(), Some(expected.remedy.as_str()));
    }
}
