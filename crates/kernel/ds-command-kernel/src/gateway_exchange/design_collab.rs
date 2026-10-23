//! Design collaboration requests: one request of Brain's closed vocabulary on
//! one of its design collaboration surfaces (selections, attachments,
//! annotations, and transformer versions with their revisions), for one
//! project. Brain owns every record, its permissions and every write; this
//! exchange names the route a request belongs to, refuses an action the
//! surface does not serve, and admits the answer's `data` object. Reads may be
//! sent twice within their deadline, writes never.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

pub const INPUT_INVALID: &str = "design_collab_input_invalid";
pub const INVALID: &str = "design_collab_answer_invalid";

/// Brain reads at most 1 MiB of a request on these surfaces.
const REQUEST_MAX_BYTES: usize = 1 << 20;
/// A tag projection or a project's attachment listing.
const RESPONSE_CAP: u64 = 8 * 1024 * 1024;
/// One immutable version snapshot.
const SNAPSHOT_CAP: u64 = 64 * 1024 * 1024;
const DEFAULT_MS: u64 = 30_000;
const DATA_MS: u64 = 120_000;
/// Finalize streams and hashes the stored object server-side.
const FINALIZE_MS: u64 = 600_000;

/// One of Brain's design collaboration surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Selections,
    Attachments,
    Annotations,
    Versions,
}

pub(crate) const SELECTIONS: &[&str] = &[
    "list",
    "get",
    "list_assignments",
    "save",
    "update",
    "archive",
    "promote_task",
];
const SELECTION_READS: &[&str] = &["list", "get", "list_assignments"];

pub(crate) const ATTACHMENTS: &[&str] = &[
    "list",
    "list_project",
    "download",
    "start_upload",
    "finalize_upload",
    "set_latest",
    "archive",
    "archive_revision",
];
const ATTACHMENT_READS: &[&str] = &["list", "list_project", "download"];

pub(crate) const ANNOTATIONS: &[&str] = &[
    "list_tag_definitions",
    "save_tag_definition",
    "archive_tag_definition",
    "list_tags",
    "set_tags",
    "query_tags",
    "list_tag_groups",
    "preview_tag_group",
    "apply_tag_group",
    "unassign_tag_group",
    "export_tag_projection",
    "preview_consumer_grouping",
    "apply_consumer_grouping",
    "read_consumer_grouping",
    "archive_consumer_grouping",
    "preview_location_enrichment",
    "apply_location_enrichment",
    "list_tag_templates",
    "promote_tag_definition",
    "adopt_tag_template",
    "list_threads",
    "get_thread",
    "create_thread",
    "add_comment",
    "resolve_thread",
    "redact_comment",
    "promote_thread",
];
const ANNOTATION_READS: &[&str] = &[
    "list_tag_definitions",
    "list_tags",
    "query_tags",
    "list_tag_groups",
    "preview_tag_group",
    "export_tag_projection",
    "preview_consumer_grouping",
    "read_consumer_grouping",
    "preview_location_enrichment",
    "list_tag_templates",
    "list_threads",
    "get_thread",
];

/// Version actions Brain serves on its design versions route.
pub(crate) const VERSIONS: &[&str] = &[
    "list_versions",
    "get_version",
    "create_version",
    "get_head",
    "get_version_catalog",
    "get_version_revision",
    "bind_version_revision",
    "renumber_version",
];
/// Version actions Brain serves on its data route, keyed by `eds_project_id`.
pub(crate) const REVISIONS: &[&str] = &[
    "get_transformer_version_revision",
    "promote_transformer_version_revision",
    "publish_transformer_version_revision",
    "import_transformer_version",
];
const VERSION_READS: &[&str] = &[
    "list_versions",
    "get_version",
    "get_head",
    "get_version_catalog",
    "get_version_revision",
    "get_transformer_version_revision",
];

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// One request to one design collaboration surface, as Brain reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignCollab {
    pub surface: Surface,
    pub request: Map<String, Value>,
}

/// The export the page calls, and the exchange it runs, for
/// [`crate::capabilities::admit_export`].
pub const EXPORT: &str = "designCollab";
pub const EXCHANGE: &str = "design_collab";

