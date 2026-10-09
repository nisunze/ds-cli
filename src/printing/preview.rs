//! May a live transformer preview be produced right now, and how it is shown.
//!
//! The editor reports facts — the draft, the transformer it chose (as the
//! printable inventory described it), which governed print styles it holds,
//! whether it is online, whether its native reporter is ready and how many
//! device pixels one paper millimetre currently spans — and receives one
//! plan: ready or the refusals by message key, the page the engine will
//! produce, and the raster the host should draw it at. Rendering is a host
//! effect (`report_export::preview` is the recipe both hosts run); the page
//! itself is the engine's SVG, which the host rasterises for its canvas at
//! the size decided here and re-rasterises, without the engine, when the
//! zoom changes.
//!
//! `room_admission` is the second decision: whether the held room the host
//! reads for that transformer is one the engine may print, and which server
//! version it carries. The host hands the room's facts as it found them;
//! this module never fetches a missing room.
use super::catalogue::Purpose;
use super::Layout;
use crate::report_formats::PrintArtifactFormat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The transformer the editor chose, as `printable_transformers` listed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transformer {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// Whether this device holds the room; `None` when unknown.
    #[serde(default)]
    pub cached: Option<bool>,
    /// Whether the held room carries unsaved edits; `None` when unknown.
    #[serde(default)]
    pub dirty: Option<bool>,
}

/// What a preview renders the template against. The layout's purpose decides
/// which kind it must be (`expected_subject`): one printable LV transformer,
/// one or more DS Grid MV models, or a map extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "subject", rename_all = "snake_case", deny_unknown_fields)]
pub enum Subject {
    /// One transformer, as `printable_transformers` listed it.
    Transformer {
        name: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        cached: Option<bool>,
        #[serde(default)]
        dirty: Option<bool>,
    },
    /// The DS Grid MV models the sheet prints, in the order chosen.
    MvModels { model_ids: Vec<String> },
    /// The project extent, or an explicit WGS84 `[west, south, east, north]`.
    Extent {
        #[serde(default)]
        bbox: Option<[f64; 4]>,
    },
}

/// The kind of subject a template previews against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    Transformer,
    MvModels,
    Extent,
}

impl SubjectKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Transformer => "transformer",
            Self::MvModels => "mv_models",
            Self::Extent => "extent",
        }
    }
    /// The purpose's subject: MV sheets and front matter preview against MV
    /// models, overview sheets against an extent, LV sheets and voltage-drop
    /// reports against one transformer.
    pub fn of(purpose: Purpose) -> Self {
        match purpose {
            Purpose::MvModel | Purpose::FrontMatter => Self::MvModels,
            Purpose::Project => Self::Extent,
            Purpose::LvTransformer | Purpose::VoltageDrop => Self::Transformer,
        }
    }
    /// The canonical command that renders a preview for this subject.
    pub fn render_command(self) -> Option<&'static str> {
        match self {
            Self::Transformer => Some(TRANSFORMER_PREVIEW_COMMAND),
            Self::MvModels => Some(super::mv_preview::COMMAND_ID),
            Self::Extent => None,
        }
    }
}

impl Subject {
    pub fn kind(&self) -> SubjectKind {
        match self {
            Self::Transformer { .. } => SubjectKind::Transformer,
            Self::MvModels { .. } => SubjectKind::MvModels,
            Self::Extent { .. } => SubjectKind::Extent,
        }
    }
}

/// The canonical command that previews a layout against one transformer.
pub const TRANSFORMER_PREVIEW_COMMAND: &str = "report.project.export";
/// The most MV models one preview names.
pub const MAX_PREVIEW_MODELS: usize = 64;

