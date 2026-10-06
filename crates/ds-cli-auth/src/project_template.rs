//! `ds auth project create --from-template <id>` — the host half of starting a
//! project from a template.
//!
//! `ds_command_kernel::project_template` decides the plan. This module only
//! reads the facts it is decided from and runs it, through doors that already
//! exist and authorize themselves: the project directory, the template's
//! printing and design inventories, then `create`, one fenced copy of the
//! template's network documents, a printing copy between the two projects and
//! the design migration. Nothing here decides what a template carries.
//!
//! A dry run reads and plans and writes nothing. An apply refuses a plan that
//! is not ready (a source outside the template state above all), creates the
//! project, and then runs every remaining step even when one item fails: each
//! door is idempotent or fenced, and the receipt names every item's outcome so
//! a partial run is stated, never silent, and can be finished with the doors
//! the receipt names.

use ds_cli_contract::outcome::Failure;
use ds_client_core::{
    ClientError, NetworkDocumentsRequest, NetworkDocumentsSource, PrintingDestination,
    PrintingRequest, PrintingScope, PrintingSource, TransformerSet, design_migration, grid_models,
    project_properties,
};
use ds_command_kernel::design_migration::Mode;
use ds_command_kernel::project_template::{
    self, Input, NewProject, Plan, PrintingSetup, Request, Step, Template, Transformer,
};
use serde_json::{Value, json};

use crate::profile::Lane;

/// The project the creation step stood up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Created {
    pub project_id: String,
    pub project_name: String,
}

/// The doors the plan reads from and runs through, as plain facts. The
/// restored native user and a linked device implement it; tests implement it
/// in memory.
pub(crate) trait Doors {
    /// The template as the caller's fresh directory lists it.
    fn template(&mut self, project: &str) -> Result<Template, Failure>;
    /// Copy every network document `template` holds into `project`: the
    /// plan, then the apply of exactly that plan. Answers the receipt.
    fn copy_network_documents(&mut self, project: &str, template: &str) -> Result<Value, Failure>;
    fn printing_setups(&mut self, project: &str) -> Result<Vec<PrintingSetup>, Failure>;
    fn copy_printing_setup(
        &mut self,
        project: &str,
        request: &PrintingRequest,
    ) -> Result<(), Failure>;
    fn transformers(&mut self, project: &str) -> Result<Vec<Transformer>, Failure>;
    fn dsgrid_models(&mut self, project: &str) -> Result<Vec<String>, Failure>;
    fn create(&mut self, project: &NewProject) -> Result<Created, Failure>;
    fn migrate(
        &mut self,
        project: &str,
        command: &design_migration::Command,
    ) -> Result<Value, Failure>;
}

/// Read everything the plan is decided from, then decide it.
pub(crate) fn plan(
    doors: &mut impl Doors,
    template: &str,
    request: Request,
) -> Result<Plan, Failure> {
    let template = doors.template(template)?;
    let project = template.project.clone();
    let input = Input {
        request,
        printing_setups: doors.printing_setups(&project)?,
        transformers: doors.transformers(&project)?,
        dsgrid_models: doors.dsgrid_models(&project)?,
        template,
    };
    Ok(project_template::plan(&input))
}

/// The template's network documents into `project`: plan, then apply exactly
/// that plan. `call` is one credential's closed network document call.
fn plan_then_apply(
    mut call: impl FnMut(&NetworkDocumentsRequest) -> Result<Value, ClientError>,
    template: &str,
) -> Result<Value, Failure> {
    let source = NetworkDocumentsSource::Project {
        project: template.to_owned(),
    };
    let plan = call(&NetworkDocumentsRequest::Plan {
        source: source.clone(),
        parts: Vec::new(),
    })
    .map_err(super::map_network_documents)?;
    call(&NetworkDocumentsRequest::Apply {
        source,
        parts: Vec::new(),
        expected_plan_sha256: plan["plan_sha256"].as_str().unwrap_or_default().to_owned(),
    })
    .map_err(super::map_network_documents)
}

fn failure_row(item: Value, error: &Failure) -> Value {
    let mut row = item;
    row["outcome"] = json!("failed");
    row["code"] = json!(error.code());
    row
}

