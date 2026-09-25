//! Governed model discovery and exact-byte download through the native owner.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::task_geometry::ObjectIndex;
use serde_json::{Value, json};
use std::io::Write;
const LOCAL: Refusal = Refusal {
    code: "grid_project_output_invalid",
    when: "the destination exists or cannot be written, or paging input is invalid",
    remedy: "use a fresh .dsgrid path and a page limit from 1 to 100",
};
const fn refusals() -> [Refusal; 1 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()] {
    let mut r = [LOCAL; 1 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()];
    let mut n = 0;
    while n < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        r[n + 1] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[n];
        n += 1;
    }
    r
}
const REFUSALS: &[Refusal] = &refusals();
const GEOJSON_OUTPUT: Refusal = Refusal {
    code: "grid_geojson_output_invalid",
    when: "the GeoJSON destination exists or cannot be written",
    remedy: "choose a fresh --out path in a writable directory",
};
const GEOJSON_PAGE: Refusal = Refusal {
    code: "grid_geojson_page_invalid",
    when: "--limit is outside 1..5000, --cursor is unknown, or --alignment and --cursor are combined",
    remedy: "use 1..5000 and the exact next_cursor from a prior page; omit --cursor with --alignment",
};
const GEOJSON_ALIGNMENT: Refusal = Refusal {
    code: "grid_geojson_alignment_not_found",
    when: "--alignment is not a routed authored alignment of the exact revision",
    remedy: "omit --alignment to list routed alignment IDs, then use one returned ID",
};
const GEOJSON_RESPONSE: Refusal = Refusal {
    code: "grid_project_response_invalid",
    when: "the project owner returned incomplete or mismatched verified model evidence",
    remedy: "retry once, then update the native client if the exact revision still fails",
};
const fn geojson_refusals() -> [Refusal; 6 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()] {
    let mut r = [GEOJSON_OUTPUT; 6 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()];
    r[1] = GEOJSON_PAGE;
    r[2] = GEOJSON_ALIGNMENT;
    r[3] = GEOJSON_RESPONSE;
    r[4] = Refusal {
        code: "package_decode_failed",
        when: "the verified model bytes cannot be decoded by this grid engine",
        remedy: "update ds to a release compatible with this model revision",
    };
    r[5] = crate::objects::MODEL_CRS_UNSUPPORTED;
    let mut n = 0;
    while n < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        r[n + 6] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[n];
        n += 1;
    }
    r
}
const GEOJSON_REFUSALS: &[Refusal] = &geojson_refusals();
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const PROJECT: Arg =
    Arg::value("project", "<ds-project>", "Exact project for this request.").required();