/// The subject `layout` previews against. Its classification decides; an
/// unclassified layout keeps the legacy reading: an MV plan-and-profile setup
/// previews against MV models, anything else against a transformer.
pub fn expected_subject(layout: &Layout) -> SubjectKind {
    match layout.classification {
        Some(classification) => SubjectKind::of(classification.purpose),
        None if layout.mv.is_some() => SubjectKind::MvModels,
        None => SubjectKind::Transformer,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub layout: Layout,
    /// The legacy transformer fact; read as a transformer `subject` when no
    /// subject is given.
    #[serde(default)]
    pub transformer: Option<Transformer>,
    /// What the preview renders against; it must be the layout's `expected_subject`.
    #[serde(default)]
    pub subject: Option<Subject>,
    #[serde(default)]
    pub available_style_refs: Vec<String>,
    #[serde(default)]
    pub online: bool,
    /// The host can run the report engine locally (desktop-native placement).
    #[serde(default)]
    pub native_ready: bool,
    /// Device pixels per paper millimetre at the editor's current zoom; the
    /// raster is chosen to match it, so the page is sharp at the zoom it is
    /// looked at and no larger than the budget allows.
    #[serde(default)]
    pub screen_px_per_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refusal {
    pub code: String,
    pub message_key: String,
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, String>,
}

/// The bitmap a host draws the page at: whole device pixels, and the scale
/// the SVG's millimetre units are multiplied by to reach them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Raster {
    pub width_px: u32,
    pub height_px: u32,
    pub px_per_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    pub ready: bool,
    pub refusals: Vec<Refusal>,
    /// Facts the host should show beside a ready preview, by message key.
    pub notes: Vec<Refusal>,
    pub transformer: Option<String>,
    pub output_id: String,
    pub format: PrintArtifactFormat,
    pub raster: Raster,
    /// Every governed print style the render needs, in layout order.
    pub print_style_refs: Vec<String>,
    /// What this layout previews against, decided by its purpose.
    pub subject: SubjectKind,
    /// The canonical command that renders that preview, when one exists.
    pub render_command: Option<String>,
}

/// The least a page is rasterised at: legible on a screen when the whole
/// sheet is fitted to a small window.
pub const MIN_PX_PER_MM: f64 = 1.5;
/// The most: the delivery raster (300 dpi) — nothing is gained past it.
pub const MAX_PX_PER_MM: f64 = 300. / 25.4;
/// The bitmap budget a webview holds comfortably; an A0 at 300 dpi would be
/// 140 megapixels, which no preview needs.
pub const MAX_RASTER_PIXELS: f64 = 40_000_000.;

/// The raster for a page of `page_mm` looked at through `screen_px_per_mm`:
/// the screen scale within bounds, then shrunk to the pixel budget.
pub fn raster(page_mm: [f64; 2], screen_px_per_mm: Option<f64>) -> Raster {
    let wanted = match screen_px_per_mm {
        Some(scale) if scale.is_finite() && scale > 0. => scale,
        _ => 96. / 25.4,
    }
    .clamp(MIN_PX_PER_MM, MAX_PX_PER_MM);
    let area = page_mm[0] * page_mm[1];
    let budget = (MAX_RASTER_PIXELS / area).sqrt();
    let px_per_mm = wanted.min(budget).max(MIN_PX_PER_MM);
    Raster {
        width_px: (page_mm[0] * px_per_mm).round().max(1.) as u32,
        height_px: (page_mm[1] * px_per_mm).round().max(1.) as u32,
        px_per_mm,
    }
}

/// One keyed refusal with its message parameters, as every printing
/// decision reports it.
pub(super) fn refusal(code: &str, key: &str, params: &[(&str, &str)]) -> Refusal {
    Refusal {
        code: code.into(),
        message_key: key.into(),
        params: params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

pub fn plan(facts: &Facts) -> Result<Plan, String> {
    super::validate(&facts.layout)?;
    let mut refusals = Vec::new();
    let mut notes = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    for reference in facts.layout.style_refs.values() {
        if !refs.contains(reference) {
            refs.push(reference.clone());
        }
        if !facts.available_style_refs.contains(reference) {
            refusals.push(refusal(
                "print_style_unavailable",
                "printing_style_unavailable",
                &[("reference", reference)],
            ));
        }
    }
    let expected = expected_subject(&facts.layout);
    // The legacy `transformer` fact is a transformer subject.
    let subject = facts.subject.clone().or_else(|| {
        facts.transformer.as_ref().map(|t| Subject::Transformer {
            name: t.name.clone(),
            kind: t.kind.clone(),
            cached: t.cached,
            dirty: t.dirty,
        })
    });
    match &subject {
        None => refusals.push(match expected {
            SubjectKind::Transformer => refusal(
                "print_preview_no_transformer",
                "printing_preview_no_transformer",
                &[],
            ),
            SubjectKind::MvModels => refusal(
                "print_preview_no_mv_model",
                "printing_preview_no_mv_model",
                &[],
            ),
            SubjectKind::Extent => {
                refusal("print_preview_no_extent", "printing_preview_no_extent", &[])
            }
        }),
        Some(chosen) if chosen.kind() != expected => refusals.push(refusal(
            "print_preview_subject_mismatch",
            "printing_preview_subject_mismatch",
            &[
                ("expected", expected.key()),
                ("subject", chosen.kind().key()),
            ],
        )),
        Some(Subject::Transformer {
            name,
            kind,
            cached,
            dirty,
        }) => {
            if crate::report_export::reportable_transformer(name).is_err()
                || !matches!(kind.as_str(), "" | "individual" | "transformer")
            {
                refusals.push(refusal(
                    "print_preview_not_a_transformer",
                    "printing_preview_not_a_transformer",
                    &[("name", name)],
                ));
            }
            // A preview reads the room this device holds; it never downloads
            // one, online or not. Holding a room is the Design/Status page's
            // download action, so the remedy is to download the design first.
            if *cached == Some(false) {
                refusals.push(refusal(
                    "print_preview_room_unavailable",
                    "printing_preview_room_unavailable",
                    &[("name", name)],
                ));
            }
            if *dirty == Some(true) {
                notes.push(refusal(
                    "print_preview_dirty_room",
                    "printing_preview_dirty_room",
                    &[("name", name)],
                ));
            }
        }
        Some(Subject::MvModels { model_ids }) => {
            let mut seen = std::collections::BTreeSet::new();
            if model_ids.is_empty() {
                refusals.push(refusal(
                    "print_preview_no_mv_model",
                    "printing_preview_no_mv_model",
                    &[],
                ));
            } else if model_ids.len() > MAX_PREVIEW_MODELS
                || model_ids.iter().any(|id| {
                    id.is_empty()
                        || id.len() > 200
                        || id.chars().any(char::is_control)
                        || !seen.insert(id)
                })
            {
                refusals.push(refusal(
                    "print_preview_mv_model_invalid",
                    "printing_preview_mv_model_invalid",
                    &[],
                ));
            }
        }
        Some(Subject::Extent { bbox }) => {
            let valid = bbox.is_none_or(|[w, s, e, n]| {
                [w, s, e, n].iter().all(|v| v.is_finite())
                    && (-180. ..=180.).contains(&w)
                    && (-180. ..=180.).contains(&e)
                    && (-90. ..=90.).contains(&s)
                    && (-90. ..=90.).contains(&n)
                    && w < e
                    && s < n
            });
            refusals.push(if valid {
                // No native renderer previews an extent yet; say so rather
                // than render the sheet around some transformer.
                refusal(
                    "print_preview_extent_unavailable",
                    "printing_preview_extent_unavailable",
                    &[],
                )
            } else {
                refusal(
                    "print_preview_extent_invalid",
                    "printing_preview_extent_invalid",
                    &[],
                )
            });
        }
    }
    if !facts.native_ready {
        refusals.push(refusal(
            "print_preview_native_unavailable",
            "printing_preview_native_unavailable",
            &[],
        ));
    }
    Ok(Plan {
        ready: refusals.is_empty(),
        refusals,
        notes,
        transformer: match &subject {
            Some(Subject::Transformer { name, .. }) => Some(name.clone()),
            _ => None,
        },
        output_id: format!("svg__{}", facts.layout.id),
        format: crate::report_export::preview::FORMAT,
        raster: raster(facts.layout.page_mm, facts.screen_px_per_mm),
        print_style_refs: refs,
        subject: expected,
        render_command: expected.render_command().map(str::to_owned),
    })
}

pub fn evaluate(facts: Facts) -> Result<Value, String> {
    serde_json::to_value(plan(&facts)?).map_err(|e| e.to_string())
}

/// What the host found when it read the held room for one transformer:
/// the row as the local design store keeps it, reduced to the facts the
/// admission reads. `present: false` means no row at all; the other fields
/// are then ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoomFacts {
    #[serde(default)]
    pub present: bool,
    /// The room's origin; only an `lv` room is a printable design.
    #[serde(default)]
    pub source: Option<String>,
    /// The transformer the row says it holds.
    #[serde(default)]
    pub transformer_name: Option<String>,
    /// How many layers the room carries; an empty room has nothing to print.
    #[serde(default)]
    pub layer_count: usize,
    /// The room carries local edits that are not a complete design.
    #[serde(default)]
    pub local_dirty: bool,
    /// The saved design's version as the row stores it (a JSON number);
    /// anything but a safe positive integer reads as 0.
    #[serde(default)]
    pub server_version: Option<f64>,
    /// When the row was last written; a room with no finite timestamp was
    /// never completely stored.
    #[serde(default)]
    pub updated_at: Option<f64>,
}

/// A held room the engine may print, with the version the request names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AdmittedRoom {
    /// The saved design's version, or 0 when the row carries none.
    pub server_version: i64,
}

/// The largest integer JavaScript stores exactly; a version beyond it is not
/// one a browser could have written.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.;

/// A stored server version as the request carries it: a safe positive
/// integer, else 0.
fn admitted_server_version(stored: Option<f64>) -> i64 {
    match stored {
        Some(v) if v.is_finite() && v > 0. && v.fract() == 0. && v <= MAX_SAFE_INTEGER => v as i64,
        _ => 0,
    }
}

/// Whether the held room is a complete `lv` design for `transformer`: the
/// row exists, names this transformer, was completely stored and carries at
/// least one layer.
fn usable(transformer: &str, facts: &RoomFacts) -> bool {
    facts.present
        && facts.source.as_deref() == Some("lv")
        && facts.transformer_name.as_deref() == Some(transformer)
        && facts.updated_at.is_some_and(f64::is_finite)
        && facts.layer_count > 0
}

/// May the room this device holds for `transformer` be printed? A usable
/// room is admitted with its server version. A room that is unusable
/// because it carries unsaved, incomplete edits refuses
/// `print_preview_room_incomplete` (save or repair it first); any other
/// unusable or absent room refuses `print_preview_room_unavailable`
/// (download its design first). Nothing is fetched on either path.
pub fn room_admission(transformer: &str, facts: &RoomFacts) -> Result<AdmittedRoom, Refusal> {
    if usable(transformer, facts) {
        return Ok(AdmittedRoom {
            server_version: admitted_server_version(facts.server_version),
        });
    }
    let params = [("name", transformer)];
    if facts.present && facts.local_dirty {
        return Err(refusal(
            "print_preview_room_incomplete",
            "printing_preview_room_incomplete",
            &params,
        ));
    }
    Err(refusal(
        "print_preview_room_unavailable",
        "printing_preview_room_unavailable",
        &params,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts() -> Facts {
        let mut layout = super::super::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        Facts {
            layout,
            transformer: Some(Transformer {
                name: "agahehe".into(),
                kind: "transformer".into(),
                cached: Some(true),
                dirty: Some(false),
            }),
            subject: None,
            available_style_refs: vec!["master/lv_lines_print".into()],
            online: true,
            native_ready: true,
            screen_px_per_mm: None,
        }
    }

    fn classified(purpose: &str, role: &str) -> Facts {
        let mut f = facts();
        f.layout.classification =
            Some(serde_json::from_value(json!({"purpose": purpose, "role": role})).unwrap());
        f
    }

    #[test]
    fn the_subject_follows_the_purpose_and_a_mismatch_is_refused_by_key() {
        // An MV sheet never previews against a transformer.
        let mv = plan(&classified("mv_model", "plan_profile")).unwrap();
        assert!(!mv.ready);
        assert_eq!(mv.subject, SubjectKind::MvModels);
        assert_eq!(mv.render_command.as_deref(), Some("report.plan-profile"));
        assert_eq!(mv.refusals[0].code, "print_preview_subject_mismatch");
        assert_eq!(mv.refusals[0].params["expected"], "mv_models");
        assert_eq!(mv.refusals[0].params["subject"], "transformer");
        // Front matter previews against the booklet's MV models.
        let mut f = classified("front_matter", "cover");
        f.transformer = None;
        f.subject = Some(Subject::MvModels {
            model_ids: vec!["model-a".into(), "model-b".into()],
        });
        let front = plan(&f).unwrap();
        assert!(front.ready, "{:?}", front.refusals);
        assert_eq!(front.transformer, None);
        // Absent, empty or repeated models are named.
        f.subject = None;
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_no_mv_model"
        );
        f.subject = Some(Subject::MvModels { model_ids: vec![] });
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_no_mv_model"
        );
        f.subject = Some(Subject::MvModels {
            model_ids: vec!["a".into(), "a".into()],
        });
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_mv_model_invalid"
        );
        // An overview sheet previews against an extent, which no renderer draws yet.
        let mut o = classified("project", "overview");
        o.transformer = None;
        o.subject = Some(Subject::Extent {
            bbox: Some([29.0, -2.9, 29.6, -2.3]),
        });
        let overview = plan(&o).unwrap();
        assert_eq!(overview.subject, SubjectKind::Extent);
        assert_eq!(overview.render_command, None);
        let codes: Vec<&str> = overview.refusals.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(codes, ["print_preview_extent_unavailable"]);
        o.subject = Some(Subject::Extent {
            bbox: Some([29.6, -2.9, 29.0, -2.3]),
        });
        assert_eq!(
            plan(&o).unwrap().refusals[0].code,
            "print_preview_extent_invalid"
        );
        o.subject = None;
        assert_eq!(
            plan(&o).unwrap().refusals[0].code,
            "print_preview_no_extent"
        );
        // LV sheets keep the transformer subject, legacy or explicit.
        let lv = plan(&classified("lv_transformer", "network")).unwrap();
        assert!(lv.ready && lv.subject == SubjectKind::Transformer);
        assert_eq!(lv.render_command.as_deref(), Some("report.project.export"));
        let mut explicit = classified("voltage_drop", "report");
        explicit.transformer = None;
        explicit.subject = Some(Subject::Transformer {
            name: "agahehe".into(),
            kind: "transformer".into(),
            cached: Some(true),
            dirty: None,
        });
        let explicit = plan(&explicit).unwrap();
        assert!(explicit.ready, "{:?}", explicit.refusals);
        assert_eq!(explicit.transformer.as_deref(), Some("agahehe"));
        // An unclassified layout carrying an MV setup reads as an MV sheet.
        let legacy: Layout = serde_json::from_str(include_str!(
            "../../tests/fixtures/mv-global-canonical.json"
        ))
        .unwrap();
        assert_eq!(expected_subject(&legacy), SubjectKind::MvModels);
        assert_eq!(expected_subject(&facts().layout), SubjectKind::Transformer);
    }

    #[test]
    fn subjects_read_the_documented_json_and_nothing_more() {
        let layout = super::super::default_layout();
        let f: Facts = serde_json::from_value(json!({"layout": layout,
            "subject": {"subject": "mv_models", "model_ids": ["m1"]}}))
        .unwrap();
        assert_eq!(
            f.subject,
            Some(Subject::MvModels {
                model_ids: vec!["m1".into()]
            })
        );
        assert!(serde_json::from_value::<Facts>(json!({"layout": layout,
            "subject": {"subject": "mv_models", "model_ids": ["m1"], "extra": 1}}))
        .is_err());
        assert!(serde_json::from_value::<Facts>(json!({"layout": layout,
            "subject": {"subject": "district"}}))
        .is_err());
    }

    #[test]
    fn a_held_transformer_with_its_styles_previews_as_the_shared_page() {
        let plan = plan(&facts()).unwrap();
        assert!(plan.ready && plan.refusals.is_empty() && plan.notes.is_empty());
        assert_eq!(plan.output_id, format!("svg__{}", facts().layout.id));
        assert_eq!(json!(plan.format), json!("svg"));
        assert_eq!(plan.print_style_refs, ["master/lv_lines_print"]);
        assert_eq!(plan.transformer.as_deref(), Some("agahehe"));
        // A3 landscape at the 96 dpi default: 1587 × 1123.
        assert_eq!((plan.raster.width_px, plan.raster.height_px), (1587, 1123));
    }

    #[test]
    fn a_transformer_returned_by_the_printable_inventory_is_admitted_unchanged() {
        let inventory = super::super::inventory::printable(
            &[json!({"name":"upgrade_munyinya","kind":"individual","metadata":{"version":7}})],
            &[super::super::inventory::Room {
                name: "upgrade_munyinya".into(),
                owner_email: Some("owner@example.com".into()),
                local_dirty: false,
            }],
            "owner@example.com",
            1,
        )
        .unwrap();
        let row = &inventory.transformers[0];
        let mut facts = facts();
        facts.transformer = Some(Transformer {
            name: row.name.clone(),
            kind: row.kind.clone(),
            cached: Some(row.cached),
            dirty: Some(row.dirty),
        });
        let planned = plan(&facts).unwrap();
        assert!(planned.ready, "{:?}", planned.refusals);
        assert_eq!(planned.transformer.as_deref(), Some("upgrade_munyinya"));
    }

    #[test]
    fn the_raster_follows_the_screen_scale_within_its_budget() {
        let a0 = [1189., 841.];
        // Fit-to-page on a laptop is below the floor: the floor wins.
        let fitted = raster(a0, Some(1.1));
        assert_eq!(fitted.px_per_mm, MIN_PX_PER_MM);
        assert_eq!((fitted.width_px, fitted.height_px), (1784, 1262));
        let zoomed = raster(a0, Some(5.));
        assert_eq!(zoomed.px_per_mm, 5.);
        assert_eq!((zoomed.width_px, zoomed.height_px), (5945, 4205));
        // Far in on an A0: the pixel budget, not the delivery raster, caps it.
        let deep = raster(a0, Some(40.));
        assert!(deep.px_per_mm < MAX_PX_PER_MM);
        assert!(f64::from(deep.width_px) * f64::from(deep.height_px) <= MAX_RASTER_PIXELS * 1.01);
        // A small sheet reaches the delivery raster before the budget.
        let a4 = raster([297., 210.], Some(40.));
        assert_eq!(a4.px_per_mm, MAX_PX_PER_MM);
        assert_eq!(raster(a0, Some(f64::NAN)).px_per_mm, 96. / 25.4);
        assert_eq!(raster(a0, None).px_per_mm, 96. / 25.4);
    }

    #[test]
    fn every_missing_fact_is_named_by_key() {
        let mut f = facts();
        f.transformer = None;
        f.available_style_refs.clear();
        f.native_ready = false;
        let first = plan(&f).unwrap();
        let codes: Vec<&str> = first.refusals.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "print_style_unavailable",
                "print_preview_no_transformer",
                "print_preview_native_unavailable"
            ]
        );
        assert_eq!(
            first.refusals[0].params["reference"],
            "master/lv_lines_print"
        );
        assert!(!first.ready);

        let mut f = facts();
        f.transformer.as_mut().unwrap().cached = None;
        f.online = false;
        assert!(plan(&f).unwrap().ready, "an unknown hold is not a refusal");

        let mut f = facts();
        f.transformer.as_mut().unwrap().kind = "project_document".into();
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_not_a_transformer"
        );
        let mut f = facts();
        f.transformer.as_mut().unwrap().name = "combined_transformer".into();
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_not_a_transformer"
        );

        let mut f = facts();
        f.transformer.as_mut().unwrap().dirty = Some(true);
        let noted = plan(&f).unwrap();
        assert!(noted.ready);
        assert_eq!(noted.notes[0].code, "print_preview_dirty_room");
    }

    #[test]
    fn a_room_this_device_does_not_hold_is_refused_online_or_not() {
        for online in [false, true] {
            let mut f = facts();
            f.transformer.as_mut().unwrap().cached = Some(false);
            f.online = online;
            let plan = plan(&f).unwrap();
            assert!(!plan.ready, "online={online}");
            assert_eq!(plan.refusals[0].code, "print_preview_room_unavailable");
            assert_eq!(
                plan.refusals[0].message_key,
                "printing_preview_room_unavailable"
            );
            assert_eq!(plan.refusals[0].params["name"], "agahehe");
        }
    }

    fn room() -> RoomFacts {
        RoomFacts {
            present: true,
            source: Some("lv".into()),
            transformer_name: Some("agahehe".into()),
            layer_count: 3,
            local_dirty: false,
            server_version: Some(7.),
            updated_at: Some(1_726_000_000_000.),
        }
    }

    #[test]
    fn room_admission_admits_a_usable_lv_room_with_its_server_version() {
        let admitted = room_admission("agahehe", &room()).unwrap();
        assert_eq!(admitted, AdmittedRoom { server_version: 7 });
        // Unsaved edits over a complete room are still a printable room: the
        // plan notes them, the admission prints them.
        let mut dirty = room();
        dirty.local_dirty = true;
        assert_eq!(room_admission("agahehe", &dirty).unwrap().server_version, 7);
    }

    #[test]
    fn room_admission_refuses_dirty_incomplete_absent_and_foreign_rooms_by_key() {
        let code = |facts: &RoomFacts| room_admission("agahehe", facts).unwrap_err().code;
        let mut incomplete = room();
        incomplete.layer_count = 0;
        incomplete.local_dirty = true;
        assert_eq!(code(&incomplete), "print_preview_room_incomplete");
        let refused = room_admission("agahehe", &incomplete).unwrap_err();
        assert_eq!(refused.message_key, "printing_preview_room_incomplete");
        assert_eq!(refused.params["name"], "agahehe");

        let mut empty = room();
        empty.layer_count = 0;
        assert_eq!(code(&empty), "print_preview_room_unavailable");
        let mut unstored = room();
        unstored.updated_at = None;
        assert_eq!(code(&unstored), "print_preview_room_unavailable");
        let mut nan = room();
        nan.updated_at = Some(f64::NAN);
        assert_eq!(code(&nan), "print_preview_room_unavailable");
        assert_eq!(
            code(&RoomFacts::default()),
            "print_preview_room_unavailable"
        );
        // An absent row is never "incomplete", whatever the dirty flag says.
        let absent_dirty = RoomFacts {
            local_dirty: true,
            ..RoomFacts::default()
        };
        assert_eq!(code(&absent_dirty), "print_preview_room_unavailable");

        let mut foreign = room();
        foreign.transformer_name = Some("kabeza".into());
        assert_eq!(code(&foreign), "print_preview_room_unavailable");
        let mut mv = room();
        mv.source = Some("mv".into());
        assert_eq!(code(&mv), "print_preview_room_unavailable");
    }

    #[test]
    fn a_non_positive_server_version_reads_as_zero() {
        let version = |stored: Option<f64>| {
            let mut facts = room();
            facts.server_version = stored;
            room_admission("agahehe", &facts).unwrap().server_version
        };
        assert_eq!(version(None), 0);
        assert_eq!(version(Some(0.)), 0);
        assert_eq!(version(Some(-3.)), 0);
        assert_eq!(version(Some(2.5)), 0);
        assert_eq!(version(Some(f64::INFINITY)), 0);
        assert_eq!(version(Some(9_007_199_254_740_992.)), 0);
        assert_eq!(version(Some(9_007_199_254_740_991.)), 9_007_199_254_740_991);
        assert_eq!(version(Some(12.)), 12);
    }
}