/// Run a ready plan. The creation is the one step whose failure stops the
/// run, because nothing else has a project to write into.
pub(crate) fn apply(doors: &mut impl Doors, plan: &Plan) -> Result<Value, Failure> {
    if !plan.ready {
        return Err(Failure::invalid(
            super::TEMPLATE_PLAN_REFUSED_REFUSAL.code,
            "the template plan is not ready, so nothing was created",
        )
        .detail(json!({ "refusals": plan.refusals }))
        .remedy(super::TEMPLATE_PLAN_REFUSED_REFUSAL.remedy));
    }
    let created = doors.create(&plan.project)?;
    let target = created.project_id.clone();
    let mut network = Vec::new();
    let mut setups = Vec::new();
    let mut migrations = Vec::new();
    for step in &plan.steps {
        match step {
            Step::CreateProject { .. } => {}
            Step::NetworkDocuments { .. } => {
                network.push(
                    match doors.copy_network_documents(&target, &plan.template) {
                        Ok(receipt) => {
                            let written = receipt["written"].clone();
                            let copied = written.as_array().is_some_and(|parts| !parts.is_empty());
                            json!({
                                "outcome": if copied { "copied" } else { "identical" },
                                "written": written,
                                "plan_sha256": receipt["plan_sha256"],
                            })
                        }
                        // A template that holds no network document has
                        // nothing to copy; that is stated, not a failure.
                        Err(error)
                            if error.code()
                                == super::NETWORK_DOCUMENTS_SOURCE_MISSING_REFUSAL.code =>
                        {
                            json!({ "outcome": "absent" })
                        }
                        Err(error) => failure_row(json!({}), &error),
                    },
                );
            }
            Step::PrintingSetups {
                setups: planned, ..
            } => {
                // The creation adopts the governed global setups; a template
                // setup with the same id replaces that seed at its revision.
                let held: std::collections::BTreeMap<String, String> = doors
                    .printing_setups(&target)?
                    .into_iter()
                    .map(|setup| (setup.id, setup.revision))
                    .collect();
                for setup in planned {
                    let expected = held.get(&setup.id).cloned().unwrap_or_default();
                    let request = PrintingRequest::Copy {
                        source: PrintingSource {
                            scope: PrintingScope::Project,
                            project: Some(plan.template.clone()),
                            id: setup.id.clone(),
                            revision: setup.revision.clone(),
                        },
                        destination: PrintingDestination {
                            scope: PrintingScope::Project,
                            id: setup.id.clone(),
                            name: None,
                            expected_revision: expected.clone(),
                        },
                    };
                    let item = json!({ "id": setup.id, "replaced_seed": !expected.is_empty() });
                    setups.push(match doors.copy_printing_setup(&target, &request) {
                        Ok(()) => {
                            let mut row = item;
                            row["outcome"] = json!("copied");
                            row
                        }
                        Err(error) => failure_row(item, &error),
                    });
                }
            }
            Step::DesignMigration {
                kind, batch, items, ..
            } => {
                let command = design_migration::Command {
                    source_project: plan.template.clone(),
                    kind: *kind,
                    mode: Mode::Apply,
                    items: items.clone(),
                    overwrite_existing: false,
                };
                let item = json!({ "kind": kind.wire(), "batch": batch, "requested": items.len() });
                migrations.push(match doors.migrate(&target, &command) {
                    Ok(receipt) => {
                        let outcomes =
                            ds_command_kernel::design_migration::read_items(&receipt["items"]);
                        let totals = ds_command_kernel::design_migration::totals(&outcomes);
                        let mut row = item;
                        row["outcome"] = json!(if totals.failed == 0 {
                            "migrated"
                        } else {
                            "partial"
                        });
                        row["moving"] = json!(totals.moving);
                        row["identical"] = json!(totals.identical);
                        row["blocked"] = json!(totals.blocked);
                        row["failed"] = json!(totals.failed);
                        row
                    }
                    Err(error) => failure_row(item, &error),
                });
            }
        }
    }
    let complete = [&network, &setups, &migrations].iter().all(|rows| {
        rows.iter()
            .all(|row| !matches!(row["outcome"].as_str(), Some("failed" | "partial")))
    });
    Ok(json!({
        "project": { "project_id": created.project_id, "project_name": created.project_name },
        "network_documents": network.into_iter().next().unwrap_or(Value::Null),
        "printing_setups": setups,
        "design_migrations": migrations,
        "complete": complete,
    }))
}

/// Plan, and apply unless this is a dry run.
pub(crate) fn run(
    doors: &mut impl Doors,
    template: &str,
    request: Request,
    dry_run: bool,
) -> Result<Value, Failure> {
    let plan = plan(doors, template, request)?;
    let applied = if dry_run {
        Value::Null
    } else {
        apply(doors, &plan)?
    };
    Ok(json!({
        "mode": if dry_run { "plan" } else { "apply" },
        "plan": plan,
        "applied": applied,
    }))
}