pub static RETIRE: Command = Command {
    id: "dsgrid.project.retire",
    path: &["dsgrid", "project", "retire"],
    contract: 1,
    summary: "Retire one superseded project DS Grid model (needs --yes).",
    purpose: "Retires the exact model head in the explicitly named project after comparing its revision and digest. Immutable revisions and model bytes remain available for lineage. A changed head is refused.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact project model ID.").required(),
        Arg::value(
            "expected-head",
            "<revision-id>",
            "Head revision inspected before retirement.",
        )
        .required(),
        Arg::value(
            "expected-digest",
            "<sha256>",
            "64-character head model digest inspected before retirement.",
        )
        .required(),
        Arg::value("reason", "<text>", "Why this model is superseded.").required(),
    ],
    output: "The retired model and pinned head, deletion time, and confirmation that immutable revisions and model bytes were retained.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["retire model", "superseded model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static RESTORE: Command = Command {
    id: "dsgrid.project.restore",
    path: &["dsgrid", "project", "restore"],
    contract: 1,
    summary: "Restore one backed-up project DS Grid model (needs --yes).",
    purpose: "Reactivates the exact retired head after the Server verifies its separate backup and immutable model bytes. Attachments remain pinned to their original version. A changed head is refused.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact retired project model ID.").required(),
        Arg::value("expected-head", "<revision-id>", "Retired head revision.").required(),
        Arg::value("expected-digest", "<sha256>", "Retired head digest.").required(),
        Arg::value("reason", "<text>", "Why this model is being restored.").required(),
    ],
    output: "Restored model and pinned head, verified backup identity, and tile invalidation.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["restore model", "recover model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static LIST: Command = Command {
    id: "dsgrid.project.list",
    path: &["dsgrid", "project", "list"],
    contract: 1,
    summary: "List one explicitly named project's saved MV models headlessly.",
    purpose: "Discover governed DS Grid model heads for MV maps and Solar network seeding without a Desktop. Returns exact revision and digest identifiers. Follow next_cursor when more is true, including an empty page. Local unpublished Desktop models are outside this inventory.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("limit", "<1..100>", "Maximum catalog rows scanned.").default("50"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Exact next cursor from the previous page.",
        ),
        Arg::switch(
            "include-deleted",
            "Include retired model heads so they can be restored.",
        ),
    ],
    output: "Selected project, bounded models with head revisions/digests, more and next_cursor.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static VERSIONS: Command = Command {
    id: "dsgrid.project.versions",
    path: &["dsgrid", "project", "versions"],
    contract: 1,
    summary: "List immutable versions of one project DS Grid model.",
    purpose: "Lists exact revision IDs and artifact digests, including versions of a retired model. Attachments remain assigned to their own version; download one exact revision with dsgrid project download.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact project model ID.").required(),
        Arg::value("limit", "<1..100>", "Maximum versions in one page.").default("50"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Exact next cursor from the previous page.",
        ),
    ],
    output: "Bounded immutable versions with revision IDs, model digests, and next cursor.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["model history", "model versions"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static DOWNLOAD: Command = Command {
    id: "dsgrid.project.download",
    path: &["dsgrid", "project", "download"],
    contract: 1,
    summary: "Download and verify one project MV model without a Desktop.",
    purpose: "Resolve an exact governed revision under the selected project, download its immutable .dsgrid bytes and verify the declared SHA-256 and byte count before creating a new local file. Use the resulting package for model inspection, tagged MV quantities and map composition. No storage URL or project override is accepted.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact model ID from project list.").required(),
        Arg::value("revision", "<id>", "Exact immutable revision ID.").required(),
        Arg::value("out", "<file.dsgrid>", "Fresh local package path.").required(),
    ],
    output: "Selected project, model/revision, verified SHA-256, byte count and local path. No signed locator.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static GEOJSON: Command = Command {
    id: "dsgrid.project.geojson",
    path: &["dsgrid", "project", "geojson"],
    contract: 1,
    summary: "Export exact project MV alignments as WGS84 GeoJSON.",
    purpose: "Download and verify one immutable project DS Grid model revision, then export only its routed, authored MV alignments as 2D WGS84 LineStrings. Each feature carries project, governed model/revision, package identity, source digest and alignment ID. Use the GeoJSON with `ds data vector buffer --radius-m 6` for an MV corridor. Page by alignment ID when a model is large; each page writes its own complete GeoJSON file.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact model ID from project list.").required(),
        Arg::value("revision", "<id>", "Exact immutable revision ID.").required(),
        Arg::value("out", "<file.geojson>", "Fresh GeoJSON output path.").required(),
        Arg::value("alignment", "<id>", "Export one exact routed alignment."),
        Arg::value("limit", "<1..5000>", "Maximum alignments in this file.").default("50"),
        Arg::value(
            "cursor",
            "<alignment-id>",
            "Exact next cursor from the previous page.",
        ),
    ],
    output: "GeoJSON path, exact verified source identity, alignment IDs and counts, plus more and next_cursor for omitted alignments. Every file is a valid FeatureCollection.",
    examples: &[],
    refusals: GEOJSON_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "mv alignment",
        "alignment geojson",
        "route corridor",
        "parcel buffer",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn failure(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("grid_project_output_invalid", e.to_string()).remedy(LOCAL.remedy)
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i.require("limit")?.parse::<u16>().map_err(failure)?;
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::List {
            limit,
            cursor: i.value("cursor").map(str::to_owned),
            include_deleted: i.switch("include-deleted"),
        },
    )?;
    Ok(r.data)
}
pub fn versions(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i.require("limit")?.parse::<u16>().map_err(failure)?;
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::ListVersions {
            model: i.require("model")?.into(),
            limit,
            cursor: i.value("cursor").map(str::to_owned),
        },
    )?;
    Ok(r.data)
}
pub fn download(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = std::path::Path::new(i.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(failure("destination already exists"));
    }
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::Download {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
        },
    )?;
    let mut r = r;
    let bytes = r
        .bytes
        .take()
        .ok_or_else(|| failure("verified owner returned no package"))?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(failure)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(failure)?;
    staged.write_all(&bytes).map_err(failure)?;
    staged.as_file().sync_all().map_err(failure)?;
    staged.persist_noclobber(out).map_err(failure)?;
    r.data["out"] = serde_json::json!(out);
    Ok(r.data)
}
pub fn geojson(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = std::path::Path::new(i.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(
            Failure::invalid(GEOJSON_OUTPUT.code, "destination already exists")
                .remedy(GEOJSON_OUTPUT.remedy),
        );
    }
    let limit = i
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=5_000).contains(limit))
        .ok_or_else(|| {
            Failure::invalid(GEOJSON_PAGE.code, "--limit must be in 1..5000")
                .remedy(GEOJSON_PAGE.remedy)
        })?;
    if i.value("alignment").is_some() && i.value("cursor").is_some() {
        return Err(Failure::invalid(
            GEOJSON_PAGE.code,
            "--alignment and --cursor cannot be combined",
        )
        .remedy(GEOJSON_PAGE.remedy));
    }
    let project = i.require("project")?;
    let model = i.require("model")?;
    let revision = i.require("revision")?;
    let mut response = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        project,
        &ds_cli_auth::GridModelsCommand::Download {
            model: model.into(),
            revision: revision.into(),
        },
    )?;
    let evidence = &response.data;
    if evidence["verified"] != true
        || evidence["project"].as_str() != Some(project)
        || evidence["model_id"].as_str() != Some(model)
        || evidence["revision_id"].as_str() != Some(revision)
        || evidence["sha256"]
            .as_str()
            .is_none_or(|sha| sha.len() != 64)
    {
        return Err(Failure::failed(
            GEOJSON_RESPONSE.code,
            "project model evidence did not match the requested immutable revision",
        )
        .remedy(GEOJSON_RESPONSE.remedy));
    }
    let digest = evidence["sha256"].as_str().unwrap().to_owned();
    let bytes = response.bytes.take().ok_or_else(|| {
        Failure::failed(
            GEOJSON_RESPONSE.code,
            "verified owner returned no model bytes",
        )
        .remedy(GEOJSON_RESPONSE.remedy)
    })?;
    let reference = format!("project:{project}:model:{model}:revision:{revision}");
    let index = crate::objects::index_bytes(&reference, &reference, &bytes, None)?;
    let page = geojson_page(
        &index,
        SourceIdentity {
            project,
            model,
            revision,
            digest: &digest,
        },
        i.value("alignment"),
        i.value("cursor"),
        limit,
    )?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    serde_json::to_writer(&mut staged, &page.document).map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    staged.write_all(b"\n").map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    staged.as_file().sync_all().map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    staged.persist_noclobber(out).map_err(|error| {
        Failure::failed(GEOJSON_OUTPUT.code, error.to_string()).remedy(GEOJSON_OUTPUT.remedy)
    })?;
    Ok(json!({
        "project": project,
        "model_id": model,
        "revision_id": revision,
        "sha256": digest,
        "verified": true,
        "package_model_id": index.model_id,
        "package_model_revision": index.model_revision,
        "snapshot_fingerprint": index.fingerprint,
        "source_crs": index.crs,
        "geometry_crs": "EPSG:4326",
        "out": out,
        "total_routed_alignments": page.total,
        "shown": page.alignment_ids.len(),
        "alignment_ids": page.alignment_ids,
        "more": page.next_cursor.is_some(),
        "next_cursor": page.next_cursor
    }))
}

