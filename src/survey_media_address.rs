//! Which stored object a survey media reference names, and how a host must
//! address it on the wire.
//!
//! Three layouts reach a DS project's survey media, because three writers
//! produced them: the browser's own five-segment reference
//! (`{project}/forms/{form}/{entry}/{file}`), the six-segment reference under
//! a `projects/` root that ds-web's fixtures carry, and the field app's
//! seven-segment reference with a per-field folder
//! (`projects/{p}/forms/{f}/{e}/{field}/{file}`, `entry_engine.dart`). Until
//! this module existed, ds-web's `surveyMediaAddress` admitted only the first
//! and ds-brain's `surveyMediaPath` composed only the first, so a photo taken
//! on a phone could be shown but never inspected, rotated or replaced.
//!
//! This module owns the whole grammar: which of the three layouts a reference
//! is, the project/form/entry/field/file it identifies, the object path it
//! resolves to, the thumbnail companion beside it, and the request body a host
//! sends to ds-brain's `survey_media` actions. Hosts marshal; no TypeScript,
//! Go or Dart re-derives any of it. Segment rules are transcribed from
//! ds-brain (`surveyMediaSegment`, `surveyMediaFileName`), so a reference this
//! module admits is one the gateway can compose, and a reference it refuses is
//! refused in the browser's existing words.
//!
//! The thumbnail is never derived here: `survey_photo_outbox::thumbnail_path_for`
//! already owns that rule for the outbox and the `surveyPhotoThumbnailPath`
//! export, and this module calls it.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::survey_photo_outbox::thumbnail_path_for;

pub const SCHEMA: &str = "ds.survey-media-address/v1";

/// Why a reference is not an address. The vocabulary is exactly one sentence
/// — the browser's existing words — so every host keeps the string it already
/// shows and no test or translation moves.
pub const REFUSAL: &str = "Media path must identify a project, form, entry and file";

/// ds-brain `surveyMediaSegment`: `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`, plus
/// its explicit rejection of any segment containing `..`.
pub const MAX_SEGMENT_CHARS: usize = 128;
/// ds-brain `surveyMediaFileName`: `^[A-Za-z0-9_.-]{1,255}$`, never `.` or
/// `..`. A filename's first character may be `_`, `.` or `-`: ds-web's
/// `safeMediaFileName` writes a field prefix that begins with `_`.
pub const MAX_FILE_NAME_CHARS: usize = 255;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// The literal folder every layout puts between a project and its forms.
const FORMS: &str = "forms";
/// The literal root the field app and ds-web's fixtures carry.
const PROJECTS: &str = "projects";

/// Which of the three shapes a reference is written in. The layout is part of
/// the address, not a guess a host repeats: ds-brain recomposes the path from
/// it rather than trusting a client-sent string.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// `{project}/forms/{form}/{entry}/{file}` — what the browser has always
    /// uploaded, and the only shape ds-brain composed before this contract.
    Web,
    /// `projects/{project}/forms/{form}/{entry}/{file}`.
    Projects,
    /// `projects/{project}/forms/{form}/{entry}/{field}/{file}` — the field
    /// app's, one folder per media field.
    FieldApp,
}

impl Layout {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Projects => "projects",
            Self::FieldApp => "field_app",
        }
    }
}

/// One survey media reference, resolved. `object_path` is the reference
/// itself — normalised only of a leading slash — because the stored object's
/// name is the host's evidence, never something this module rewrites.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SurveyMediaAddress {
    pub layout: Layout,
    pub project: String,
    pub form: String,
    pub entry: String,
    /// The media field's folder. Present for `field_app` only; the other two
    /// layouts store a file directly under its entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub file: String,
    pub object_path: String,
    /// The JPEG companion beside the original, from
    /// `survey_photo_outbox::thumbnail_path_for`. A reference that already
    /// names a thumbnail is its own companion.
    pub thumbnail_object_path: String,
}

