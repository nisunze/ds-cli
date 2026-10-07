//! Starting a project from a template project — the ONE plan.
//!
//! A template is an ordinary project in the `template` lifecycle state
//! (ds-brain `docs/contracts/project-lifecycle.md`, owner 2026-10-05). Starting
//! a new project from one is NOT a new server operation. It is the existing
//! project creation followed by copy doors that already exist and authorize
//! themselves:
//!
//! 1. `POST /api/v1/projects {action:"create"}` — the new project, carrying the
//!    template's own settings;
//! 2. the `/report` `network_documents` copy — the template's own network
//!    documents (`docs/network_template`, `docs/network_config`), planned
//!    then applied under the plan's digest (ds-brain
//!    `docs/contracts/project-network-documents.md`);
//! 3. `POST /api/v1/printing {action:"copy"}` — every printing setup the
//!    template holds, each with the logos its layout carries;
//! 4. the design migration — every active transformer with the MV design
//!    document (`kind: transformer`), then every live DS Grid model
//!    (`kind: dsgrid`), in batches of [`crate::design_migration::MAX_ITEMS`].
//!
//! This module decides that plan from facts a host has read, and nothing
//! else: no IO, no clock, no credential. The one host that runs it is
//! `ds auth project create --from-template`; the web's "Start from template"
//! hands the operator that door rather than running the plan a second time.
//!
//! What never travels is stated, never silent ([`NOT_CARRIED`]): computed
//! results regenerate, survey data is the new project's own, members are
//! granted per project, and Solar city data is copied city by city later.
//!
//! Every finding is a key; the words are the host's.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: &str = "ds.project-template-plan/v1";
/// One design-migration request's bound, shared with the migration door.
pub const MIGRATION_BATCH: usize = crate::design_migration::MAX_ITEMS;
/// The listing bucket a template is held in.
pub const TEMPLATE_STATE: &str = "template";
/// The longest display name, location, client or country the creation door takes.
const MAX_TEXT_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 2000;
/// A settings block carried into the creation payload is a small document.
const MAX_SETTINGS_BYTES: usize = 8192;
const MAX_PRINTING_SETUPS: usize = 200;
const MAX_TRANSFORMERS: usize = 10_000;
const MAX_MODELS: usize = 2_000;

/// What the operator asked for: the new project's own identity. Everything
/// else is the template's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Overrides the template's country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Overrides the template's client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

/// The template project as the caller's freshly read project directory holds
/// it. Visibility in that directory is the only admission the plan makes;
/// every door re-authorizes its own call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub project: String,
    /// The listing bucket the directory read the template from.
    pub lifecycle_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styling_template_id: Option<String>,
    #[serde(default)]
    pub project_params: Value,
    #[serde(default)]
    pub project_components: Value,
    #[serde(default)]
    pub project_phases: Value,
    #[serde(default)]
    pub processing_lanes: Value,
}

/// One printing setup the template holds, as the printing list returns it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintingSetup {
    pub id: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One transformer inventory row of the template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transformer {
    pub name: String,
    /// `transformer` or `project_level`.
    pub kind: String,
    /// `active`, `retired`, `deleted` or `missing`.
    pub lifecycle: String,
}

/// Everything the plan is decided from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub request: Request,
    pub template: Template,
    #[serde(default)]
    pub printing_setups: Vec<PrintingSetup>,
    #[serde(default)]
    pub transformers: Vec<Transformer>,
    /// Live DS Grid model ids of the template.
    #[serde(default)]
    pub dsgrid_models: Vec<String>,
}

/// The creation payload: the operator's identity over the template's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub display_name: String,
    /// The `project_name` slug the browser and the creation door derive.
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styling_template_id: Option<String>,
    /// The template's settings blocks, each carried only when it is a
    /// bounded JSON object; an absent block is the server's default.
    pub settings: BTreeMap<String, Value>,
}

/// One door call group, in the order a host runs them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "door", rename_all = "snake_case")]
pub enum Step {
    /// `POST /api/v1/projects {action:"create"}` with [`Plan::project`].
    CreateProject { order: usize },
    /// `/report` `network_documents` plan then apply: every network document
    /// the template holds, copied whole into the new project.
    NetworkDocuments { order: usize },
    /// `POST /api/v1/printing {action:"copy"}` from the template into the new
    /// project, one call per setup, same id.
    PrintingSetups {
        order: usize,
        setups: Vec<PrintingSetup>,
    },
    /// `design migrate apply --source-project <template> --kind <kind>`, one
    /// call per batch.
    DesignMigration {
        order: usize,
        kind: crate::design_migration::Kind,
        batch: usize,
        items: Vec<String>,
    },
}

