//! The command registry: the one place a command becomes reachable.
//!
//! Registering a command is a single act that makes it dispatchable,
//! discoverable, documented and completable at once. There is no second list
//! to keep in step — `ds capabilities` and `ds <domain> --help` walk exactly
//! this table, so a command cannot exist without being described, and cannot
//! be described without existing.
//!
//! Domains are listed in the order the root help prints them. That order is
//! part of the interface: an agent that has seen root help once should find
//! the same domain in the same place next time.

use ds_cli_contract::spec::{Command, Domain};
use ds_cli_contract::{Context, Failure, Handler, Inputs};
use serde_json::Value;

use crate::meta;

/// One registered command: its contract, how to run it, and how to show it
/// to a person. The renderer is separate from the handler so human output is
/// always a projection of the machine result, never a parallel computation.
pub struct Entry {
    pub command: &'static Command,
    pub handler: Handler,
    pub render: fn(&Value) -> String,
}

pub struct Registered {
    pub domain: &'static Domain,
    pub entries: &'static [Entry],
}

/// Every domain, in root-help order. Static because the table is the
/// interface: it is walked by dispatch, by help and by the contract tests,
/// and all three must be looking at the same thing.
static DSGRID_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid::inspect::COMMAND,
        handler: ds_cli_dsgrid::inspect::run,
        render: ds_cli_dsgrid::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid::validate::COMMAND,
        handler: ds_cli_dsgrid::validate::run,
        render: ds_cli_dsgrid::validate::render,
    },
    Entry {
        command: &ds_cli_dsgrid::describe::COMMAND,
        handler: ds_cli_dsgrid::describe::run,
        render: ds_cli_dsgrid::describe::render,
    },
    Entry {
        command: &ds_cli_dsgrid::apply::COMMAND,
        handler: ds_cli_dsgrid::apply::run,
        render: ds_cli_dsgrid::apply::render,
    },
];

/// The exchange domain lists its commands in the order they are meant to be
/// called: classify, then plan, then convert. Domain help prints this order
/// verbatim, so the index doubles as the procedure — a reader who works down
/// the list is following the safe sequence rather than reconstructing it.
static DSGRID_EXCHANGE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid_exchange::inspect::COMMAND,
        handler: ds_cli_dsgrid_exchange::inspect::run,
        render: ds_cli_dsgrid_exchange::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::plan::COMMAND,
        handler: ds_cli_dsgrid_exchange::plan::run,
        render: ds_cli_dsgrid_exchange::plan::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::convert::COMMAND,
        handler: ds_cli_dsgrid_exchange::convert::run,
        render: ds_cli_dsgrid_exchange::convert::render,
    },
];

static LIBRARY_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_library::verify::COMMAND,
        handler: ds_cli_library::verify::run,
        render: ds_cli_library::verify::render,
    },
    Entry {
        command: &ds_cli_library::open::COMMAND,
        handler: ds_cli_library::open::run,
        render: ds_cli_library::open::render,
    },
    Entry {
        command: &ds_cli_library::catalog::COMMAND,
        handler: ds_cli_library::catalog::run,
        render: ds_cli_library::catalog::render,
    },
    Entry {
        command: &ds_cli_library::pack::COMMAND,
        handler: ds_cli_library::pack::run,
        render: ds_cli_library::pack::render,
    },
    Entry {
        command: &ds_cli_library::unpack::COMMAND,
        handler: ds_cli_library::unpack::run,
        render: ds_cli_library::unpack::render,
    },
    Entry {
        command: &ds_cli_library::seed::COMMAND,
        handler: ds_cli_library::seed::run,
        render: ds_cli_library::seed::render,
    },
    Entry {
        command: &ds_cli_library::resolve_native::COMMAND,
        handler: ds_cli_library::resolve_native::run,
        render: ds_cli_library::resolve_native::render,
    },
];