// ---------------------------------------------------------------------------
// The two credential providers.
// ---------------------------------------------------------------------------

fn template_from(project: &ds_client_core::Project) -> Template {
    let settings = project.settings();
    Template {
        project: project.ds_project().to_owned(),
        lifecycle_state: project.status().query_value().to_owned(),
        display_name: project.display_name().map(str::to_owned),
        country: settings.country.clone(),
        client: settings.client.clone(),
        network_template: settings.network_template.clone(),
        styling_template_id: settings.styling_template_id.clone(),
        project_params: project.project_params().clone(),
        project_components: settings.project_components.clone(),
        project_phases: settings.project_phases.clone(),
        processing_lanes: settings.processing_lanes.clone(),
    }
}

fn visible(
    directory: &ds_client_core::ProjectDirectory,
    project: &str,
) -> Result<Template, Failure> {
    directory.exact(project).map(template_from).ok_or_else(|| {
        Failure::invalid(
            super::TEMPLATE_NOT_VISIBLE_REFUSAL.code,
            "that exact project id is not in the project directory the gateway returned",
        )
        .remedy(super::TEMPLATE_NOT_VISIBLE_REFUSAL.remedy)
        .next("ds auth project list --output json")
    })
}

fn setups_from(value: &Value) -> Vec<PrintingSetup> {
    value["setups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(PrintingSetup {
                id: row["id"].as_str()?.to_owned(),
                revision: row["revision"].as_str()?.to_owned(),
                name: row["name"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

fn transformers_from(inventory: &ds_client_core::TransformerInventory) -> Vec<Transformer> {
    inventory
        .rows()
        .iter()
        .map(|row| Transformer {
            name: row.name().to_owned(),
            kind: row.kind().token().to_owned(),
            lifecycle: row.lifecycle().token().to_owned(),
        })
        .collect()
}

/// Every live model id, page by page, bounded by the kernel's own model cap.
fn models_from(
    mut page: impl FnMut(Option<String>) -> Result<grid_models::Receipt, Failure>,
) -> Result<Vec<String>, Failure> {
    let mut models = Vec::new();
    let mut cursor = None;
    loop {
        let receipt = page(cursor.take())?;
        for row in receipt.data["models"].as_array().into_iter().flatten() {
            if let Some(id) = row["model_id"].as_str() {
                models.push(id.to_owned());
            }
        }
        let next = receipt.data["next_cursor"].as_str().unwrap_or("");
        if receipt.data["more"] != true || next.is_empty() || models.len() >= 2_000 {
            return Ok(models);
        }
        cursor = Some(next.to_owned());
    }
}

fn created_from(receipt: project_properties::Receipt) -> Result<Created, Failure> {
    match receipt {
        project_properties::Receipt::Created(created) => Ok(Created {
            project_id: created.project_id,
            project_name: created.project_name,
        }),
        project_properties::Receipt::Updated(_) => Err(Failure::unavailable(
            super::UNREADABLE_REFUSAL.code,
            "the create door answered with an update receipt",
        )),
    }
}

/// One page of live models; the door's own page bound.
fn list_models(cursor: Option<String>) -> grid_models::Command {
    grid_models::Command::List {
        limit: 100,
        cursor,
        include_deleted: false,
    }
}

/// The restored native user.
pub(crate) struct NativeDoors {
    pub(crate) client: super::NativeClient,
}

impl Doors for NativeDoors {
    fn template(&mut self, project: &str) -> Result<Template, Failure> {
        let directory = self
            .client
            .list_projects(super::now())
            .map_err(super::map_client)?;
        visible(&directory, project)
    }
    fn copy_network_documents(&mut self, project: &str, template: &str) -> Result<Value, Failure> {
        let client = &mut self.client;
        plan_then_apply(
            |request| client.network_documents(project, request, super::now()),
            template,
        )
    }
    fn printing_setups(&mut self, project: &str) -> Result<Vec<PrintingSetup>, Failure> {
        self.client
            .printing(project, &PrintingRequest::List {}, super::now())
            .map(|value| setups_from(&value))
            .map_err(super::map_client)
    }
    fn copy_printing_setup(
        &mut self,
        project: &str,
        request: &PrintingRequest,
    ) -> Result<(), Failure> {
        self.client
            .printing(project, request, super::now())
            .map(|_| ())
            .map_err(super::map_client)
    }
    fn transformers(&mut self, project: &str) -> Result<Vec<Transformer>, Failure> {
        let all = TransformerSet::new(Vec::new()).map_err(super::map_client)?;
        self.client
            .transformer_inventory(project, &all, super::now())
            .map(|inventory| transformers_from(&inventory))
            .map_err(super::map_client)
    }
    fn dsgrid_models(&mut self, project: &str) -> Result<Vec<String>, Failure> {
        let client = &mut self.client;
        models_from(|cursor| {
            client
                .grid_models(project, &list_models(cursor), super::now())
                .map_err(super::map_client)
        })
    }
    fn create(&mut self, project: &NewProject) -> Result<Created, Failure> {
        let command = project_properties::Command::CreateFromTemplate {
            project: project.clone(),
        };
        self.client
            .project_properties(&command, super::now())
            .map_err(super::map_project_properties_client)
            .and_then(created_from)
    }
    fn migrate(
        &mut self,
        project: &str,
        command: &design_migration::Command,
    ) -> Result<Value, Failure> {
        self.client
            .design_migration(project, command, super::now())
            .map_err(super::map_client)
    }
}

/// A linked device credential: the same doors, the device's authority.
impl Doors for crate::device::DeviceSession {
    fn template(&mut self, project: &str) -> Result<Template, Failure> {
        let directory = self.list_projects().map_err(super::map_client)?;
        visible(&directory, project)
    }
    fn copy_network_documents(&mut self, project: &str, template: &str) -> Result<Value, Failure> {
        plan_then_apply(|request| self.network_documents(project, request), template)
    }
    fn printing_setups(&mut self, project: &str) -> Result<Vec<PrintingSetup>, Failure> {
        self.printing(project, &PrintingRequest::List {})
            .map(|value| setups_from(&value))
            .map_err(super::map_client)
    }
    fn copy_printing_setup(
        &mut self,
        project: &str,
        request: &PrintingRequest,
    ) -> Result<(), Failure> {
        self.printing(project, request)
            .map(|_| ())
            .map_err(super::map_client)
    }
    fn transformers(&mut self, project: &str) -> Result<Vec<Transformer>, Failure> {
        let all = TransformerSet::new(Vec::new()).map_err(super::map_client)?;
        self.transformer_inventory(project, &all)
            .map(|inventory| transformers_from(&inventory))
            .map_err(super::map_client)
    }
    fn dsgrid_models(&mut self, project: &str) -> Result<Vec<String>, Failure> {
        models_from(|cursor| {
            self.grid_models(project, &list_models(cursor))
                .map_err(super::map_client)
        })
    }
    fn create(&mut self, project: &NewProject) -> Result<Created, Failure> {
        let command = project_properties::Command::CreateFromTemplate {
            project: project.clone(),
        };
        self.project_properties(&command)
            .map_err(super::map_project_properties_client)
            .and_then(created_from)
    }
    fn migrate(
        &mut self,
        project: &str,
        command: &design_migration::Command,
    ) -> Result<Value, Failure> {
        self.design_migration(project, command)
            .map_err(super::map_client)
    }
}

/// The lane's credential, then the plan (and the apply unless dry).
pub(crate) fn run_for_lane(
    lane: Lane,
    template: &str,
    request: Request,
    dry_run: bool,
) -> Result<Value, Failure> {
    if let Some(mut device) = super::restored_device_session(lane)? {
        return run(&mut device, template, request, dry_run);
    }
    let profile = crate::profile::load(lane)?;
    let store = super::NativeRefreshStore::open()?;
    let mut client = ds_client_core::Client::new(profile, super::NativeTransport, store);
    super::require_restore_before_context(&mut client)?;
    run(&mut NativeDoors { client }, template, request, dry_run)
}

/// The one-paragraph human rendering of a plan or an apply receipt.
pub fn render(data: &Value) -> String {
    let plan = &data["plan"];
    let totals = &plan["totals"];
    let mut out = format!(
        "{} {}  from template {} ({})\n  {} network document copy · {} printing setup(s) · {} transformer(s) · {} DS Grid model(s) · {} write call(s)\n",
        if data["mode"] == "plan" {
            "would create"
        } else {
            "created"
        },
        plan["project"]["display_name"].as_str().unwrap_or("?"),
        plan["template"].as_str().unwrap_or("?"),
        plan["template_state"].as_str().unwrap_or("?"),
        totals["network_documents"],
        totals["printing_setups"],
        totals["transformers"],
        totals["dsgrid_models"],
        totals["write_calls"],
    );
    for refusal in plan["refusals"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  refused: {}{}\n",
            refusal["code"].as_str().unwrap_or("?"),
            refusal["item"]
                .as_str()
                .map(|item| format!(" ({item})"))
                .unwrap_or_default()
        ));
    }
    for skipped in plan["skipped"].as_array().into_iter().flatten().take(20) {
        out.push_str(&format!(
            "  left behind: {} {}\n",
            skipped["code"].as_str().unwrap_or("?"),
            skipped["item"].as_str().unwrap_or("")
        ));
    }
    out.push_str(&format!(
        "  never carried: {}\n",
        plan["not_carried"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if data["mode"] == "plan" {
        out.push_str(if plan["ready"] == true {
            "  dry run only; re-run without --dry-run and with --yes to create it\n"
        } else {
            "  not ready: nothing would be created until the refusals above are cleared\n"
        });
    } else {
        out.push_str(&format!(
            "  project {}  complete: {}\n",
            data["applied"]["project"]["project_id"]
                .as_str()
                .unwrap_or("?"),
            data["applied"]["complete"]
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const REVISION: &str = "3e417f9c61c328262ada243ed6e954ad680839866e830ddc86239a2bf8b1813d";
    const SEED: &str = "547a91038c7642e4108c6c6fb9feeb726a28c702c06e40e39fa6847bb5b9b701";

    /// Every door in memory, recording what was asked of it.
    #[derive(Default)]
    struct FakeDoors {
        state: String,
        calls: Vec<String>,
        held: BTreeMap<String, Vec<PrintingSetup>>,
        // The network document copy answers this, or this refusal code.
        network_refusal: Option<&'static str>,
    }

    impl FakeDoors {
        fn template(state: &str) -> Self {
            let mut held = BTreeMap::new();
            held.insert(
                "czgmdwth_gisagara".to_owned(),
                vec![
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
            );
            // The creation adopts one governed seed of the same id.
            held.insert(
                "uid_nyaruguru_lv".to_owned(),
                vec![PrintingSetup {
                    id: "a3-landscape-gisagara-cjic".into(),
                    revision: SEED.into(),
                    name: None,
                }],
            );
            Self {
                state: state.into(),
                held,
                ..Self::default()
            }
        }
    }

    impl Doors for FakeDoors {
        fn template(&mut self, project: &str) -> Result<Template, Failure> {
            self.calls.push(format!("directory {project}"));
            if project != "czgmdwth_gisagara" {
                return Err(Failure::invalid(
                    crate::TEMPLATE_NOT_VISIBLE_REFUSAL.code,
                    "not visible",
                ));
            }
            Ok(Template {
                project: project.into(),
                lifecycle_state: self.state.clone(),
                display_name: Some("Gisagara, CJIC".into()),
                country: Some("Rwanda".into()),
                client: Some("EDCL".into()),
                network_template: Some("master".into()),
                styling_template_id: Some(String::new()),
                project_params: json!({"project_type":"new_build","crs":{"mode":"tm_rwanda"}}),
                project_components: json!({"design":true}),
                project_phases: json!({"network":["I"],"solar":["I"]}),
                processing_lanes: json!({"fast":true}),
            })
        }
        fn copy_network_documents(
            &mut self,
            project: &str,
            template: &str,
        ) -> Result<Value, Failure> {
            self.calls
                .push(format!("network documents {template} -> {project}"));
            match self.network_refusal {
                Some(code) => Err(Failure::invalid(code, "refused")),
                None => Ok(
                    json!({"written":["network_template","network_config"],"plan_sha256":REVISION}),
                ),
            }
        }
        fn printing_setups(&mut self, project: &str) -> Result<Vec<PrintingSetup>, Failure> {
            self.calls.push(format!("printing list {project}"));
            Ok(self.held.get(project).cloned().unwrap_or_default())
        }
        fn copy_printing_setup(
            &mut self,
            project: &str,
            request: &PrintingRequest,
        ) -> Result<(), Failure> {
            let PrintingRequest::Copy {
                source,
                destination,
            } = request
            else {
                panic!("only copies are run")
            };
            self.calls.push(format!(
                "printing copy {}:{} -> {project}:{} expecting {:?}",
                source.project.as_deref().unwrap_or("?"),
                source.id,
                destination.id,
                destination.expected_revision
            ));
            Ok(())
        }
        fn transformers(&mut self, project: &str) -> Result<Vec<Transformer>, Failure> {
            self.calls.push(format!("inventory {project}"));
            Ok(vec![
                Transformer {
                    name: "TR-1".into(),
                    kind: "transformer".into(),
                    lifecycle: "active".into(),
                },
                Transformer {
                    name: "mv_data".into(),
                    kind: "project_level".into(),
                    lifecycle: "active".into(),
                },
            ])
        }
        fn dsgrid_models(&mut self, project: &str) -> Result<Vec<String>, Failure> {
            self.calls.push(format!("models {project}"));
            Ok(vec!["model_a".into()])
        }
        fn create(&mut self, project: &NewProject) -> Result<Created, Failure> {
            self.calls.push(format!(
                "create {} country={:?} settings={}",
                project.project_name,
                project.country,
                project.settings.len()
            ));
            Ok(Created {
                project_id: "uid_nyaruguru_lv".into(),
                project_name: project.project_name.clone(),
            })
        }
        fn migrate(
            &mut self,
            project: &str,
            command: &design_migration::Command,
        ) -> Result<Value, Failure> {
            self.calls.push(format!(
                "migrate {} {} -> {project} {:?} overwrite={}",
                command.kind.wire(),
                command.source_project,
                command.items,
                command.overwrite_existing
            ));
            Ok(
                json!({"items": command.items.iter().map(|name| json!({"name": name, "status": "copied"})).collect::<Vec<_>>()}),
            )
        }
    }

    fn request() -> Request {
        Request {
            display_name: "Nyaruguru LV".into(),
            location: Some("Nyaruguru".into()),
            description: None,
            country: None,
            client: None,
        }
    }

    /// A dry run reads the template through the five read doors and plans; it
    /// reaches no write door at all.
    #[test]
    fn a_dry_run_reads_and_plans_and_writes_nothing() {
        let mut doors = FakeDoors::template("template");
        let answer = run(&mut doors, "czgmdwth_gisagara", request(), true).unwrap();
        assert_eq!(answer["mode"], "plan");
        assert!(answer["applied"].is_null());
        assert_eq!(answer["plan"]["ready"], true);
        assert_eq!(answer["plan"]["totals"]["transformers"], 2);
        assert_eq!(
            doors.calls,
            [
                "directory czgmdwth_gisagara",
                "printing list czgmdwth_gisagara",
                "inventory czgmdwth_gisagara",
                "models czgmdwth_gisagara",
            ]
        );
    }

    /// The apply creates, then runs every existing door in the plan's order:
    /// the network documents into the new project, printing copied between the projects
    /// (replacing the adopted seed of the same id at its revision), then the
    /// two design migrations without overwrite.
    #[test]
    fn an_apply_creates_then_runs_every_copy_door_into_the_new_project() {
        let mut doors = FakeDoors::template("template");
        let answer = run(&mut doors, "czgmdwth_gisagara", request(), false).unwrap();
        let writes: Vec<&str> = doors.calls[4..].iter().map(String::as_str).collect();
        assert_eq!(
            writes,
            [
                "create nyaruguru_lv country=Some(\"Rwanda\") settings=4",
                "network documents czgmdwth_gisagara -> uid_nyaruguru_lv",
                "printing list uid_nyaruguru_lv",
                "printing copy czgmdwth_gisagara:a0-landscape-gisagara-cjic -> uid_nyaruguru_lv:a0-landscape-gisagara-cjic expecting \"\"",
                &format!(
                    "printing copy czgmdwth_gisagara:a3-landscape-gisagara-cjic -> uid_nyaruguru_lv:a3-landscape-gisagara-cjic expecting \"{SEED}\""
                ),
                "migrate transformer czgmdwth_gisagara -> uid_nyaruguru_lv [\"TR-1\", \"mv_data\"] overwrite=false",
                "migrate dsgrid czgmdwth_gisagara -> uid_nyaruguru_lv [\"model_a\"] overwrite=false",
            ]
        );
        let applied = &answer["applied"];
        assert_eq!(applied["project"]["project_id"], "uid_nyaruguru_lv");
        assert_eq!(applied["network_documents"]["outcome"], "copied");
        assert_eq!(applied["network_documents"]["written"][1], "network_config");
        assert_eq!(applied["printing_setups"][1]["replaced_seed"], true);
        assert_eq!(applied["design_migrations"][0]["moving"], 2);
        assert_eq!(applied["complete"], true);
    }

    /// A project outside the template state is planned in full for the dry
    /// run, but an apply creates nothing.
    #[test]
    fn an_apply_from_a_project_that_is_no_template_creates_nothing() {
        let mut doors = FakeDoors::template("active");
        let refused = run(&mut doors, "czgmdwth_gisagara", request(), false).unwrap_err();
        assert_eq!(refused.code(), crate::TEMPLATE_PLAN_REFUSED_REFUSAL.code);
        assert!(!doors.calls.iter().any(|call| call.starts_with("create")));
        let dry = run(&mut doors, "czgmdwth_gisagara", request(), true).unwrap();
        assert_eq!(dry["plan"]["ready"], false);
        assert_eq!(
            dry["plan"]["refusals"][0]["code"],
            "template_not_in_template_state"
        );
        assert!(render(&dry).contains("not ready"));
    }

    /// One item failing after the creation is stated, and the run goes on.
    #[test]
    fn a_failed_item_after_creation_is_stated_and_the_rest_still_runs() {
        let mut doors = FakeDoors::template("template");
        doors.network_refusal = Some("network_documents_plan_changed");
        let answer = run(&mut doors, "czgmdwth_gisagara", request(), false).unwrap();
        let applied = &answer["applied"];
        assert_eq!(applied["network_documents"]["outcome"], "failed");
        assert_eq!(
            applied["network_documents"]["code"],
            "network_documents_plan_changed"
        );
        assert_eq!(applied["complete"], false);
        assert!(
            doors
                .calls
                .iter()
                .any(|call| call.starts_with("migrate dsgrid"))
        );
    }

    /// A template that holds no network document copies none; that is
    /// stated, and the run is still complete.
    #[test]
    fn a_template_without_network_documents_reports_them_absent() {
        let mut doors = FakeDoors::template("template");
        doors.network_refusal = Some(crate::NETWORK_DOCUMENTS_SOURCE_MISSING_REFUSAL.code);
        let answer = run(&mut doors, "czgmdwth_gisagara", request(), false).unwrap();
        assert_eq!(answer["applied"]["network_documents"]["outcome"], "absent");
        assert_eq!(answer["applied"]["complete"], true);
    }

    #[test]
    fn a_template_outside_the_directory_is_refused_before_any_read() {
        let mut doors = FakeDoors::template("template");
        let refused = run(&mut doors, "someone_else", request(), true).unwrap_err();
        assert_eq!(refused.code(), crate::TEMPLATE_NOT_VISIBLE_REFUSAL.code);
        assert_eq!(doors.calls, ["directory someone_else"]);
    }

    #[test]
    fn models_are_read_page_by_page_until_the_catalog_says_no_more() {
        let mut pages = vec![
            json!({"models":[{"model_id":"m2"}],"more":false,"next_cursor":""}),
            json!({"models":[{"model_id":"m1"}],"more":true,"next_cursor":"c1"}),
        ];
        let mut cursors = Vec::new();
        let models = models_from(|cursor| {
            cursors.push(cursor);
            Ok(grid_models::Receipt {
                data: pages.pop().unwrap(),
                bytes: None,
            })
        })
        .unwrap();
        assert_eq!(models, ["m1", "m2"]);
        assert_eq!(cursors, [None, Some("c1".to_owned())]);
    }

    /// A read-only dry run against a local ds-brain, which the native profile
    /// cannot address (its origin is a gateway). The doors read the same
    /// routes the gateway serves; every write door panics, so the dry run is
    /// proven to reach none. Run:
    ///
    /// ```text
    /// DS_TEMPLATE_LIVE_BRAIN=http://127.0.0.1:8080 \
    /// DS_TEMPLATE_LIVE_TOKEN_FILE=<id-token file> \
    /// DS_TEMPLATE_LIVE_TEMPLATE=czgmdwth_gisagara \
    /// DS_TEMPLATE_LIVE_NAME="Gisagara II" DS_TEMPLATE_LIVE_OUT=<plan.json> \
    /// cargo test -p ds-cli-auth --lib live_local_dry_run -- --ignored
    /// ```
    #[test]
    #[ignore = "needs a local ds-brain, a signed-in ID token and a template id"]
    fn live_local_dry_run() {
        struct LocalDoors {
            base: String,
            token: String,
        }
        impl LocalDoors {
            fn get(&self, path: &str) -> Value {
                let mut response = ureq::get(format!("{}{path}", self.base))
                    .header("Authorization", &format!("Bearer {}", self.token))
                    .call()
                    .expect("local read");
                response.body_mut().read_json().expect("json")
            }
            fn post(&self, path: &str, body: Value) -> Value {
                let mut response = ureq::post(format!("{}{path}", self.base))
                    .header("Authorization", &format!("Bearer {}", self.token))
                    .send_json(body)
                    .expect("local read");
                response.body_mut().read_json().expect("json")
            }
        }
        impl Doors for LocalDoors {
            fn template(&mut self, project: &str) -> Result<Template, Failure> {
                for status in ["active", "archived", "testing", "template"] {
                    let listed = self.get(&format!("/api/v1/user/projects?status={status}"));
                    assert_eq!(
                        listed["data"]["status"], status,
                        "the brain serves {status}"
                    );
                    let rows = listed["data"]["projects"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if let Some(row) = rows.iter().find(|row| row["eds_project_id"] == project) {
                        let text = |key: &str| row[key].as_str().map(str::to_owned);
                        return Ok(Template {
                            project: project.to_owned(),
                            lifecycle_state: text("lifecycle_state").expect("bucket"),
                            display_name: text("display_name"),
                            country: text("country"),
                            client: text("client"),
                            network_template: text("network_template"),
                            styling_template_id: text("styling_template_id"),
                            project_params: row["project_params"].clone(),
                            project_components: row["project_components"].clone(),
                            project_phases: row["project_phases"].clone(),
                            processing_lanes: row["processing_lanes"].clone(),
                        });
                    }
                }
                Err(Failure::invalid(
                    crate::TEMPLATE_NOT_VISIBLE_REFUSAL.code,
                    "not visible",
                ))
            }
            fn copy_network_documents(&mut self, _: &str, _: &str) -> Result<Value, Failure> {
                panic!("a dry run reaches no write door")
            }
            fn printing_setups(&mut self, project: &str) -> Result<Vec<PrintingSetup>, Failure> {
                let listed = self.post(
                    "/api/v1/printing",
                    json!({"action":"list","scope":"project","project_id":project}),
                );
                Ok(setups_from(&listed["data"]))
            }
            fn copy_printing_setup(&mut self, _: &str, _: &PrintingRequest) -> Result<(), Failure> {
                panic!("a dry run reaches no write door")
            }
            fn transformers(&mut self, project: &str) -> Result<Vec<Transformer>, Failure> {
                let inventory = self.post(
                    "/report",
                    json!({"action":"transformer_inventory","eds_project_id":project}),
                );
                Ok(inventory["transformers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| Transformer {
                        name: row["name"].as_str().unwrap_or_default().to_owned(),
                        kind: row["kind"].as_str().unwrap_or_default().to_owned(),
                        lifecycle: row["state"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect())
            }
            fn dsgrid_models(&mut self, project: &str) -> Result<Vec<String>, Failure> {
                models_from(|cursor| {
                    let listed = self.post(
                        "/api/v1/grid/models",
                        json!({"action":"list_models","project_id":project,"page_size":100,
                            "cursor":cursor.unwrap_or_default(),"include_deleted":false}),
                    );
                    let data = &listed["data"];
                    Ok(grid_models::Receipt {
                        data: json!({"models": data["models"], "more": data["has_more"],
                            "next_cursor": data["next_cursor"]}),
                        bytes: None,
                    })
                })
            }
            fn create(&mut self, _: &NewProject) -> Result<Created, Failure> {
                panic!("a dry run reaches no write door")
            }
            fn migrate(
                &mut self,
                _: &str,
                _: &design_migration::Command,
            ) -> Result<Value, Failure> {
                panic!("a dry run reaches no write door")
            }
        }
        let env = |key: &str| std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"));
        let mut doors = LocalDoors {
            base: env("DS_TEMPLATE_LIVE_BRAIN"),
            token: std::fs::read_to_string(env("DS_TEMPLATE_LIVE_TOKEN_FILE"))
                .expect("token file")
                .trim()
                .to_owned(),
        };
        let request = Request {
            display_name: env("DS_TEMPLATE_LIVE_NAME"),
            location: None,
            description: None,
            country: None,
            client: None,
        };
        let answer = run(&mut doors, &env("DS_TEMPLATE_LIVE_TEMPLATE"), request, true).unwrap();
        assert!(answer["applied"].is_null());
        let out = env("DS_TEMPLATE_LIVE_OUT");
        std::fs::write(&out, serde_json::to_vec_pretty(&answer).unwrap()).unwrap();
        std::fs::write(format!("{out}.txt"), render(&answer)).unwrap();
    }
}