/// The capability one action of one surface performs: every action the
/// surfaces serve names exactly one manifest row, so the page export runs
/// only what a row declares for its platform.
pub fn capability(surface: Surface, action: &str) -> Option<&'static str> {
    Some(match (surface, action) {
        (Surface::Selections, "list") => "design.selection.list",
        (Surface::Selections, "get" | "list_assignments") => "design.selection.read",
        (Surface::Selections, "save" | "update") => "design.selection.save",
        (Surface::Selections, "archive") => "design.selection.archive",
        (Surface::Selections, "promote_task") => "design.selection.assign",
        (Surface::Attachments, "list") => "design.attachment.list",
        (Surface::Attachments, "list_project") => "design.attachment.list-project",
        (Surface::Attachments, "download") => "design.attachment.download",
        (Surface::Attachments, "start_upload" | "finalize_upload") => "design.attachment.publish",
        (Surface::Attachments, "set_latest") => "design.attachment.set-latest",
        (Surface::Attachments, "archive" | "archive_revision") => "design.attachment.retire",
        (Surface::Annotations, "list_tag_definitions") => "design.tag.project-list",
        (Surface::Annotations, "save_tag_definition" | "archive_tag_definition") => {
            "design.tag.define"
        }
        (Surface::Annotations, "list_tags") => "design.tag.list",
        (Surface::Annotations, "set_tags") => "design.tag.set",
        (Surface::Annotations, "query_tags") => "design.tag.query",
        (Surface::Annotations, "list_tag_groups") => "design.group.list",
        (Surface::Annotations, "preview_tag_group") => "design.group.preview",
        (Surface::Annotations, "apply_tag_group") => "design.group.apply",
        (Surface::Annotations, "unassign_tag_group") => "design.group.unassign",
        (Surface::Annotations, "export_tag_projection") => "design.group.export",
        (Surface::Annotations, "preview_consumer_grouping") => "design.consumer-grouping.preview",
        (Surface::Annotations, "apply_consumer_grouping") => "design.consumer-grouping.apply",
        (Surface::Annotations, "read_consumer_grouping") => "design.consumer-grouping.read",
        (Surface::Annotations, "archive_consumer_grouping") => "design.consumer-grouping.archive",
        (Surface::Annotations, "preview_location_enrichment") => "design.tag.enrich-preview",
        (Surface::Annotations, "apply_location_enrichment") => "design.tag.enrich-apply",
        (Surface::Annotations, "list_tag_templates") => "design.tag.template.list",
        (Surface::Annotations, "promote_tag_definition") => "design.tag.template.promote",
        (Surface::Annotations, "adopt_tag_template") => "design.tag.template.adopt",
        (Surface::Annotations, "list_threads") => "design.comment.list",
        (Surface::Annotations, "get_thread") => "design.comment.read",
        (Surface::Annotations, "create_thread" | "add_comment") => "design.comment.post",
        (Surface::Annotations, "resolve_thread") => "design.comment.resolve",
        (Surface::Annotations, "redact_comment") => "design.comment.redact",
        (Surface::Annotations, "promote_thread") => "design.comment.promote",
        (Surface::Versions, "list_versions") => "design.version.list",
        (Surface::Versions, "get_version") => "design.version.show",
        (Surface::Versions, "create_version") => "design.version.begin",
        (Surface::Versions, "get_head") => "design.version.status",
        (Surface::Versions, "get_version_catalog") => "design.version.catalog",
        (Surface::Versions, "get_version_revision" | "get_transformer_version_revision") => {
            "design.version.revision.show"
        }
        (Surface::Versions, "bind_version_revision") => "design.version.revision.bind",
        (Surface::Versions, "renumber_version") => "design.version.renumber",
        (Surface::Versions, "promote_transformer_version_revision") => {
            "design.version.revision.promote"
        }
        (Surface::Versions, "publish_transformer_version_revision") => {
            "design.version.revision.publish"
        }
        (Surface::Versions, "import_transformer_version") => "design.version.import",
        _ => return None,
    })
}