static PLS_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_pls::backup_create::COMMAND,
        handler: ds_cli_pls::backup_create::run,
        render: ds_cli_pls::backup_create::render,
    },
    Entry {
        command: &ds_cli_pls::pole_capacity::COMMAND,
        handler: ds_cli_pls::pole_capacity::run,
        render: ds_cli_pls::pole_capacity::render,
    },
    Entry {
        command: &ds_cli_pls::reference_closure::COMMAND,
        handler: ds_cli_pls::reference_closure::run,
        render: ds_cli_pls::reference_closure::render,
    },
    Entry {
        command: &ds_cli_pls::section_orientation::COMMAND,
        handler: ds_cli_pls::section_orientation::run,
        render: ds_cli_pls::section_orientation::render,
    },
    Entry {
        command: &ds_cli_pls::compare_don::COMMAND,
        handler: ds_cli_pls::compare_don::run,
        render: ds_cli_pls::compare_don::render,
    },
    Entry {
        command: &ds_cli_pls::terrain_reconcile::COMMAND,
        handler: ds_cli_pls::terrain_reconcile::run,
        render: ds_cli_pls::terrain_reconcile::render,
    },
    Entry {
        command: &ds_cli_pls::deviation_labels::COMMAND,
        handler: ds_cli_pls::deviation_labels::run,
        render: ds_cli_pls::deviation_labels::render,
    },
    Entry {
        command: &ds_cli_pls::delivery_verify::COMMAND,
        handler: ds_cli_pls::delivery_verify::run,
        render: ds_cli_pls::delivery_verify::render,
    },
];

static REPORT_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_report::engine::COMMAND,
        handler: ds_cli_report::engine::run,
        render: ds_cli_report::engine::render,
    },
    Entry {
        command: &ds_cli_report::tasks::COMMAND,
        handler: ds_cli_report::tasks::run,
        render: ds_cli_report::tasks::render,
    },
    Entry {
        command: &ds_cli_report::export::COMMAND,
        handler: ds_cli_report::export::run,
        render: ds_cli_report::export::render,
    },
    Entry {
        command: &ds_cli_report::bundle::COMMAND,
        handler: ds_cli_report::bundle::run,
        render: ds_cli_report::bundle::render,
    },
];

static SOLAR_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_solar::engine::COMMAND,
        handler: ds_cli_solar::engine::run,
        render: ds_cli_solar::engine::render,
    },
    Entry {
        command: &ds_cli_solar::prepare::COMMAND,
        handler: ds_cli_solar::prepare::run,
        render: ds_cli_solar::prepare::render,
    },
    Entry {
        command: &ds_cli_solar::run::COMMAND,
        handler: ds_cli_solar::run::run,
        render: ds_cli_solar::run::render,
    },
    Entry {
        command: &ds_cli_solar::paired_run::START_COMMAND,
        handler: ds_cli_solar::paired_run::start,
        render: ds_cli_solar::paired_run::render_start,
    },
    Entry {
        command: &ds_cli_solar::paired_run::PROGRESS_COMMAND,
        handler: ds_cli_solar::paired_run::progress,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::RESULT_COMMAND,
        handler: ds_cli_solar::paired_run::result,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::CANCEL_COMMAND,
        handler: ds_cli_solar::paired_run::cancel,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::paired_run::READ_COMMAND,
        handler: ds_cli_solar::paired_run::read,
        render: ds_cli_solar::paired_run::render_receipt,
    },
    Entry {
        command: &ds_cli_solar::workflow::RESULTS_READ_COMMAND,
        handler: ds_cli_solar::workflow::results_read,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::SYNC_STATUS_COMMAND,
        handler: ds_cli_solar::workflow::sync_status,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::PORTFOLIO_LIST_COMMAND,
        handler: ds_cli_solar::workflow::portfolio_list,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::PORTFOLIO_READ_COMMAND,
        handler: ds_cli_solar::workflow::portfolio_read,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::FINAL_IMPORT_COMMAND,
        handler: ds_cli_solar::workflow::final_import,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::workflow::FINAL_SUBMIT_COMMAND,
        handler: ds_cli_solar::workflow::final_submit,
        render: ds_cli_solar::workflow::render,
    },
    Entry {
        command: &ds_cli_solar::exports::REPORT_EXPORT_COMMAND,
        handler: ds_cli_solar::exports::export_report,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::exports::PORTFOLIO_EXPORT_COMMAND,
        handler: ds_cli_solar::exports::export_portfolio,
        render: ds_cli_solar::exports::render,
    },
    Entry {
        command: &ds_cli_solar::weather::COMMAND,
        handler: ds_cli_solar::weather::run,
        render: ds_cli_solar::weather::render,
    },
];