/// A keyed finding with the item it is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl Finding {
    fn new(code: &str, item: Option<&str>) -> Self {
        Self {
            code: code.to_owned(),
            item: item.map(str::to_owned),
        }
    }
}

/// What the plan carries, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// One copy of the template's network documents.
    pub network_documents: usize,
    pub printing_setups: usize,
    pub transformers: usize,
    pub dsgrid_models: usize,
    /// Door calls the apply makes beyond its fresh reads.
    pub write_calls: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub schema: String,
    pub template: String,
    pub template_state: String,
    /// False while any refusal stands; a host applies only a ready plan.
    pub ready: bool,
    pub refusals: Vec<Finding>,
    pub project: NewProject,
    pub steps: Vec<Step>,
    /// Items the template holds that this plan leaves behind, each keyed.
    pub skipped: Vec<Finding>,
    /// Whole domains no template carries, stated rather than silent.
    pub not_carried: Vec<String>,
    pub totals: Totals,
}

/// The domains a template never carries.
pub const NOT_CARRIED: [&str; 5] = [
    // Reports, exports, tiles and projections are computed per project and
    // regenerate in the new one; the design migration already drops them.
    "computed_results",
    // Survey forms and entries are the new project's own collection.
    "survey_forms_and_entries",
    // Membership is granted per project; the creator is the first admin.
    "members",
    // Project Work, Assets, tags and comments belong to the template's record.
    "project_records",
    // Solar city data is copied city by city later (owner, 2026-10-05).
    "solar",
];

/// Why the plan cannot be applied as asked.
pub mod refusal {
    pub const TEMPLATE_NOT_IN_TEMPLATE_STATE: &str = "template_not_in_template_state";
    pub const TEMPLATE_INVALID: &str = "template_invalid";
    pub const DISPLAY_NAME_INVALID: &str = "display_name_invalid";
    pub const PROPERTY_INVALID: &str = "property_invalid";
}

/// Why an item the template holds is left behind.
pub mod skip {
    pub const SETTINGS_NOT_AN_OBJECT: &str = "settings_block_not_an_object";
    pub const TRANSFORMER_NOT_ACTIVE: &str = "transformer_not_active";
    pub const PROJECT_LEVEL_COMPUTED: &str = "project_level_document_computed";
    pub const PRINTING_SETUP_INVALID: &str = "printing_setup_invalid";
}

/// The MV design document travels with the transformers; every other
/// project-level document is computed.
const MV_DESIGN_DOCUMENT: &str = "mv_data";

fn text(value: &str, max: usize, multiline: bool) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
}

fn optional_text(value: &Option<String>, max: usize, multiline: bool) -> bool {
    value.as_deref().is_none_or(|v| text(v, max, multiline))
}