impl DesignCollab {
    fn action(&self) -> &str {
        self.request
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// Admit this request from the page: its capability's row must name
    /// [`EXPORT`] and be supported on this build's platform.
    pub fn admit_in_page(&self, platform: &str) -> Result<&'static str, Refusal> {
        let action = self.action();
        let id = capability(self.surface, action).ok_or_else(|| {
            input_invalid(format!(
                "`{action}` is not an action of design {:?}.",
                self.surface
            ))
        })?;
        crate::capabilities::admit_export_on(platform, id, EXPORT, EXCHANGE)
            .map(|capability| capability.id)
            .map_err(|refused| Refusal::new(refused.code, refused.message))
    }

    /// The route and deadline this action is served on, or `None` when the
    /// surface does not serve it.
    fn route(&self) -> Option<(&'static str, u64, bool)> {
        let action = self.action();
        let (path, actions, reads) = match self.surface {
            Surface::Selections => ("/api/v1/design/selections", SELECTIONS, SELECTION_READS),
            Surface::Attachments => ("/api/v1/design/attachments", ATTACHMENTS, ATTACHMENT_READS),
            Surface::Annotations => ("/api/v1/design/annotations", ANNOTATIONS, ANNOTATION_READS),
            Surface::Versions if REVISIONS.contains(&action) => {
                return Some(("/api/v1/data", DATA_MS, VERSION_READS.contains(&action)));
            }
            Surface::Versions => ("/api/v1/design/versions", VERSIONS, VERSION_READS),
        };
        if !actions.contains(&action) {
            return None;
        }
        let timeout = if action == "finalize_upload" {
            FINALIZE_MS
        } else {
            DEFAULT_MS
        };
        Some((path, timeout, reads.contains(&action)))
    }
}

impl Exchange for DesignCollab {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let action = self.action();
        let (path, timeout, read) = self.route().ok_or_else(|| {
            input_invalid(format!(
                "`{action}` is not an action of design {:?}.",
                self.surface
            ))
        })?;
        let project = self
            .project()
            .ok_or_else(|| input_invalid("A design collaboration request names its project."))?;
        project_reference(project)?;
        // An absent member reaches here as `null` from the page; Brain refuses
        // unknown and null members alike, so it is left out.
        let body = Value::Object(
            self.request
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        if serde_json::to_vec(&body).map_or(true, |bytes| bytes.len() > REQUEST_MAX_BYTES) {
            return Err(input_invalid(
                "A design collaboration request is at most 1 MiB.",
            ));
        }
        let cap = if path == "/api/v1/data" {
            SNAPSHOT_CAP
        } else {
            RESPONSE_CAP
        };
        Ok(Plan {
            request: Request::post(path, body)
                .timeout_ms(timeout)
                .response_cap(cap),
            retries: u32::from(read),
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        match data(&body) {
            answer @ Value::Object(_) => Ok(Admitted::Done(answer.clone())),
            _ => Err(Refusal::new(
                INVALID,
                "The design collaboration service returned an invalid response.",
            )),
        }
    }

    fn project(&self) -> Option<&str> {
        self.request
            .get("project_id")
            .or_else(|| self.request.get("eds_project_id"))
            .and_then(Value::as_str)
    }
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{DesignCollab, Surface};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    /// One request to one design collaboration surface; answers Brain's `data`.
    #[wasm_bindgen(js_name = designCollab, unchecked_return_type = "Record<string, unknown>")]
    pub async fn design_collab(
        host: GatewayHost,
        #[wasm_bindgen(
            unchecked_param_type = "'selections' | 'attachments' | 'annotations' | 'versions'"
        )]
        surface: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] request: JsValue,
    ) -> Result<JsValue, JsValue> {
        let surface: Surface = tsify::serde_wasm_bindgen::from_value(surface)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let request: Map<String, Value> = tsify::serde_wasm_bindgen::from_value(request)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let call = DesignCollab { surface, request };
        call.admit_in_page(crate::capabilities::PLATFORM)
            .map_err(|refusal| crate::gateway_exchange::browser::rejection(&refusal))?;
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "design_collab_tests.rs"]
mod tests;