static MAP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_map::view::COMMAND,
        handler: ds_cli_map::view::run,
        render: ds_cli_map::view::render,
    },
    Entry {
        command: &ds_cli_map::draw::COMMAND,
        handler: ds_cli_map::draw::run,
        render: ds_cli_map::draw::render,
    },
    Entry {
        command: &ds_cli_map::remove::COMMAND,
        handler: ds_cli_map::remove::run,
        render: ds_cli_map::remove::render,
    },
    Entry {
        command: &ds_cli_map::zoom::COMMAND,
        handler: ds_cli_map::zoom::run,
        render: ds_cli_map::zoom::render,
    },
    Entry {
        command: &ds_cli_map::layer::list::COMMAND,
        handler: ds_cli_map::layer::list::run,
        render: ds_cli_map::layer::list::render,
    },
    Entry {
        command: &ds_cli_map::layer::reorder::COMMAND,
        handler: ds_cli_map::layer::reorder::run,
        render: ds_cli_map::layer::reorder::render,
    },
    Entry {
        command: &ds_cli_map::layer::remote_list::COMMAND,
        handler: ds_cli_map::layer::remote_list::run,
        render: ds_cli_map::layer::remote_list::render,
    },
    Entry {
        command: &ds_cli_map::layer::add::COMMAND,
        handler: ds_cli_map::layer::add::run,
        render: ds_cli_map::layer::add::render,
    },
    Entry {
        command: &ds_cli_map::layer::remove::COMMAND,
        handler: ds_cli_map::layer::remove::run,
        render: ds_cli_map::layer::remove::render,
    },
    Entry {
        command: &ds_cli_map::layer::visibility::COMMAND,
        handler: ds_cli_map::layer::visibility::run,
        render: ds_cli_map::layer::visibility::render,
    },
    Entry {
        command: &ds_cli_map::ui::open::COMMAND,
        handler: ds_cli_map::ui::open::run,
        render: ds_cli_map::ui::open::render,
    },
    Entry {
        command: &ds_cli_map::evidence::capture::COMMAND,
        handler: ds_cli_map::evidence::capture::run,
        render: ds_cli_map::evidence::capture::render,
    },
    Entry {
        command: &ds_cli_map::points_along::COMMAND,
        handler: ds_cli_map::points_along::run,
        render: ds_cli_map::points_along::render,
    },
    Entry {
        command: &ds_cli_map::random_points::COMMAND,
        handler: ds_cli_map::random_points::run,
        render: ds_cli_map::random_points::render,
    },
    Entry {
        command: &ds_cli_map::outliers::COMMAND,
        handler: ds_cli_map::outliers::run,
        render: ds_cli_map::outliers::render,
    },
    Entry {
        command: &ds_cli_map::line_difference::COMMAND,
        handler: ds_cli_map::line_difference::run,
        render: ds_cli_map::line_difference::render,
    },
    Entry {
        command: &ds_cli_map::survey::download::COMMAND,
        handler: ds_cli_map::survey::download::run,
        render: ds_cli_map::survey::download::render,
    },
    Entry {
        command: &ds_cli_map::survey::plan::COMMAND,
        handler: ds_cli_map::survey::plan::run,
        render: ds_cli_map::survey::plan::render,
    },
    Entry {
        command: &ds_cli_map::survey::apply::COMMAND,
        handler: ds_cli_map::survey::apply::run,
        render: ds_cli_map::survey::apply::render,
    },
    Entry {
        command: &ds_cli_map::design::read::COMMAND,
        handler: ds_cli_map::design::read::run,
        render: ds_cli_map::design::read::render,
    },
    Entry {
        command: &ds_cli_map::design::discard::COMMAND,
        handler: ds_cli_map::design::discard::run,
        render: ds_cli_map::design::discard::render,
    },
    Entry {
        command: &ds_cli_map::design::layer_to_local::COMMAND,
        handler: ds_cli_map::design::layer_to_local::run,
        render: ds_cli_map::design::layer_to_local::render,
    },
    Entry {
        command: &ds_cli_map::design::upload_to_local::COMMAND,
        handler: ds_cli_map::design::upload_to_local::run,
        render: ds_cli_map::design::upload_to_local::render,
    },
    Entry {
        command: &ds_cli_map::design::select::COMMAND,
        handler: ds_cli_map::design::select::run,
        render: ds_cli_map::design::select::render,
    },
    Entry {
        command: &ds_cli_map::design::set::COMMAND,
        handler: ds_cli_map::design::set::run,
        render: ds_cli_map::design::set::render,
    },
    Entry {
        command: &ds_cli_map::design::create::COMMAND,
        handler: ds_cli_map::design::create::run,
        render: ds_cli_map::design::create::render,
    },
    Entry {
        command: &ds_cli_map::design::delete::COMMAND,
        handler: ds_cli_map::design::delete::run,
        render: ds_cli_map::design::delete::render,
    },
    Entry {
        command: &ds_cli_map::design::geometry::COMMAND,
        handler: ds_cli_map::design::geometry::run,
        render: ds_cli_map::design::geometry::render,
    },
    Entry {
        command: &ds_cli_map::design::process_setup::COMMAND,
        handler: ds_cli_map::design::process_setup::run,
        render: ds_cli_map::design::process_setup::render,
    },
    Entry {
        command: &ds_cli_map::design::version_create::COMMAND,
        handler: ds_cli_map::design::version_create::run,
        render: ds_cli_map::design::version_create::render,
    },
    Entry {
        command: &ds_cli_map::design::process::COMMAND,
        handler: ds_cli_map::design::process::run,
        render: ds_cli_map::design::process::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_process::COMMAND,
        handler: ds_cli_map::design::batch_process::run,
        render: ds_cli_map::design::batch_process::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_report::COMMAND,
        handler: ds_cli_map::design::batch_report::run,
        render: ds_cli_map::design::batch_report::render,
    },
    Entry {
        command: &ds_cli_map::design::batch_save::COMMAND,
        handler: ds_cli_map::design::batch_save::run,
        render: ds_cli_map::design::batch_save::render,
    },
    Entry {
        command: &ds_cli_map::design::save::COMMAND,
        handler: ds_cli_map::design::save::run,
        render: ds_cli_map::design::save::render,
    },
    Entry {
        command: &ds_cli_map::design::list::COMMAND,
        handler: ds_cli_map::design::list::run,
        render: ds_cli_map::design::list::render,
    },
    Entry {
        command: &ds_cli_map::design::report::COMMAND,
        handler: ds_cli_map::design::report::run,
        render: ds_cli_map::design::report::render,
    },
    Entry {
        command: &ds_cli_map::design::attach_print::COMMAND,
        handler: ds_cli_map::design::attach_print::run,
        render: ds_cli_map::design::attach_print::render,
    },
    Entry {
        command: &ds_cli_map::design::upload::COMMAND,
        handler: ds_cli_map::design::upload::run,
        render: ds_cli_map::design::upload::render,
    },
    Entry {
        command: &ds_cli_map::design::upload_stage::COMMAND,
        handler: ds_cli_map::design::upload_stage::run,
        render: ds_cli_map::design::upload_stage::render,
    },
];