struct SourceIdentity<'a> {
    project: &'a str,
    model: &'a str,
    revision: &'a str,
    digest: &'a str,
}

struct GeoJsonPage {
    document: Value,
    total: usize,
    alignment_ids: Vec<String>,
    next_cursor: Option<String>,
}

fn geojson_page(
    index: &ObjectIndex,
    source: SourceIdentity<'_>,
    alignment: Option<&str>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<GeoJsonPage, Failure> {
    let mut routed: Vec<_> = index.alignments.iter().collect();
    routed.sort_by(|a, b| a.id.cmp(&b.id));
    let total = routed.len();
    let selected: Vec<_> = if let Some(id) = alignment {
        vec![*routed.iter().find(|a| a.id == id).ok_or_else(|| {
            Failure::invalid(
                GEOJSON_ALIGNMENT.code,
                format!("`{id}` is not a routed alignment of this revision"),
            )
            .remedy(GEOJSON_ALIGNMENT.remedy)
        })?]
    } else {
        let start = match cursor {
            Some(id) => routed.iter().position(|a| a.id == id).ok_or_else(|| {
                Failure::invalid(
                    GEOJSON_PAGE.code,
                    format!("`{id}` is not an alignment cursor of this revision"),
                )
                .remedy(GEOJSON_PAGE.remedy)
            })?,
            None => 0,
        };
        routed.iter().skip(start).take(limit).copied().collect()
    };
    let next_cursor = if alignment.is_none() {
        let start = cursor
            .and_then(|id| routed.iter().position(|a| a.id == id))
            .unwrap_or(0);
        routed.get(start + selected.len()).map(|a| a.id.clone())
    } else {
        None
    };
    let alignment_ids = selected.iter().map(|a| a.id.clone()).collect();
    let features: Vec<Value> = selected
        .iter()
        .map(|alignment| {
            let coordinates: Vec<_> = alignment
                .vertices
                .iter()
                .map(|vertex| json!([vertex[0], vertex[1]]))
                .collect();
            json!({
                "type": "Feature",
                "id": alignment.id,
                "geometry": {"type": "LineString", "coordinates": coordinates},
                "properties": {
                    "project": source.project,
                    "project_model_id": source.model,
                    "project_revision_id": source.revision,
                    "package_model_id": index.model_id,
                    "package_model_revision": index.model_revision,
                    "snapshot_fingerprint": index.fingerprint,
                    "package_sha256": source.digest,
                    "alignment_id": alignment.id,
                    "alignment_label": alignment.label,
                    "route_length_m": alignment.vertices.last().map(|vertex| vertex[2]),
                    "feature_kind": "designed_mv_alignment",
                    "voltage_class": "MV",
                    "source_crs": index.crs,
                    "geometry_crs": "EPSG:4326"
                }
            })
        })
        .collect();
    Ok(GeoJsonPage {
        document: json!({"type": "FeatureCollection", "features": features}),
        total,
        alignment_ids,
        next_cursor,
    })
}
pub fn retire(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = i.require("project")?;
    let receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        project,
        &ds_cli_auth::GridModelsCommand::Delete {
            model: i.require("model")?.into(),
            expected_revision: i.require("expected-head")?.into(),
            expected_digest: i.require("expected-digest")?.into(),
            reason: i.require("reason")?.into(),
        },
    )?;
    Ok(receipt.data)
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::Restore {
            model: i.require("model")?.into(),
            expected_revision: i.require("expected-head")?.into(),
            expected_digest: i.require("expected-digest")?.into(),
            reason: i.require("reason")?.into(),
        },
    )?;
    Ok(receipt.data)
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_index() -> ObjectIndex {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let bytes = std::fs::read(&path).expect("real grid fixture");
        crate::objects::index_bytes("fixture", &path.display().to_string(), &bytes, None)
            .expect("index through the same in-memory path as project export")
    }

    fn identity() -> SourceIdentity<'static> {
        SourceIdentity {
            project: "project-1",
            model: "model-1",
            revision: "revision-1",
            digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        }
    }

    #[test]
    fn authored_alignment_geojson_is_two_dimensional_wgs84_with_exact_source() {
        let index = fixture_index();
        let page = geojson_page(&index, identity(), None, None, 50).expect("page");
        assert_eq!(page.total, 1);
        assert_eq!(page.alignment_ids, vec!["aln-e9bc0619ae359761-1"]);
        assert!(page.next_cursor.is_none());
        let feature = &page.document["features"][0];
        assert_eq!(feature["geometry"]["type"], "LineString");
        let points = feature["geometry"]["coordinates"].as_array().unwrap();
        assert_eq!(points.len(), 3);
        for point in points {
            let coordinates = point.as_array().unwrap();
            assert_eq!(
                coordinates.len(),
                2,
                "chainage must not become GeoJSON altitude"
            );
            let lon = coordinates[0].as_f64().unwrap();
            let lat = coordinates[1].as_f64().unwrap();
            assert!((26.0..28.0).contains(&lon) && (-90.0..0.0).contains(&lat));
        }
        assert_eq!(feature["properties"]["project"], "project-1");
        assert_eq!(feature["properties"]["project_revision_id"], "revision-1");
        assert_eq!(feature["properties"]["geometry_crs"], "EPSG:4326");
        assert_eq!(feature["properties"]["route_length_m"], 400.0);
        assert_eq!(feature["properties"]["package_model_id"], index.model_id);
    }

    #[test]
    fn alignment_pages_resume_at_the_first_omitted_id_without_duplication() {
        let mut index = fixture_index();
        for id in ["alignment-b", "alignment-a"] {
            let mut another = index.alignments[0].clone();
            another.id = id.into();
            index.alignments.push(another);
        }
        let first = geojson_page(&index, identity(), None, None, 2).expect("first page");
        assert_eq!(first.alignment_ids, vec!["alignment-a", "alignment-b"]);
        assert_eq!(first.next_cursor.as_deref(), Some("aln-e9bc0619ae359761-1"));
        let second = geojson_page(&index, identity(), None, first.next_cursor.as_deref(), 2)
            .expect("second page");
        assert_eq!(second.alignment_ids, vec!["aln-e9bc0619ae359761-1"]);
        assert!(second.next_cursor.is_none());
        assert_eq!(
            geojson_page(&index, identity(), Some("missing"), None, 1)
                .err()
                .unwrap()
                .code(),
            GEOJSON_ALIGNMENT.code
        );
        assert_eq!(
            geojson_page(&index, identity(), None, Some("missing"), 1)
                .err()
                .unwrap()
                .code(),
            GEOJSON_PAGE.code
        );
    }
}