/// ds-brain's segment rule, transcribed.
fn valid_segment(value: &str) -> bool {
    value.len() <= MAX_SEGMENT_CHARS
        && value.starts_with(|first: char| first.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !value.contains("..")
}

/// ds-brain's filename rule, transcribed.
fn valid_file_name(value: &str) -> bool {
    (1..=MAX_FILE_NAME_CHARS).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && value != "."
        && value != ".."
}

fn file_name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Which stored object this reference names, or the one refusal.
///
/// Every segment must be one the gateway can compose, so a reference admitted
/// here cannot be refused by ds-brain afterwards — including the thumbnail
/// companion, which must itself be a bounded object name or the reference
/// names an original whose thumbnail could never exist.
pub fn parse(path: &str) -> Result<SurveyMediaAddress, String> {
    let object_path = path.strip_prefix('/').unwrap_or(path);
    let parts: Vec<&str> = object_path.split('/').collect();
    let (layout, project, form, entry, field, file) = match parts.as_slice() {
        [project, forms, form, entry, file] if *forms == FORMS => {
            (Layout::Web, *project, *form, *entry, None, *file)
        }
        [projects, project, forms, form, entry, file]
            if *projects == PROJECTS && *forms == FORMS =>
        {
            (Layout::Projects, *project, *form, *entry, None, *file)
        }
        [projects, project, forms, form, entry, field, file]
            if *projects == PROJECTS && *forms == FORMS =>
        {
            (
                Layout::FieldApp,
                *project,
                *form,
                *entry,
                Some(*field),
                *file,
            )
        }
        _ => return Err(REFUSAL.to_owned()),
    };
    if !valid_segment(project)
        || !valid_segment(form)
        || !valid_segment(entry)
        || field.is_some_and(|field| !valid_segment(field))
        || !valid_file_name(file)
    {
        return Err(REFUSAL.to_owned());
    }
    let thumbnail_object_path = thumbnail_path_for(object_path).map_err(|_| REFUSAL.to_owned())?;
    if !valid_file_name(file_name_of(&thumbnail_object_path)) {
        return Err(REFUSAL.to_owned());
    }
    Ok(SurveyMediaAddress {
        layout,
        project: project.to_owned(),
        form: form.to_owned(),
        entry: entry.to_owned(),
        field: field.map(str::to_owned),
        file: file.to_owned(),
        object_path: object_path.to_owned(),
        thumbnail_object_path,
    })
}

/// The body a host sends to ds-brain's `survey_media` actions for this
/// address. `layout` and `media_field` are omitted for the web layout, so
/// today's requests stay byte-identical and an old gateway keeps serving them.
pub fn wire(address: &SurveyMediaAddress) -> Value {
    let mut body = json!({
        "project_id": address.project,
        "form_slug": address.form,
        "entry_id": address.entry,
        "file_name": address.file,
    });
    if address.layout != Layout::Web {
        body["layout"] = json!(address.layout.wire());
        if let Some(field) = &address.field {
            body["media_field"] = json!(field);
        }
    }
    body
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// Which stored object this reference names.
    Parse { path: String },
    /// The request body ds-brain's survey_media actions are asked with.
    Wire { path: String },
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("survey media address request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid survey media address request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported survey media address schema; expected {SCHEMA}"
        ));
    }
    let value = match request.op {
        Op::Parse { path } => {
            serde_json::to_value(parse(&path)?).map_err(|error| error.to_string())?
        }
        Op::Wire { path } => wire(&parse(&path)?),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(path: &str) -> SurveyMediaAddress {
        parse(path).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    fn ask(op: Value) -> Result<Value, String> {
        let request = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes");
        evaluate(&request).map(|answer| serde_json::from_str(&answer).expect("answers JSON"))
    }

    #[test]
    fn the_browsers_own_five_segment_reference_is_the_web_layout() {
        let parsed = address("p1/forms/poles/e1/photo_1.jpg");
        assert_eq!(parsed.layout, Layout::Web);
        assert_eq!(parsed.project, "p1");
        assert_eq!(parsed.form, "poles");
        assert_eq!(parsed.entry, "e1");
        assert_eq!(parsed.field, None);
        assert_eq!(parsed.file, "photo_1.jpg");
        assert_eq!(parsed.object_path, "p1/forms/poles/e1/photo_1.jpg");
        assert_eq!(
            parsed.thumbnail_object_path,
            "p1/forms/poles/e1/photo_1_thunder.jpeg"
        );
    }

    #[test]
    fn a_projects_root_is_the_projects_layout() {
        let parsed = address("projects/p1/forms/poles/e1/photo_1.jpg");
        assert_eq!(parsed.layout, Layout::Projects);
        assert_eq!(parsed.project, "p1");
        assert_eq!(parsed.form, "poles");
        assert_eq!(parsed.entry, "e1");
        assert_eq!(parsed.field, None);
        assert_eq!(
            parsed.thumbnail_object_path,
            "projects/p1/forms/poles/e1/photo_1_thunder.jpeg"
        );
    }

    #[test]
    fn the_field_apps_per_field_folder_is_the_field_app_layout() {
        let parsed = address("projects/p1/forms/poles/e1/photo_1/1700000000000_0.jpg");
        assert_eq!(parsed.layout, Layout::FieldApp);
        assert_eq!(parsed.project, "p1");
        assert_eq!(parsed.form, "poles");
        assert_eq!(parsed.entry, "e1");
        assert_eq!(parsed.field.as_deref(), Some("photo_1"));
        assert_eq!(parsed.file, "1700000000000_0.jpg");
        // The companion stays beside the original, inside the field folder.
        assert_eq!(
            parsed.thumbnail_object_path,
            "projects/p1/forms/poles/e1/photo_1/1700000000000_0_thunder.jpeg"
        );
    }

    #[test]
    fn a_thumbnail_reference_is_its_own_companion_and_a_leading_slash_is_the_only_normalisation() {
        let parsed = address("projects/p1/forms/poles/e1/photo_1/1700000000000_0_thunder.jpeg");
        assert_eq!(parsed.object_path, parsed.thumbnail_object_path);
        let slashed = address("/p1/forms/poles/e1/photo_1.jpg");
        assert_eq!(slashed, address("p1/forms/poles/e1/photo_1.jpg"));
        // A file without an extension still has a companion beside it.
        assert_eq!(
            address("p1/forms/poles/e1/capture").thumbnail_object_path,
            "p1/forms/poles/e1/capture_thunder.jpeg"
        );
        // A field folder may carry a dot; only `..` is traversal.
        assert_eq!(
            address("projects/p1/forms/poles/e1/photo.front/1700000000000_0.jpg")
                .field
                .as_deref(),
            Some("photo.front")
        );
    }

    #[test]
    fn every_refusal_is_the_one_sentence_the_browser_already_shows() {
        for refused in [
            // Not one of the three layouts.
            "",
            "p1/forms/poles/e1",
            "p1/forms/poles/e1/photo_1/1700000000000_0.jpg",
            "projects/p1/forms/poles/e1/photo_1/extra/1700000000000_0.jpg",
            "p1/form/poles/e1/photo_1.jpg",
            "prjcts/p1/forms/poles/e1/photo_1.jpg",
            // A bucket-qualified object name: hosts strip the bucket first.
            "ds-survey-media/p1/forms/poles/e1/photo_1.jpg",
            "ds-survey-media/projects/p1/forms/poles/e1/photo_1/1700000000000_0.jpg",
            // Segments the gateway would refuse.
            "p1/forms/../e1/photo_1.jpg",
            "p1/forms/poles/e1/photo%201.jpg",
            "p1\\forms\\poles\\e1\\photo_1.jpg",
            "p1/forms//e1/photo_1.jpg",
            "p1/forms/poles/e1/",
            "p1/forms/_poles/e1/photo_1.jpg",
            "p1/forms/poles/e1/photo_1.jpg?generation=1",
            "p1/forms/poles/e1/photo_1.jpg#top",
            "p1/forms/poles/e1/photo\u{1}.jpg",
            "p1/forms/poles/e1/.",
            "p1/forms/poles/e1/..",
            "projects/p1/forms/poles/e1/_field/1700000000000_0.jpg",
            "projects/p1/forms/poles/e1/ph..oto/1700000000000_0.jpg",
        ] {
            assert_eq!(parse(refused), Err(REFUSAL.to_owned()), "{refused:?}");
        }
        // Bounded: 129 characters in a segment, and a filename whose companion
        // would not fit inside the gateway's own filename bound.
        let over = "a".repeat(MAX_SEGMENT_CHARS + 1);
        assert!(parse(&format!("p1/forms/{over}/e1/photo_1.jpg")).is_err());
        assert!(parse(&format!(
            "p1/forms/{}/e1/photo_1.jpg",
            "a".repeat(MAX_SEGMENT_CHARS)
        ))
        .is_ok());
        let stem = "a".repeat(MAX_FILE_NAME_CHARS - "_thunder.jpeg".len());
        assert_eq!(
            parse(&format!("p1/forms/poles/e1/{stem}.jpg"))
                .unwrap()
                .thumbnail_object_path
                .len(),
            "p1/forms/poles/e1/".len() + MAX_FILE_NAME_CHARS
        );
        assert!(parse(&format!("p1/forms/poles/e1/{stem}a.jpg")).is_err());
    }

    #[test]
    fn the_web_layouts_wire_body_is_todays_body_and_the_others_name_themselves() {
        assert_eq!(
            wire(&address("p1/forms/poles/e1/photo_1.jpg")),
            json!({"project_id": "p1", "form_slug": "poles", "entry_id": "e1", "file_name": "photo_1.jpg"})
        );
        assert_eq!(
            wire(&address("projects/p1/forms/poles/e1/photo_1.jpg")),
            json!({"project_id": "p1", "form_slug": "poles", "entry_id": "e1", "file_name": "photo_1.jpg", "layout": "projects"})
        );
        assert_eq!(
            wire(&address(
                "projects/p1/forms/poles/e1/photo_1/1700000000000_0.jpg"
            )),
            json!({"project_id": "p1", "form_slug": "poles", "entry_id": "e1", "file_name": "1700000000000_0.jpg", "layout": "field_app", "media_field": "photo_1"})
        );
    }

    #[test]
    fn evaluate_answers_both_ops_and_refuses_a_malformed_request() {
        let parsed = ask(json!({"kind": "parse", "path": "projects/p1/forms/poles/e1/x.jpg"}))
            .expect("parsed");
        assert_eq!(parsed["layout"], "projects");
        assert_eq!(parsed["object_path"], "projects/p1/forms/poles/e1/x.jpg");
        assert_eq!(
            parsed["thumbnail_object_path"],
            "projects/p1/forms/poles/e1/x_thunder.jpeg"
        );
        assert!(parsed.get("field").is_none());
        assert_eq!(
            ask(json!({"kind": "wire", "path": "p1/forms/poles/e1/x.jpg"})).expect("wired"),
            json!({"project_id": "p1", "form_slug": "poles", "entry_id": "e1", "file_name": "x.jpg"})
        );
        // Both ops refuse in the same words.
        assert_eq!(
            ask(json!({"kind": "wire", "path": "p1/forms/poles/e1"})),
            Err(REFUSAL.to_owned())
        );
        assert_eq!(
            ask(json!({"kind": "parse", "path": "p1/forms/poles/e1"})),
            Err(REFUSAL.to_owned())
        );
        assert!(evaluate(b"{").is_err());
        assert!(ask(json!({"kind": "compose", "path": "p1/forms/poles/e1/x.jpg"})).is_err());
        assert!(
            ask(json!({"kind": "parse", "path": "p1/forms/poles/e1/x.jpg", "extra": 1})).is_err()
        );
        assert!(evaluate(
            &serde_json::to_vec(&json!({"schema": "ds.other/v1", "op": {"kind": "parse", "path": "p1/forms/poles/e1/x.jpg"}}))
                .unwrap()
        )
        .is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }
}