static SURVEY_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_survey::forms::LIST_COMMAND,
        handler: ds_cli_survey::forms::list,
        render: ds_cli_survey::forms::render_list,
    },
    Entry {
        command: &ds_cli_survey::forms::READ_COMMAND,
        handler: ds_cli_survey::forms::read,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::TYPES_COMMAND,
        handler: ds_cli_survey::forms::types,
        render: ds_cli_survey::forms::render_types,
    },
    Entry {
        command: &ds_cli_survey::forms::CREATE_COMMAND,
        handler: ds_cli_survey::forms::create,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::UPDATE_COMMAND,
        handler: ds_cli_survey::forms::update,
        render: ds_cli_survey::forms::render_form,
    },
    Entry {
        command: &ds_cli_survey::forms::LIFECYCLE_COMMAND,
        handler: ds_cli_survey::forms::lifecycle,
        render: ds_cli_survey::forms::render_lifecycle,
    },
    Entry {
        command: &ds_cli_survey::project_forms::READ_COMMAND,
        handler: ds_cli_survey::project_forms::read,
        render: ds_cli_survey::project_forms::render_read,
    },
    Entry {
        command: &ds_cli_survey::project_forms::EDITOR_COMMAND,
        handler: ds_cli_survey::project_forms::editor,
        render: ds_cli_survey::project_forms::render_editor,
    },
    Entry {
        command: &ds_cli_survey::project_forms::PLAN_COMMAND,
        handler: ds_cli_survey::project_forms::plan,
        render: ds_cli_survey::project_forms::render_plan,
    },
    Entry {
        command: &ds_cli_survey::project_forms::APPLY_COMMAND,
        handler: ds_cli_survey::project_forms::apply,
        render: ds_cli_survey::project_forms::render_apply,
    },
    Entry {
        command: &ds_cli_survey::templates::LIST_COMMAND,
        handler: ds_cli_survey::templates::list,
        render: ds_cli_survey::templates::render_list,
    },
    Entry {
        command: &ds_cli_survey::templates::READ_COMMAND,
        handler: ds_cli_survey::templates::read,
        render: ds_cli_survey::templates::render_template,
    },
    Entry {
        command: &ds_cli_survey::templates::CREATE_COMMAND,
        handler: ds_cli_survey::templates::create,
        render: ds_cli_survey::templates::render_template,
    },
    Entry {
        command: &ds_cli_survey::templates::APPLY_COMMAND,
        handler: ds_cli_survey::templates::apply,
        render: ds_cli_survey::templates::render_mutation,
    },
    Entry {
        command: &ds_cli_survey::templates::LIFECYCLE_COMMAND,
        handler: ds_cli_survey::templates::lifecycle,
        render: ds_cli_survey::templates::render_mutation,
    },
    Entry {
        command: &ds_cli_survey::templates::CREATE_PROJECT_COMMAND,
        handler: ds_cli_survey::templates::create_project,
        render: ds_cli_survey::templates::render_project,
    },
];