/// The template's own scalar, dropped when it is empty.
fn carried(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// The `project_name` slug, exactly as the creation door derives it.
pub fn slug(display_name: &str) -> String {
    let mut out = String::with_capacity(display_name.len());
    for c in display_name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_owned()
}

fn valid_revision(revision: &str) -> bool {
    revision.len() == 64
        && revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Decide the plan. Pure: the same input is the same plan.
pub fn plan(input: &Input) -> Plan {
    let request = &input.request;
    let template = &input.template;
    let mut refusals = BTreeSet::new();
    let mut skipped = BTreeSet::new();

    if !crate::execution_context::valid_project(&template.project) {
        refusals.insert(Finding::new(refusal::TEMPLATE_INVALID, None));
    }
    if template.lifecycle_state != TEMPLATE_STATE {
        refusals.insert(Finding::new(
            refusal::TEMPLATE_NOT_IN_TEMPLATE_STATE,
            Some(&template.lifecycle_state),
        ));
    }
    let project_name = slug(&request.display_name);
    if !text(&request.display_name, MAX_TEXT_CHARS, false) || project_name.is_empty() {
        refusals.insert(Finding::new(refusal::DISPLAY_NAME_INVALID, None));
    }
    for (name, value, max, multiline) in [
        ("location", &request.location, MAX_TEXT_CHARS, false),
        (
            "description",
            &request.description,
            MAX_DESCRIPTION_CHARS,
            true,
        ),
        ("country", &request.country, MAX_TEXT_CHARS, false),
        ("client", &request.client, MAX_TEXT_CHARS, false),
    ] {
        if !optional_text(value, max, multiline) {
            refusals.insert(Finding::new(refusal::PROPERTY_INVALID, Some(name)));
        }
    }

    let mut settings = BTreeMap::new();
    for (key, value) in [
        ("project_params", &template.project_params),
        ("project_components", &template.project_components),
        ("project_phases", &template.project_phases),
        ("processing_lanes", &template.processing_lanes),
    ] {
        match value {
            Value::Null => {}
            Value::Object(_)
                if serde_json::to_vec(value).map_or(usize::MAX, |b| b.len())
                    <= MAX_SETTINGS_BYTES =>
            {
                settings.insert(key.to_owned(), value.clone());
            }
            _ => {
                skipped.insert(Finding::new(skip::SETTINGS_NOT_AN_OBJECT, Some(key)));
            }
        }
    }
    let project = NewProject {
        display_name: request.display_name.clone(),
        project_name,
        location: request.location.clone(),
        description: request.description.clone(),
        country: request
            .country
            .clone()
            .or_else(|| carried(&template.country)),
        client: request.client.clone().or_else(|| carried(&template.client)),
        network_template: carried(&template.network_template),
        styling_template_id: carried(&template.styling_template_id),
        settings,
    };
    for (name, value) in [
        ("country", &project.country),
        ("client", &project.client),
        ("network_template", &project.network_template),
        ("styling_template_id", &project.styling_template_id),
    ] {
        if !optional_text(value, MAX_TEXT_CHARS, false) {
            refusals.insert(Finding::new(refusal::PROPERTY_INVALID, Some(name)));
        }
    }

    // Printing: every valid setup, by id; the copy keeps the id.
    let mut setups: BTreeMap<String, PrintingSetup> = BTreeMap::new();
    for setup in input.printing_setups.iter().take(MAX_PRINTING_SETUPS) {
        if crate::printing::identifier(&setup.id) && valid_revision(&setup.revision) {
            setups
                .entry(setup.id.clone())
                .or_insert_with(|| setup.clone());
        } else {
            skipped.insert(Finding::new(skip::PRINTING_SETUP_INVALID, Some(&setup.id)));
        }
    }

    // Transformers: active ones and the MV design document travel.
    let mut transformers = BTreeSet::new();
    for row in input.transformers.iter().take(MAX_TRANSFORMERS) {
        let name = row.name.trim();
        if name.is_empty() {
            continue;
        }
        if row.lifecycle != "active" {
            skipped.insert(Finding::new(skip::TRANSFORMER_NOT_ACTIVE, Some(name)));
        } else if row.kind == "transformer" || name == MV_DESIGN_DOCUMENT {
            transformers.insert(name.to_owned());
        } else {
            skipped.insert(Finding::new(skip::PROJECT_LEVEL_COMPUTED, Some(name)));
        }
    }
    let models: BTreeSet<String> = input
        .dsgrid_models
        .iter()
        .take(MAX_MODELS)
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();

    let mut steps = vec![Step::CreateProject { order: 1 }];
    // The network documents travel whole: one plan and one apply.
    steps.push(Step::NetworkDocuments { order: 2 });
    let mut write_calls = 2;
    if !setups.is_empty() {
        write_calls += setups.len();
        steps.push(Step::PrintingSetups {
            order: steps.len() + 1,
            setups: setups.values().cloned().collect(),
        });
    }
    for (kind, items) in [
        (crate::design_migration::Kind::Transformer, &transformers),
        (crate::design_migration::Kind::Dsgrid, &models),
    ] {
        let items: Vec<String> = items.iter().cloned().collect();
        for (index, batch) in items.chunks(MIGRATION_BATCH).enumerate() {
            write_calls += 1;
            steps.push(Step::DesignMigration {
                order: steps.len() + 1,
                kind,
                batch: index + 1,
                items: batch.to_vec(),
            });
        }
    }

    let totals = Totals {
        network_documents: 1,
        printing_setups: setups.len(),
        transformers: transformers.len(),
        dsgrid_models: models.len(),
        write_calls,
    };
    let refusals: Vec<Finding> = refusals.into_iter().collect();
    Plan {
        schema: SCHEMA.to_owned(),
        template: template.project.clone(),
        template_state: template.lifecycle_state.clone(),
        ready: refusals.is_empty(),
        refusals,
        project,
        steps,
        skipped: skipped.into_iter().collect(),
        not_carried: NOT_CARRIED
            .iter()
            .map(|domain| (*domain).to_owned())
            .collect(),
        totals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REVISION: &str = "3e417f9c61c328262ada243ed6e954ad680839866e830ddc86239a2bf8b1813d";

    fn template(state: &str) -> Template {
        Template {
            project: "czgmdwth_gisagara".into(),
            lifecycle_state: state.into(),
            display_name: Some("Gisagara, CJIC".into()),
            country: Some("Rwanda".into()),
            client: Some("EDCL".into()),
            network_template: Some("master".into()),
            styling_template_id: Some(String::new()),
            project_params: json!({"project_type":"new_build","crs":{"mode":"tm_rwanda"}}),
            project_components: json!({"design":true,"solar":false}),
            project_phases: json!({"network":["I"],"solar":["I"]}),
            processing_lanes: json!({"fast":true,"standard":false}),
        }
    }

    fn input(state: &str) -> Input {
        Input {
            request: Request {
                display_name: "Nyaruguru LV".into(),
                location: Some("Nyaruguru".into()),
                description: None,
                country: None,
                client: None,
            },
            template: template(state),
            printing_setups: vec![
                PrintingSetup {
                    id: "a3-landscape-gisagara-cjic".into(),
                    revision: REVISION.into(),
                    name: Some("A3".into()),
                },
                PrintingSetup {
                    id: "a0-landscape-gisagara-cjic".into(),
                    revision: REVISION.into(),
                    name: None,
                },
            ],
            transformers: vec![
                Transformer {
                    name: "TR-2".into(),
                    kind: "transformer".into(),
                    lifecycle: "active".into(),
                },
                Transformer {
                    name: "mv_data".into(),
                    kind: "project_level".into(),
                    lifecycle: "active".into(),
                },
                Transformer {
                    name: "TR-1".into(),
                    kind: "transformer".into(),
                    lifecycle: "active".into(),
                },
                Transformer {
                    name: "TR-old".into(),
                    kind: "transformer".into(),
                    lifecycle: "retired".into(),
                },
                Transformer {
                    name: "collisions".into(),
                    kind: "project_level".into(),
                    lifecycle: "active".into(),
                },
            ],
            dsgrid_models: vec!["model_b".into(), "model_a".into(), "model_a".into()],
        }
    }

    #[test]
    fn a_template_plans_creation_then_every_existing_copy_door_in_order() {
        let plan = plan(&input(TEMPLATE_STATE));
        assert!(plan.ready, "{:?}", plan.refusals);
        assert_eq!(plan.schema, SCHEMA);
        let doors: Vec<&str> = plan
            .steps
            .iter()
            .map(|step| match step {
                Step::CreateProject { .. } => "create_project",
                Step::NetworkDocuments { .. } => "network_documents",
                Step::PrintingSetups { .. } => "printing_setups",
                Step::DesignMigration { kind, .. } => kind.wire(),
            })
            .collect();
        assert_eq!(
            doors,
            [
                "create_project",
                "network_documents",
                "printing_setups",
                "transformer",
                "dsgrid"
            ]
        );
        let orders: Vec<usize> = plan
            .steps
            .iter()
            .map(|step| match step {
                Step::CreateProject { order }
                | Step::NetworkDocuments { order }
                | Step::PrintingSetups { order, .. }
                | Step::DesignMigration { order, .. } => *order,
            })
            .collect();
        assert_eq!(orders, [1, 2, 3, 4, 5]);
        // The operator's identity over the template's settings.
        assert_eq!(plan.project.project_name, "nyaruguru_lv");
        assert_eq!(plan.project.country.as_deref(), Some("Rwanda"));
        assert_eq!(plan.project.client.as_deref(), Some("EDCL"));
        assert_eq!(plan.project.network_template.as_deref(), Some("master"));
        assert_eq!(
            plan.project.styling_template_id, None,
            "an empty template id is no template"
        );
        assert_eq!(plan.project.location.as_deref(), Some("Nyaruguru"));
        assert_eq!(plan.project.settings.len(), 4);
        assert_eq!(
            plan.totals,
            Totals {
                network_documents: 1,
                printing_setups: 2,
                transformers: 3,
                dsgrid_models: 2,
                write_calls: 1 + 1 + 2 + 1 + 1,
            }
        );
        assert_eq!(plan.not_carried, NOT_CARRIED);
    }

    #[test]
    fn items_travel_sorted_and_what_stays_behind_is_keyed() {
        let plan = plan(&input(TEMPLATE_STATE));
        let Step::DesignMigration { items, .. } = &plan.steps[3] else {
            panic!("transformers migrate fourth")
        };
        assert_eq!(items, &["TR-1", "TR-2", "mv_data"]);
        let Step::DesignMigration { items, .. } = &plan.steps[4] else {
            panic!("models migrate last")
        };
        assert_eq!(items, &["model_a", "model_b"]);
        let Step::PrintingSetups { setups, .. } = &plan.steps[2] else {
            panic!("printing third")
        };
        assert_eq!(setups[0].id, "a0-landscape-gisagara-cjic");
        assert!(plan
            .skipped
            .contains(&Finding::new(skip::TRANSFORMER_NOT_ACTIVE, Some("TR-old"))));
        assert!(plan.skipped.contains(&Finding::new(
            skip::PROJECT_LEVEL_COMPUTED,
            Some("collisions")
        )));
    }

    /// A project that is not in the template state is no template. The plan
    /// is still stated in full, so a dry run shows what marking it would
    /// carry, but a host never applies it.
    #[test]
    fn a_project_outside_the_template_state_is_refused_but_still_planned() {
        let plan = plan(&input("active"));
        assert!(!plan.ready);
        assert_eq!(
            plan.refusals,
            [Finding::new(
                refusal::TEMPLATE_NOT_IN_TEMPLATE_STATE,
                Some("active")
            )]
        );
        assert_eq!(plan.totals.transformers, 3);
    }

    #[test]
    fn the_new_projects_name_and_overrides_are_validated() {
        let mut bad = input(TEMPLATE_STATE);
        bad.request.display_name = "———".into();
        bad.request.client = Some(" padded".into());
        let plan = plan(&bad);
        assert!(!plan.ready);
        assert!(plan
            .refusals
            .contains(&Finding::new(refusal::DISPLAY_NAME_INVALID, None)));
        assert!(plan
            .refusals
            .contains(&Finding::new(refusal::PROPERTY_INVALID, Some("client"))));

        let mut overridden = input(TEMPLATE_STATE);
        overridden.request.country = Some("Chad".into());
        assert_eq!(
            super::plan(&overridden).project.country.as_deref(),
            Some("Chad")
        );
    }

    #[test]
    fn migrations_are_batched_at_the_doors_bound() {
        let mut many = input(TEMPLATE_STATE);
        many.transformers = (0..(MIGRATION_BATCH + 5))
            .map(|i| Transformer {
                name: format!("TR-{i:04}"),
                kind: "transformer".into(),
                lifecycle: "active".into(),
            })
            .collect();
        let plan = plan(&many);
        let batches: Vec<(usize, usize)> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::DesignMigration {
                    kind: crate::design_migration::Kind::Transformer,
                    batch,
                    items,
                    ..
                } => Some((*batch, items.len())),
                _ => None,
            })
            .collect();
        assert_eq!(batches, [(1, MIGRATION_BATCH), (2, 5)]);
    }

    #[test]
    fn an_empty_template_is_still_a_creation_with_its_network_documents() {
        let empty = Input {
            printing_setups: vec![],
            transformers: vec![],
            dsgrid_models: vec![],
            ..input(TEMPLATE_STATE)
        };
        let plan = plan(&empty);
        assert!(plan.ready);
        assert_eq!(
            plan.steps,
            [
                Step::CreateProject { order: 1 },
                Step::NetworkDocuments { order: 2 }
            ]
        );
        assert_eq!(plan.totals.write_calls, 2);
    }

    #[test]
    fn the_input_is_closed_and_the_plan_deterministic() {
        let input = input(TEMPLATE_STATE);
        assert_eq!(plan(&input), plan(&input));
        let wire = serde_json::to_value(plan(&input)).unwrap();
        assert_eq!(wire["steps"][0]["door"], "create_project");
        assert_eq!(wire["steps"][3]["kind"], "transformer");
        let mut unknown = serde_json::to_value(&input).unwrap();
        unknown["template"]["is_template"] = json!(true);
        assert!(serde_json::from_value::<Input>(unknown).is_err());
    }
}