/// Map styling. Ordered as a session uses it: list the refs, read one, author
/// guided base appearance, then plan, publish or clear a second dimension, and
/// last the cartography — line type, direction, casing and hatching — which
/// needs no field and so is reached once the field-driven axes are settled.
static STYLE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_style::list::COMMAND,
        handler: ds_cli_style::list::run,
        render: ds_cli_style::list::render,
    },
    Entry {
        command: &ds_cli_style::read::COMMAND,
        handler: ds_cli_style::read::run,
        render: ds_cli_style::read::render,
    },
    Entry {
        command: &ds_cli_style::appearance::plan::COMMAND,
        handler: ds_cli_style::appearance::plan::run,
        render: ds_cli_style::appearance::plan::render,
    },
    Entry {
        command: &ds_cli_style::appearance::set::COMMAND,
        handler: ds_cli_style::appearance::set::run,
        render: ds_cli_style::appearance::set::render,
    },
    Entry {
        command: &ds_cli_style::dimension::plan::COMMAND,
        handler: ds_cli_style::dimension::plan::run,
        render: ds_cli_style::dimension::plan::render,
    },
    Entry {
        command: &ds_cli_style::dimension::set::COMMAND,
        handler: ds_cli_style::dimension::set::run,
        render: ds_cli_style::dimension::set::render,
    },
    Entry {
        command: &ds_cli_style::dimension::clear::COMMAND,
        handler: ds_cli_style::dimension::clear::run,
        render: ds_cli_style::dimension::clear::render,
    },
    Entry {
        command: &ds_cli_style::cartography::plan::COMMAND,
        handler: ds_cli_style::cartography::plan::run,
        render: ds_cli_style::cartography::plan::render,
    },
    Entry {
        command: &ds_cli_style::cartography::set::COMMAND,
        handler: ds_cli_style::cartography::set::run,
        render: ds_cli_style::cartography::set::render,
    },
];

/// Tiling. Ordered as a session uses it: read the state, look at the
/// sources, decide, run; then the catalogue.
static TILE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_tile::status::COMMAND,
        handler: ds_cli_tile::status::run,
        render: ds_cli_tile::status::render,
    },
    Entry {
        command: &ds_cli_tile::preflight::COMMAND,
        handler: ds_cli_tile::preflight::run,
        render: ds_cli_tile::preflight::render,
    },
    Entry {
        command: &ds_cli_tile::plan::COMMAND,
        handler: ds_cli_tile::plan::run,
        render: ds_cli_tile::plan::render,
    },
    Entry {
        command: &ds_cli_tile::generate::COMMAND,
        handler: ds_cli_tile::generate::run,
        render: ds_cli_tile::generate::render,
    },
    Entry {
        command: &ds_cli_tile::list::COMMAND,
        handler: ds_cli_tile::list::run,
        render: ds_cli_tile::list::render,
    },
    Entry {
        command: &ds_cli_tile::add::COMMAND,
        handler: ds_cli_tile::add::run,
        render: ds_cli_tile::add::render,
    },
    Entry {
        command: &ds_cli_tile::remove::COMMAND,
        handler: ds_cli_tile::remove::run,
        render: ds_cli_tile::remove::render,
    },
];

/// The Project Work domain lists its commands in the order a session uses
/// them: look at the plan, find the item, read it, then act on it. Domain
/// help prints this order verbatim, so the index doubles as the procedure.
static WORK_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_work::plan::COMMAND,
        handler: ds_cli_work::plan::run,
        render: ds_cli_work::plan::render,
    },
    Entry {
        command: &ds_cli_work::task::list::COMMAND,
        handler: ds_cli_work::task::list::run,
        render: ds_cli_work::task::list::render,
    },
    Entry {
        command: &ds_cli_work::task::read::COMMAND,
        handler: ds_cli_work::task::read::run,
        render: ds_cli_work::task::read::render,
    },
    Entry {
        command: &ds_cli_work::task::create::COMMAND,
        handler: ds_cli_work::task::create::run,
        render: ds_cli_work::task::create::render,
    },
    Entry {
        command: &ds_cli_work::task::update::COMMAND,
        handler: ds_cli_work::task::update::run,
        render: ds_cli_work::task::update::render,
    },
    Entry {
        command: &ds_cli_work::task::assign::COMMAND,
        handler: ds_cli_work::task::assign::run,
        render: ds_cli_work::task::assign::render,
    },
    Entry {
        command: &ds_cli_work::task::respond::COMMAND,
        handler: ds_cli_work::task::respond::run,
        render: ds_cli_work::task::respond::render,
    },
    Entry {
        command: &ds_cli_work::record::list::COMMAND,
        handler: ds_cli_work::record::list::run,
        render: ds_cli_work::record::list::render,
    },
    Entry {
        command: &ds_cli_work::record::read::COMMAND,
        handler: ds_cli_work::record::read::run,
        render: ds_cli_work::record::read::render,
    },
];

static SRE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_sre::overview::COMMAND,
        handler: ds_cli_sre::overview::run,
        render: ds_cli_sre::overview::render,
    },
    Entry {
        command: &ds_cli_sre::events::COMMAND,
        handler: ds_cli_sre::events::run,
        render: ds_cli_sre::events::render,
    },
];

static DESKTOP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_desktop::status::COMMAND,
        handler: ds_cli_desktop::status::run,
        render: ds_cli_desktop::status::render,
    },
    Entry {
        command: &ds_cli_desktop::project::LIST_COMMAND,
        handler: ds_cli_desktop::project::list,
        render: ds_cli_desktop::project::render_list,
    },
    Entry {
        command: &ds_cli_desktop::project::SWITCH_COMMAND,
        handler: ds_cli_desktop::project::switch,
        render: ds_cli_desktop::project::render_switch,
    },
];

/// Feedback is a loop: report a gap, find it again once a session has fixed
/// it, and close it with what changed. The order is the loop.
static FEEDBACK_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_feedback::submit::COMMAND,
        handler: ds_cli_feedback::submit::run,
        render: ds_cli_feedback::submit::render,
    },
    Entry {
        command: &ds_cli_feedback::list::COMMAND,
        handler: ds_cli_feedback::list::run,
        render: ds_cli_feedback::list::render,
    },
    Entry {
        command: &ds_cli_feedback::close::COMMAND,
        handler: ds_cli_feedback::close::run,
        render: ds_cli_feedback::close::render,
    },
];

/// The shell domain lists its commands in the order a person needs them:
/// look, register, and — rarely — undo.
static SHELL_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_shell::status::COMMAND,
        handler: ds_cli_shell::status::run,
        render: ds_cli_shell::status::render,
    },
    Entry {
        command: &ds_cli_shell::register::COMMAND,
        handler: ds_cli_shell::register::run,
        render: ds_cli_shell::register::render,
    },
    Entry {
        command: &ds_cli_shell::unregister::COMMAND,
        handler: ds_cli_shell::unregister::run,
        render: ds_cli_shell::unregister::render,
    },
];

/// One setup door: inspect and plan before either proven machine mutation.
static WORKSTATION_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_workstation::status::COMMAND,
        handler: ds_cli_workstation::status::run,
        render: ds_cli_workstation::status::render,
    },
    Entry {
        command: &ds_cli_workstation::plan::COMMAND,
        handler: ds_cli_workstation::plan::run,
        render: ds_cli_workstation::plan::render,
    },
    Entry {
        command: &ds_cli_workstation::install::COMMAND,
        handler: ds_cli_workstation::install::run,
        render: ds_cli_workstation::install::render,
    },
    Entry {
        command: &ds_cli_workstation::configure::COMMAND,
        handler: ds_cli_workstation::configure::run,
        render: ds_cli_workstation::configure::render,
    },
    Entry {
        command: &ds_cli_workstation::verify::COMMAND,
        handler: ds_cli_workstation::verify::run,
        render: ds_cli_workstation::verify::render,
    },
    Entry {
        command: &ds_cli_workstation::components::COMMAND,
        handler: ds_cli_workstation::components::run,
        render: ds_cli_workstation::components::render,
    },
];

static MCP_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_mcp::serve::COMMAND,
        handler: ds_cli_mcp::serve::run,
        render: ds_cli_mcp::serve::render,
    },
    Entry {
        command: &ds_cli_mcp::install::COMMAND,
        handler: ds_cli_mcp::install::run,
        render: ds_cli_mcp::install::render,
    },
];

static DOMAINS: &[Registered] = &[
    Registered {
        domain: &ds_cli_dsgrid::DOMAIN,
        entries: DSGRID_ENTRIES,
    },
    Registered {
        domain: &ds_cli_dsgrid_exchange::DOMAIN,
        entries: DSGRID_EXCHANGE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_library::DOMAIN,
        entries: LIBRARY_ENTRIES,
    },
    Registered {
        domain: &ds_cli_pls::DOMAIN,
        entries: PLS_ENTRIES,
    },
    Registered {
        domain: &ds_cli_solar::DOMAIN,
        entries: SOLAR_ENTRIES,
    },
    Registered {
        domain: &ds_cli_report::DOMAIN,
        entries: REPORT_ENTRIES,
    },
    Registered {
        domain: &ds_cli_survey::DOMAIN,
        entries: SURVEY_ENTRIES,
    },
    Registered {
        domain: &ds_cli_map::DOMAIN,
        entries: MAP_ENTRIES,
    },
    Registered {
        domain: &ds_cli_work::DOMAIN,
        entries: WORK_ENTRIES,
    },
    Registered {
        domain: &ds_cli_sre::DOMAIN,
        entries: SRE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_style::DOMAIN,
        entries: STYLE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_tile::DOMAIN,
        entries: TILE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_feedback::DOMAIN,
        entries: FEEDBACK_ENTRIES,
    },
    Registered {
        domain: &ds_cli_desktop::DOMAIN,
        entries: DESKTOP_ENTRIES,
    },
    Registered {
        domain: &ds_cli_shell::DOMAIN,
        entries: SHELL_ENTRIES,
    },
    Registered {
        domain: &ds_cli_workstation::DOMAIN,
        entries: WORKSTATION_ENTRIES,
    },
    Registered {
        domain: &ds_cli_mcp::DOMAIN,
        entries: MCP_ENTRIES,
    },
];

pub fn domains() -> &'static [Registered] {
    DOMAINS
}

/// The root-level commands that describe the CLI itself. They are separate
/// from domains because they are not a subject area — `ds capabilities` is
/// not a kind of engineering work — and because they must stay reachable when
/// every domain is unavailable.
pub fn meta_commands() -> &'static [Entry] {
    meta::ENTRIES
}

pub fn find_domain(id: &str) -> Option<&'static Registered> {
    DOMAINS.iter().find(|registered| registered.domain.id == id)
}

/// Resolve a command from an invocation path, longest match first.
///
/// Returns the entry and how many tokens it consumed, so the caller knows
/// where the command's own arguments begin. Longest-first matters because a
/// domain may hold both `ds solar run` and a future `ds solar run status`:
/// resolving the shorter path first would make the longer one unreachable
/// and silently feed `status` to the wrong command as an operand.
pub fn find_by_path(tokens: &[String]) -> Option<(&'static Entry, usize)> {
    let mut best: Option<(&'static Entry, usize)> = None;
    for registered in DOMAINS {
        for entry in registered.entries {
            let path = entry.command.path;
            if path.len() > tokens.len() {
                continue;
            }
            if !path.iter().zip(tokens).all(|(part, token)| *part == token) {
                continue;
            }
            if best.is_none_or(|(_, consumed)| path.len() > consumed) {
                best = Some((entry, path.len()));
            }
        }
    }
    best
}

/// Resolve a dotted command id such as `dsgrid.inspect`.
pub fn find_by_id(id: &str) -> Option<&'static Entry> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .find(|entry| entry.command.id == id)
}

/// Every registered command, in registration order. Used by capability search
/// and by the contract tests that hold the whole surface to its rules.
pub fn all_commands() -> Vec<&'static Command> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .map(|entry| entry.command)
        .collect()
}

/// Run one entry: parse its declared inputs, enforce the confirmation policy
/// its effect class implies, then hand off. Confirmation is checked here, in
/// one place, so a handler cannot forget it.
pub fn dispatch(entry: &Entry, tokens: &[String], context: &Context) -> Result<Value, Failure> {
    let inputs: Inputs = ds_cli_contract::parse(entry.command, tokens)?;

    if entry.command.effect.needs_confirmation() && !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            format!(
                "`ds {}` {} and needs explicit confirmation",
                entry.command.path.join(" "),
                entry.command.effect.gloss()
            ),
        )
        .remedy("re-run with --yes once you intend the effect")
        .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    if let ds_cli_contract::Availability::Unavailable {
        code,
        reason,
        remedy,
    } = (entry.command.availability)()
    {
        return Err(Failure::unavailable(code, reason)
            .remedy(remedy)
            .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    (entry.handler)(&inputs, context)
}
